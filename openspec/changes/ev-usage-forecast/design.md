# Design

## Context

See `proposal.md` - Why. This builds directly on `ev-usage-simulation` (shipped, live on 12 fleet
VENs) — do not re-derive that work, extend it. Relevant existing mechanisms:

- **The physics/prediction primitives already exist and are reused verbatim**: `daily_trip`,
  `active_trip_at`, `most_recently_ended_trip` (`VEN/src/assets/ev_schedule.rs`), and
  `EvCharger::is_away_at`/`ended_trip_at`/`apply_return_drop`/`apply_usage_sim_tick`
  (`VEN/src/assets/ev.rs` + `ev_schedule.rs`). `usage_forecast` uses these exactly as `usage_sim`
  does for the physical simulation — no duplication.
- **The connection point already exists and is adequate**: `MilpParticipant::build_milp_context`
  (implemented by `EvCharger` in `VEN/src/assets/ev.rs`, delegating to
  `EvMilpContext::from_state` in `VEN/src/assets/ev_milp.rs`) already receives `n: usize` (slot
  count) and `cum_s: &[i64]` (cumulative per-slot second-offsets from `now`), alongside the
  existing `ev_session: Option<&EvSession>`. That is already enough to compute every future plan
  slot's real timestamp and sample the EV's own prediction functions per slot — entirely inside
  code the EV asset already owns. No new cross-module plumbing is needed, and no hexagonal-layering
  rule is at risk: `controller/milp_planner` never imports `assets::` directly today and does not
  need to for this change either — the asset still just hands the controller an already-built
  `Box<dyn AssetMilpContext>`.
