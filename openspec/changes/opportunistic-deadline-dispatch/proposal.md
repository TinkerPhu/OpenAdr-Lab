## Why

The opportunistic objective (051) serves a user's EV request without look-ahead, but it ignores what the
request says. ASAP and BY_DEADLINE both start now at full power, and an EV that is plugged in with no
request is only offered PV surplus. The day-long fleet switch of 2026-10-21 showed the consequence: all
10 EVs (profile class `usage_forecast`, so no stated request) charged 0 kWh in the 16 hours after the
switch, against 66-93 kWh over the three days before it. They sit plugged in at night, when there is no
PV. The Fleet plan card read this as 36 % less planned import, but that figure is mostly demand left
unserved, and the energy that was still imported cost more per kWh (0.111 to 0.172 EUR/kWh). A baseline
that does not serve the same demand cannot be compared with the planners.

## What Changes

- A plugged-in EV below its target with no request is served as an implicit ASAP request: start now at
  full power until the target. Its opportunistic-charging switch (default on) still turns this off.
- A BY_DEADLINE request is served surplus-first with a latest start: the asset takes PV surplus while it
  waits, and runs at full power from the instant `deadline - time-to-target - margin`, until the target.
  The time to target is declared by the asset itself.
- An ASAP request keeps starting now. The two modes now differ in opportunistic dispatch, as they already
  do in planning (the lateness price).
- The opportunistic forecast (the plan the VEN reports under this objective) plans the same behaviour,
  because it runs the same decision per slot.
- The diagnostics outcome of the opportunistic pass says which assets are waiting and until when, and the
  VEN UI shows it.
- **BREAKING (BDD):** the scenario "A BY_DEADLINE EV request charges at once, without look-ahead" no
  longer holds. It is replaced, not weakened: ASAP charges at once, BY_DEADLINE waits then charges.

## Capabilities

### New Capabilities
- `opportunistic-dispatch`: how the opportunistic objective serves requests without look-ahead: which
  request modes start now, which wait for a latest start, what an unrequested plugged-in EV does, and what
  the pass reports about it.

### Modified Capabilities
<!-- None: `openspec/specs/` holds no capability for the opportunistic objective; the 051 change was
     waved into docs/architecture/VEN_ARCHITECTURE.md and deleted. -->

## Impact

- VEN: `controller/arbiter/opportunistic.rs` (`request_regime`, `OpportunisticOutcome`),
  `entities/design_vocabulary.rs` (`NonPlanningDispatch`, `non_planning_dispatch`),
  `entities/asset.rs` and `controller/sim_snapshot.rs` (two typed asset declarations),
  `assets/ev*.rs` and `assets/shiftable_declared.rs` (the declarations), `simulator/opportunistic_forecast.rs`
  and `tasks/sim_tick/setpoints.rs` (they pass `now`).
- VEN UI: the arbiter diagnostics row for the opportunistic outcome; the EV card's switch label.
- BDD: `tests/features/ven_opportunistic_objective.feature`.
- Docs: `docs/architecture/VEN_ARCHITECTURE.md` (the opportunistic section), UC-19 in
  `docs/use-cases/HEMS-USE-CASE-OBSERVATION-MANUAL.md`, `docs/use-cases/SYSTEM-USE-CASES.md` section 14.
- Out of scope, filed as issues: a service-level KPI (SoC at departure, unserved energy) on the Fleet plan
  card; a latest start for the heater (needs its thermal model). The 10.3 fleet run of
  `opportunistic-objective` should be repeated after this change is deployed; its first run is the evidence
  above.
