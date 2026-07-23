## Context

`PvInverter` (`VEN/src/assets/pv.rs`) already has an `export_limit_kw:
Option<f64>` field and its `step_inner`/`peek_pv_kw` physics already clamp
output to it (`raw_kw.max(lim)`, `lim ≤ 0`). Nothing sets this field in
production — it defaults to `None` at `from_params` and is only assigned
in unit tests. A separate, *different* mechanism in
`controller/dispatcher.rs::build_setpoints` computes a curtailed PV
setpoint from the VTN's `EXPORT_CAPACITY_LIMIT` signal
(`OadrCapacityState.export_limit_kw`) and writes it into the dispatcher's
setpoints map — a channel `PvInverter::step_inner` ignores entirely (its
parameter is literally named `_setpoint_kw`). So today, neither an
operator override nor the VTN's signal can actually curtail PV: the field
physics respects is never set, and the mechanism that computes a VTN-aware
value writes to a channel physics ignores.

**Scope note (revised mid-implementation):** the original framing treated
VTN-driven curtailment as a deferred follow-up and scoped only an operator
override. That was revisited once it became clear VTN-reactivity is the
primary motivation — both sources are now combined into the *same*
mechanism (see Decision 2). Also corrected: `PvInverter.export_limit_kw`
(the field) was mistakenly assumed dead alongside the dispatcher clamp; it
is not — only the dispatcher's separate `setpoint_kw`-based path was
truly dead. No change to `step_inner`'s handling of its `setpoint_kw`
parameter is made by this change.

The closest existing pattern for "a persistent, runtime-settable numeric
override that changes simulator physics and can trigger a replan" is
`grid_export_limit_kw` (now removed as dead code — see Decision 1):
`SimInjectState.grid_export_limit_kw` → `PostSimInjectBody` + `merge_f64!`
macro → replan-trigger check → applied each tick onto
`OadrCapacityState.export_limit_kw`. This design mirrors that shape but
targets `PvInverter.export_limit_kw` directly instead of routing through
`OadrCapacityState`.

## Goals / Non-Goals

**Goals:**
- A `pv_export_limit_kw` sim-inject field for an operator/UI-set PV
  export ceiling (kW, positive magnitude).
- Combine that with the VTN's `EXPORT_CAPACITY_LIMIT` signal
  (`OadrCapacityState.export_limit_kw`) into one effective ceiling per
  tick — whichever is more restrictive wins — threaded into
  `PvInverter.export_limit_kw` every simulator tick.
- Setting/clearing the operator override triggers an out-of-cycle replan.
- A UI control (persistent-override style, not decaying) so an operator
  can set/clear their override from the VEN UI Controller tab.
- Removal of the dead dispatcher-side PV clamp (superseded).

**Non-Goals:**
- No change to `AssetCapability`/`is_fixed()` for PV — capability keeps
  reporting `max_export_kw == max_import_kw`. This is deliberate: the
  planner still cannot request an arbitrary PV setpoint, only a ceiling
  can be imposed externally (by VTN or operator), so advertising a genuine
  range would misrepresent what the planner can actually achieve.
- No MILP solver changes; the planner's PV *forecast* input is not made
  ceiling-aware here (logged as `TECHNICAL_DEBTS.md` R-58) — only the
  simulator's live PV output responds to the ceiling immediately. Making
  PV a genuine MILP decision variable is Tier 2 scope.
- No history changes (`TickSample.power_kw` stays post-curtailment only).
- The now-dead `grid_import_limit_kw`/`grid_export_limit_kw` sim-inject
  fields were removed entirely rather than left as unused dead code (their
  only consumer was the dispatcher clamp this change deletes; confirmed
  `grid_import_limit_kw` had zero consumers even before this change).

## Decisions

**1. New sim-inject field, not reuse of `OadrCapacityState.export_limit_kw`.**
`OadrCapacityState.export_limit_kw` is VTN-signal-owned (populated from
parsed OpenADR events, `controller/openadr_interface.rs:267`) and is a
*grid-level* (site-wide) limit, not PV-specific. Overloading it for an
operator-set PV-only ceiling would conflate two different authorities
(VTN vs. local operator) over two different scopes (site vs. asset).
Instead, `pv_export_limit_kw` is a new, independent sim-inject field, and
the two authorities are explicitly *combined* (not conflated) at the
point of use — see Decision 2.