- **The actual `AssetMilpContext` trait shape** (`VEN/src/controller/asset_milp_port.rs:109`),
  confirmed during this design pass (corrects an earlier, less precise assumption): four phases —
  `milp_params(&self, n, now) -> AssetMilpParams` (scalar/array extraction), `declare_vars_into_pool`
  (LP variable declaration), `constraints(&self, pool, n, dt_h) -> Vec<good_lp::Constraint>` (this
  asset's own LP rows, built from its own typed variable handles), and `objective`. Each asset kind
  already implements all four independently (enum-dispatched via `AssetMilpParams::Ev(..)`,
  `::Heater(..)`, etc.) — there is no single generic per-slot loop shared across asset kinds today;
  each kind's own `constraints()` already builds its own per-slot rows using `n`/`dt_h`, which it
  already receives.
- **The one-shot `EvSession`-driven mask stays exactly as it is** (`controller/milp_planner/tests/
  basic.rs`'s `ev_mask_plugged_with_session_deadline`, `ev_mask_unplugged_all_false`,
  `ev_mode_must_run_for_firm_deadline_session`, `ev_mode_may_run_for_soft_deadline_session`) — this
  change adds a new, additional code path active only when `usage_forecast` is configured; it does
  not alter how a real `EvSession` is handled otherwise.
- **The MAX_COST insufficient-budget warning** is the precedent for Decision 6 below — the existing
  "deliver best-effort, surface a warning, never hard-reject" shape for an unreachable target.

## Goals / Non-Goals

**Goals:**
- Give the planner the EV's true predicted future availability, for as many trips as the horizon
  contains, with no new MILP decision variables.
- Let `engage_charge_planning` target the next predicted departure without occupying the single
  `EvSession` slot, so it can never conflict with a real user/VTN request for that slot.
- Keep `usage_sim` completely unchanged — `usage_forecast` is a new, alternative path, not a
  replacement.

**Non-Goals:**
- A target/urgency computed for every visible future trip in one horizon (only the next one, same
  as `usage_sim`'s existing `engage_charge_planning` semantics) — deferred; requires generalizing
  the scalar `t_ev_dead_step`/`e_ev_core_kwh` fields into a list, a materially larger change.
- A real, learned/heuristic-based forecast for non-simulated deployments. `usage_forecast`'s data
  is deterministic ground-truth disclosure from the same `daily_trip()` function that drives the
  physics (reading ahead in a pure, seeded function, not predicting with error) — there is no
  forecast-accuracy problem to solve here. That problem is real but distinct and out of scope.
- Any change to `usage_sim`, `EvSessionOrigin`, or `usage_sim_plan_ahead.rs`.
- Giving the EV MILP a genuine per-slot SoC trajectory (and with it, the ability to actively plan
  the post-return recharge). The existing EV model is total-energy-based with no SoC variable — see
  Decision 3/3a. Out of scope; this change delivers truthful availability plus a correct projected
  SoC curve, not post-return recharge planning.

## Decisions

**1. No new MILP decision variables.** The existing per-slot `p_ev_kw[t]` variable (however it is
actually named in `ev_milp.rs`/`pool`) is unchanged. What's new is per-slot bound/coefficient data
constraining that same variable — the same shape PV's and base-load's own per-slot forecast arrays
already use. Confirmed with the user as the guiding principle for the whole change.

**2. Availability needs no new primitive — `a_ev` already is the per-slot mask.** Verified while
implementing (this corrects an earlier assumption in this document): `EvMilpContext.a_ev: Vec<bool>`
(`controller/milp_planner/asset_port.rs:86`) is already a per-slot availability array, and it is
already fully wired into the LP — `ev_milp.rs:98` derives each slot's charge ceiling from it
(`let ev_ub = if self.a_ev[t] { self.p_max_kw } else { 0.0 }`) and `ev_milp.rs:41` uses it for
`z_ev_on`'s upper bound. It is simply always populated *uniformly* today: `vec![false; n]`
(unplugged), `vec![true; n]` (plugged, no session), or the one-sided `deadline_mask`.
   So `usage_forecast`'s availability contribution is: for each slot `t` in `0..n`, compute that
   slot's real timestamp (`now + Duration::seconds(cum_s[t])`), evaluate the EV's own
   `is_away_at`/`active_trip_at` at it, and AND the result into `a_ev[t]` alongside whatever mask
   the existing session/plugged logic already produced. No new field, no new trait method, no new
   LP wiring — just populating an existing, already-consumed array truthfully instead of uniformly.
   This is a pure function of `(usage_forecast config, seed_tag, slot timestamp)` — recomputed
   fresh each planning cycle, no caching or staleness, exactly like PV/base-load forecasts already.

**3. The SoC drop belongs in the post-solve trajectory, not in a constraint — there is no
SoC-balance constraint to add it to.** Verified while implementing (this replaces this document's
earlier, incorrect claim that a term would be added to an EV SoC-balance constraint):
   - The EV MILP has **no per-slot SoC variable and no SoC-balance constraint**. `EvMilpVars` is
     `{p_ev, z_ev_on, z_ev_core, e_ev_extra, delta_ev, delta_ev_ramp}`; `EvMilpContext::constraints`
     (`ev_milp.rs:92-148`) works entirely on *total energy* (`ev_energy == e_core_kwh + e_ev_extra`).
     SoC enters only as the scalar `soc_init` plus the energy requirements derived from it.
   - The plan's EV SoC curve is produced **after** the solve, by
     `ev_soc_trajectory(&sol.p_ev_kw, soc_init, battery_kwh, &inputs.dt_h)` at
     `controller/milp_planner/results.rs:332` — a pure post-processing integrator.
   So the drop is applied there: `ev_soc_trajectory` gains an exogenous per-slot delta input
   (sparse — zero except at the first slot whose start is at-or-after a predicted trip's
   `return_at`, where it is `−soc_drop_pct/100 × battery_kwh`, floored so the resulting projected
   SoC never falls below `min_soc_after_drop_pct`). This fully satisfies the spec requirement
   ("the plan's projected state-of-charge at that boundary reflects the configured drop") — it is
   the same observable behavior, at the only place the projected SoC actually exists.
   - **R-73 consolidation, done as part of this work** (per this project's `refactoring` rule —
     relevant Small/Trivial debt in the touched area gets fixed before adding behaviour there):
     `ev_soc_trajectory` (`asset_port.rs:274`) and `EvCharger::soc_trajectory` (`ev.rs:229`, carrying
     an explicit `#[allow(dead_code)]` + R-73 comment) are a known duplicate pair. Rather than
     threading the new delta through a duplicate (or adding a third copy), consolidate to one
     function first, then extend that one. Remove R-73 from `docs/reference/TECHNICAL_DEBTS.md` if
     this closes it.

**3a. Consequence of the above, stated explicitly so it is not mistaken for a gap:** because the LP
carries no SoC trajectory, the plan **cannot actively pre-plan the post-return recharge**. What
`usage_forecast` delivers is (a) truthful per-slot availability, so the plan never schedules
charging into a window the car is predicted to be away for, and never treats a predicted-away slot
as usable capacity, and (b) a projected SoC curve that reflects the drop. It does *not* make the
solver reason "after the return I will be at 25%, so I should plan to top up" — that would require
giving the EV MILP a genuine per-slot SoC trajectory, a materially larger rework of the existing
total-energy EV model, explicitly out of scope here (see Non-Goals).

**4. New profile class, not a mode flag on `usage_sim`.** `EvConfig` gains `usage_forecast:
Option<EvUsageForecastConfig>` alongside the existing `usage_sim: Option<EvUsageSimConfig>`,
validated as mutually exclusive. `EvUsageForecastConfig`/`EvUsageForecastDayConfig` mirror
`EvUsageSimConfig`/`EvUsageDayConfig` field-for-field (`engage_charge_planning`, `weekday`,
`weekend`, `min_soc_after_drop_pct`, and each day's `leave_time`/`leave_jitter_min`/`return_time`/
`return_jitter_min`/`leave_probability`/`soc_drop_pct_mean`/`soc_drop_pct_stddev`). Whether these
are genuinely separate types or the same underlying params type wrapped by a small mode enum
(`EvUsageMode::Simulated(EvUsageSimParams) | Forecast(EvUsageSimParams)`, sharing one params shape
since the fields are identical) is another implementation-time call — the latter avoids a literal
field-for-field duplicate struct definition and is likely preferable given the fields are
identical; noted here rather than decided, since it only affects internal typing, not behavior.

**5. `engage_charge_planning` reused verbatim, mechanism differs by class.** Under `usage_sim`, it
auto-writes an `EvSession` (unchanged). Under `usage_forecast`, it populates the equivalent
deadline/target fields directly inside `EvMilpContext::from_state`, from the next predicted trip's
`leave_at` and the EV's configured target SoC — no `EvSession`/`AppState` write at all for this
path, so it never competes for the single session slot. When off, the availability data (Decision
2) is still always asserted; only the target/deadline is withheld.

**6. Precedence: real session's target always wins; availability is never negotiable.** A real
`ev_session: Option<&EvSession>` — unchanged, still passed into `EvMilpContext::from_state` exactly
as today — always overrides whatever `usage_forecast`/`engage_charge_planning` would have derived
for the target/deadline, mirroring the "real always wins" precedent already established for
`usage_sim`'s `EvSessionOrigin`. This is enforced one level lower than that precedent (inside the
context-builder function itself, since there is no side-channel session object to arbitrate here —
both inputs already meet inside the same function call). The availability bounds from Decision 2
are asserted unconditionally regardless of any session — a real session can set a goal, never
override physical fact.

**7. Unreachable deadline: clamp the core energy, warn with the existing plan-warning kind.**
Revised against the code during implementation: the EV core-energy constraint is a hard equality
(`ev_energy == e_core_kwh + e_ev_extra`, `assets/ev_milp.rs`) with **no slack variable** — masking
the slots before a deadline therefore makes the whole site solve infeasible, not merely suboptimal
(pinned by `tests/solver.rs::solve_ev_must_run_core_energy_beyond_what_the_available_slots_can_
deliver`). So the shortfall is resolved where the fact is known, in the asset:
`EvMilpContext::clamp_core_to_reachable_energy` (`assets/ev_usage_forecast.rs`) clamps the core to
what the unmasked pre-deadline slots can physically deliver and records `core_unmet_warning`, which
travels `EvScalars` -> `MilpInputs` -> `ev_diagnostics::ev_warnings` and surfaces as an existing
`WarningKind::EvCoreEnergyUnmet` plan warning (same stable-text/dedup contract as the MAX_COST
budget warning, so no new warning mechanism and no request-time rejection). The clamp is not
forecast-only in effect: a *real* session's target can be stranded by the same mask, and is clamped
identically.

## Risks / Trade-offs

- **[MILP solver-correctness code]** → This changes how `a_ev` is populated, the same rigor class as
  existing `e_ev_core_kwh`/`t_ev_dead_step` logic. Every existing pinned test in
  `controller/milp_planner/tests/basic.rs` and `tests/solver.rs` must stay green unchanged before any
  new test is added, proving the new path is additive, not a reinterpretation of existing behavior.
  `a_ev` is populated uniformly today in every existing branch, so an `is_away_at`-derived AND must
  be a no-op whenever `usage_forecast` is absent — that invariant is what the baseline tests protect.
- **[Consolidating R-73 while adding behaviour (Decision 3)]** → Touching the duplicate trajectory
  pair is deliberate (project `refactoring` rule) but does widen the diff beyond `usage_forecast`
  itself. Mitigation: consolidate first as its own step with the existing
  `soc_trajectory_charges_monotonically`/`soc_trajectory_clamps_at_one` tests (`ev.rs`) kept green
  unchanged, and only then extend the surviving function with the exogenous-delta input.
- **[A real session's deadline can now land inside a predicted-unavailable slot]** → A new
  infeasibility shape that doesn't exist today (today's one-sided mask can never strand a deadline
  mid-gap). Mitigated by Decision 7's existing-pattern reuse (best-effort + warning), but this
  specific shape needs its own dedicated test — it is not automatically covered by the existing
  MAX_COST test, which exercises a different constraint.
- **[Two near-identical config types (`usage_sim` vs `usage_forecast`)]** → Deliberate, not
  accidental duplication: they diverge in how the same schedule reaches the planner, which is
  exactly the axis this change is about. Mitigated by sharing the underlying params type per
  Decision 4 rather than truly duplicating every field definition.

## Migration Plan

Purely additive: new optional profile section, mutually exclusive with `usage_sim` by validation,
zero effect on any profile that declares neither or only `usage_sim` (all 12 fleet VENs currently
use `usage_sim` and are unaffected). No data migration — same in-memory-only state model as
`ev-usage-simulation`. Standard branch → test-first implementation → Node2 verification →
rebase/fast-forward merge → deploy, per project convention. To adopt on a fleet VEN, swap that EV's
`usage_sim:` block for an equivalent `usage_forecast:` block — no other profile changes needed.

## Open Questions

- ~~Trait method vs. shared helper~~ — **resolved during implementation**: neither is needed.
  Availability reuses the existing `a_ev` array (Decision 2) and the SoC drop lands in the
  post-solve trajectory (Decision 3), so no new `AssetMilpContext` trait method and no new shared
  MILP primitive are introduced by this change at all. The "generic, not EV-bespoke" goal is met by
  *not adding* a mechanism: both halves reuse existing, already-generic machinery.
- ~~Separate types vs. a shared params type with a mode enum (Decision 4)~~ — **resolved during
  implementation**: shared type plus a mode tag, no new duplicate structs and no renames.
  `schema.rs` gains `usage_forecast: Option<EvUsageSimConfig>` reusing the *same* config type as
  `usage_sim` (the fields are identical, so a parallel struct would be pure duplication), validated
  mutually exclusive. `asset_params.rs` adds `enum EvUsageMode { Simulated, Forecast }` and one
  `mode` field on the existing `EvUsageSimParams`; `EvCharger` keeps its single `usage_sim` field.
  This makes "both modes at once" structurally impossible below the YAML layer rather than a runtime
  invariant, and the existing type/field names stay accurate — both modes *are* usage simulation;
  they differ only in whether the schedule is disclosed to the planner.
