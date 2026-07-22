## Context

`PvInverter` (`VEN/src/assets/pv.rs`) already has an `export_limit_kw:
Option<f64>` field and its `step_inner` physics already clamps output to
it (`raw_kw.max(lim)`, `lim ≤ 0`). Nothing sets this field in production —
it defaults to `None` at `from_params` and is only assigned in unit tests.
A separate, unrelated clamp lives in `controller/dispatcher.rs::build_setpoints`
(enforces `capacity.export_limit_kw`, sourced from the VTN's
`EXPORT_CAPACITY_LIMIT` signal, onto the PV entry of the setpoints map),
but PV physics ignores its setpoint entirely (`step_inner`'s doc comment:
"Ignores setpoint (non-curtailable in Phase A)"), so that clamp has no
effect on simulated output. This proposal wires a *new*, independent
control path — an operator/UI-settable ceiling via sim-inject — directly
into the field PV physics already respects, and removes the dead
dispatcher clamp since it does nothing and is confusing to read.

The closest existing pattern for "a persistent, runtime-settable numeric
override that changes simulator physics and can trigger a replan" is
`grid_export_limit_kw`:
`SimInjectState.grid_export_limit_kw` (`entities/sim_inject.rs:27`) →
`PostSimInjectBody.grid_export_limit_kw` + `merge_f64!` macro
(`routes/sim.rs:50,97`) → replan-trigger check (`routes/sim.rs:232`) →
applied each tick onto `OadrCapacityState.export_limit_kw` in
`tasks/sim_tick/helpers.rs:74-75`. This design mirrors that shape but
targets `PvInverter.export_limit_kw` directly instead of routing through
`OadrCapacityState` (which is VTN-signal-owned, not operator-owned — see
Decisions).

## Goals / Non-Goals

**Goals:**
- A `pv_export_limit_kw` sim-inject field that, once set, is threaded into
  `PvInverter.export_limit_kw` every tick and actually reduces PV output
  in the simulator.
- Setting/clearing it triggers an out-of-cycle replan, consistent with
  `grid_export_limit_kw`.
- A UI control (persistent-override style, not decaying) so an operator
  can set/clear the ceiling from the VEN UI Controller tab.
- Removal of the dead dispatcher-side PV clamp.

**Non-Goals:**
- No change to `AssetCapability`/`is_fixed()` for PV — capability keeps
  reporting `max_export_kw == max_import_kw`. This is deliberate: the
  planner still cannot request an arbitrary PV setpoint (physics ignores
  it), only a ceiling can be imposed externally, so advertising a genuine
  range would be a lie about what the planner can actually achieve.
- No MILP solver changes; PV forecast input to the planner is untouched.
- No change to how the VTN's `EXPORT_CAPACITY_LIMIT` signal is handled —
  it already reaches only the dead dispatcher clamp being removed here.
  Making that VTN signal actually curtail PV (by routing it into the same
  `pv_export_limit_kw` mechanism this change introduces) is a natural
  follow-up but is explicitly out of scope here to keep this change to a
  single, operator-facing control path.
- No history changes (`TickSample.power_kw` stays post-curtailment only).

## Decisions

**1. New sim-inject field, not reuse of `OadrCapacityState.export_limit_kw`.**
`OadrCapacityState.export_limit_kw` is VTN-signal-owned (populated from
parsed OpenADR events, `controller/openadr_interface.rs:267`) and is a
*grid-level* (site-wide) limit, not PV-specific. Overloading it for an
operator-set PV-only ceiling would conflate two different authorities
(VTN vs. local operator) over two different scopes (site vs. asset).
`grid_export_limit_kw` in `SimInjectState` already shows the established
way to let a local operator override a capacity-like value for
testing/demo purposes without touching the VTN-owned state — the new
`pv_export_limit_kw` field follows that same separation.

**2. Threaded directly onto `PvInverter.export_limit_kw`, not through the
dispatcher's setpoint map.** The dispatcher clamp already proved this path
doesn't work (PV physics ignores setpoints). Since the target field lives
on the asset config itself and physics already reads it directly, the
simplest correct wiring is: sim-inject value → `SimState::tick` argument →
assigned onto `AssetConfig::Pv(pv).export_limit_kw` each tick, in the same
place `weather_power_kw` and `pv_irradiance_override` are already
threaded (`simulator/mod.rs:252-262`). This keeps the "who sets
`PvInverter` fields each tick" logic in one place.

**3. Persistent-override UI pattern (heater temp-limit style), not
decaying Behaviour-B (irradiance-slider style).** `pv_irradiance` decays
back toward the sin-model each tick via `pv_alpha` — appropriate for a
transient "simulate a cloud passing" demo action. A curtailment ceiling is
a standing operational constraint (e.g. "the grid connection point can't
take more than 5kW export right now") that should stay in effect until
explicitly cleared, matching `heater_temp_min_c`/`heater_temp_max_c`'s
existing persistent-override handling in `AssetRightSection.tsx`.

**4. Remove the dead dispatcher clamp rather than leave it in place.**
Once the real mechanism exists, the `dispatcher.rs` code computing an
unused PV setpoint clamp is pure dead weight that misleads readers into
thinking curtailment happens there. Deleting it is a same-change cleanup,
not a separate refactor, because it's the exact code this proposal
supersedes.

**5. Replan trigger: include `pv_export_limit_kw` in the sim-inject
event-trigger list (`routes/sim.rs`'s replan-check condition, alongside
`grid_export_limit_kw`).** A changed export ceiling changes what the plan
can assume PV will deliver in upcoming slots; waiting for the next
`Periodic` cycle (up to ~5 minutes, per `replan_interval_s=300`) would
leave the plan silently over-committing PV export in the meantime. This
mirrors existing precedent exactly, so it's a low-risk inclusion.

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
- **[Risk]** Removing the dispatcher clamp could look like a regression if
  read in isolation (a PR reviewer unfamiliar with the "this code never
  worked" finding might assume capability is being removed). →
  **Mitigation**: proposal.md and the commit message explicitly state the
  clamp was dead code (setpoint never consumed by PV physics), verified by
  `assets/pv.rs`'s own doc comment.
- **[Trade-off]** The VTN's `EXPORT_CAPACITY_LIMIT` signal still won't
  curtail PV after this change (see Non-Goals #3) — an operator/VTN
  reading the UI might expect otherwise. Accepted for this change's scope;
  flagged as a natural follow-up in the proposal.

## Migration Plan

No data migration. `SimInjectState` gains one new `Option<f64>` field,
defaulting to `None` (backward compatible — existing `/data/sim_state.json`
files deserialize fine via serde default). No env vars, no Docker changes.
Deploy via the existing `docker compose build && up -d` cycle. Rollback is
a plain revert (no persisted-state format change to undo).

## Open Questions

None outstanding — scope, sign convention, and UI pattern were confirmed
during the planning conversation preceding this proposal.