**2. Combine VTN capacity + operator override into one effective ceiling,
computed in `tasks/sim_tick/tick.rs`, threaded directly onto
`PvInverter.export_limit_kw` — not through the dispatcher's setpoint map.**
`effective_pv_export_ceiling_kw(operator_override_kw, vtn_capacity_kw)`
takes whichever source is more restrictive (`Option::min`, both
positive-magnitude). The dispatcher's setpoint-map path was proven dead
(PV physics ignores setpoints); since `PvInverter.export_limit_kw` is a
field physics already reads directly, the correct wiring is: combined
ceiling → `SimState::tick` argument → assigned onto
`AssetConfig::Pv(pv).export_limit_kw` each tick, in the same place
`weather_power_kw` and `pv_irradiance_override` are already threaded
(`simulator/mod.rs`). This makes both the VTN signal and the operator
override real with one mechanism, and gives `peek_pv_kw` (used by the
EV-surplus overlay) ceiling-awareness for free, since it already reads
`pv_cfg.export_limit_kw` — no separate plumbing needed there.

**3. Persistent-override UI pattern (heater temp-limit style), not
decaying Behaviour-B (irradiance-slider style).** `pv_irradiance` decays
back toward the sin-model each tick via `pv_alpha` — appropriate for a
transient "simulate a cloud passing" demo action. A curtailment ceiling is
a standing operational constraint that should stay in effect until
explicitly cleared, matching `heater_temp_min_c`/`heater_temp_max_c`'s
existing persistent-override handling in `AssetRightSection.tsx`. The
Controller slider's fallback reads the *live sim's* effective ceiling
(abs of the signed field), not just the operator's own override, so it
reflects whichever source is actually binding.

**4. Remove the dead dispatcher clamp, and the now-fully-dead
`grid_import_limit_kw`/`grid_export_limit_kw` sim-inject fields it was the
only consumer of, rather than leave dead weight in place.** Once the real
mechanism exists, the `dispatcher.rs` code computing an unused PV setpoint
clamp is pure dead weight. Removing it left `build_setpoints`'s `capacity`
parameter and `build_tick_setpoints`'s `effective_capacity`/grid-limit
merge with no remaining consumer — confirmed `grid_import_limit_kw` had
*zero* consumers even before this change (only `capacity.export_limit_kw`
was ever read). Both were deleted entirely, not just deprioritized,
per explicit direction: functionality that exists via a working mechanism
doesn't need a parallel unused one kept "just in case."

**5. Replan trigger: include `pv_export_limit_kw` in the sim-inject
event-trigger list (`routes/sim.rs`'s replan-check condition).** A changed
operator ceiling changes what the plan can assume PV will deliver in
upcoming slots; waiting for the next `Periodic` cycle (up to ~5 minutes,
per `replan_interval_s=300`) would leave the plan silently over-committing
PV export in the meantime. VTN capacity changes already trigger a replan
via their own existing event path, so only the operator field needed
adding here.

## Risks / Trade-offs

- **[Risk]** Adding a third control to `PvInverter.control_schema()`
  slightly grows `assets/pv.rs`, which is already a moderately-sized file
  under the 500-production-line cap. → **Mitigation**: the addition is a
  small descriptor struct literal (~10 lines), well within budget; run
  `scripts/audit_file_sizes.py` before merging to confirm.
- **[Risk]** Sign-convention confusion: `PvInverter.export_limit_kw` is
  stored `≤ 0` internally, but a UI slider for "curtail export to N kW"
  is naturally entered as a positive magnitude, matching how
  `grid_export_limit_kw`'s UI/route layer already handles the analogous
  sign flip for `capacity.export_limit_kw` (positive magnitude in the
  API/UI, negative-signed internally). → **Mitigation**: follow the exact
  existing sign-conversion convention documented at `assets/grid.rs:47-49`
  and `tasks/sim_tick/helpers.rs:200-201` (UI/API surfaces a positive
  magnitude; internal fields stay `≤ 0`), and add a unit test asserting
  the conversion.
- **[Risk]** Removing the dispatcher clamp, and the
  `grid_import_limit_kw`/`grid_export_limit_kw` sim-inject fields, could
  look like a regression if read in isolation (a reviewer unfamiliar with
  the "this code never worked" finding might assume capability is being
  removed). → **Mitigation**: proposal.md and design.md explicitly state
  both were dead code (confirmed zero real consumers), and the commit
  message states the same.
- **[Trade-off]** The planner's PV forecast input is not ceiling-aware
  (logged as `TECHNICAL_DEBTS.md` R-58) — only live simulator physics
  responds to the ceiling immediately, so the plan can transiently assume
  more PV export than physics will deliver until the next replan cycle.
  Accepted for this change's scope; planner-side awareness is Tier 2.

## Migration Plan

No data migration. `SimInjectState` gains one new `Option<f64>` field,
defaulting to `None` (backward compatible — existing `/data/sim_state.json`
files deserialize fine via serde default). No env vars, no Docker changes.
Deploy via the existing `docker compose build && up -d` cycle. Rollback is
a plain revert (no persisted-state format change to undo).

## Open Questions

None outstanding — scope, sign convention, and UI pattern were confirmed
during the planning conversation preceding this proposal.
