# Tasks

Branch: `051-opportunistic-objective`, worktree `.claude/worktrees/051-opportunistic-objective`,
based on `main` after W1–W6. R-94 (heater latch pins) is still open on another session's branch;
if it lands first, rebase and re-record the goldens of group 0 before any production edit of
this branch. Every group is test-first: write the test, run it to see it fail, then implement.

"VEN tests green" means `cargo check --tests -j 2` in WSL under `scripts/wsl_lock.sh` after a
free-memory check, then `DOCKER_HOST=Node2 bash run_all_tests.sh --rust` with the Node2 lock held
and the host checked out on the branch (`git log -1` verified). "BDD green" means
`python scripts/audit_step_definitions.py` locally, then `--e2e` on Node2 the same way.

## 0. Invariance net (before any production edit)

- [x] 0.1 Tick golden (`tasks/sim_tick/tests/setpoints_golden.rs`, floats at 1e-9: the arbiter's
      sums over `HashMap`s differ in the last bit between processes): a test that replays a fixed set of planned-tick scenarios through
      `build_tick_setpoints` and records every asset setpoint and every `ArbiterOutcome` field,
      with an `UPDATE_TICK_GOLDEN=1` writer. Scenarios: plan slot present / absent / expired; EV
      overlay on / off; deviation correction on / off; an import limit with each switching lever;
      a DISPATCH_SETPOINT window; comms loss; an alert window; heater `heater_setpoint_c` inject.
      Record on unmodified code; it must pass unchanged after every later group (D0).
- [x] 0.2 Frozen copy of today's `rank_levers` in the test module, and a randomized test (fixed
      seed, 10 000 candidate sets incl. equal costs and an incumbent) that the `Cost` basis
      returns the same order.
- [x] 0.3 Gate net: a frozen copy of today's `evaluate_acceptance_gate` compared on 10 000 random
      pairs of planning plans (triggers, solved/failed, age, expiry, decay, switch surcharge)
      (`services/planning/gate_frozen_tests.rs`).

## 1. Declarations (no behaviour change)

- [x] 1.1 `PlannerObjective::Opportunistic` (wire `opportunistic`) with `dispatches_from_plan()`
      declared beside the variants; extend `planner_objective_ts_spelling_is_the_wire_spelling`;
      regenerate TS; add `OBJECTIVE_LABELS.opportunistic` and the Planner page option.
      `build_phase1_weights` returns `MinCost` weights for it with a comment: the cycle guard
      (4.1) never sends this objective to the MILP.
- [x] 1.2 `SolveStatus::Simulated` (`solved()` true) and `WarningKind::OpportunisticNoForecast`;
      regenerate TS; VEN UI shows both (Plan history status, warnings list).
- [x] 1.3 `UserRequestMode::non_planning_dispatch()` → `NonPlanningDispatch::{StartNow,
      SurplusOnly}` with the mapping of D4, and a test over every variant.
- [x] 1.4 One rank constant per asset kind, returned by both the kind's
      `AssetMilpContext::surplus_claim_rank` (`*_declared.rs`) and a new
      `Asset::surplus_claim_rank() -> Option<u8>` (PV/base load `None`; their context keeps
      `u8::MAX`), carried on `AssetSnapshot` via `simulator/snapshot.rs::to_sim_snapshot`;
      `plans_golden.rs` unchanged.
      Pin, per asset kind, that the snapshot fields the generic levers read already say "nothing
      to absorb / discharge" when they must (D2): battery at full / at `min_soc_frac`; EV unplugged
      or at target; heater at `temp_max_c` and while latched; shiftable load finished or past its
      window; EV below its minimum rate takes nothing. Fix the asset, not the lever, where one
      does not.
- [x] 1.5 Delete `RequestResolvable::surplus_charge_kw` and its tests (test-only; D2 makes the
      snapshot fields the one answer).

## 2. The opportunistic pass (controller, unit level)

- [x] 2.1 (dropped) A ranking basis on `LeverPolicy`: not needed, the pass does not use the
      closed-loop lever machinery (D2, rejected alternative).
- [x] 2.2 `arbiter::opportunistic_pass(tick, base, excluded_ids)`: open-loop allocation in
      declared rank order (D2), result in `ArbiterOutcome::opportunistic` only. Tests
      (`controller/arbiter/tests/opportunistic_tests.rs`): scarce surplus in order, ample surplus
      fills all and exports the rest, a full asset passes on, a stage that would import is not
      taken, night base load covered, never beyond the import, never charges from the grid,
      empty battery leaves the import, balanced site moves nothing, excluded asset keeps its
      base, no deviation state written, same balance gives the same commands every tick.
- [x] 2.3 `ArbiterTick.active_requests`; start-now base setpoint = `cap_max_import_kw` for every
      asset kind (D4; the targets that make it fall to zero are injected in 3.2); a
      start-now asset is not also an absorb candidate; surplus-only requests make their asset an
      absorb candidate regardless of the EV toggle.
      Tests: BY_DEADLINE EV starts now and stops at target, OPPORTUNISTIC EV idle without
      surplus, ASAP shiftable starts at earliest start, BY_DEADLINE heater drives toward target,
      MAX_COST is surplus-only, start-now EV bounded by the limit pass.
- [x] 2.4 Opportunistic decisions in the decision trace and `/arbiter-diagnostics` (pass, direction,
      leading lever); extend the diagnostics route test and the VEN UI arbiter diagnostics panel.

## 3. Live dispatch

- [x] 3.1 `dispatcher::build_setpoints(.., follow_allocations)`; every planning call passes `true`,
      only the opportunistic arm passes `false`; the planning no-plan base is untouched. Test:
      forecast allocations ignored when `false`; existing tests unchanged; tick golden holds.
- [x] 3.2 `build_tick_setpoints` gains one arm, taken only when the adopted plan's objective has
      `!dispatches_from_plan()`, that calls `opportunistic_step`, independent of
      `deviation_arbiter_enabled`. `reconcile` and the no-plan path stay as they are. Inside that
      arm only, collect `active_requests` in `tasks/sim_tick` from
      `current_ev_session(asset_id, now)` per EV, `heater_targets()` and active
      `shiftable_loads()`. New per-asset `TickInputs`/`TickOverrides` maps
      `ev_soc_target_by_asset` and `heater_temp_max_by_asset` (same shape as
      `ev_departure_by_asset`), filled only in the opportunistic arm: each EV's session target,
      and `min(target_temp_c, temp_max_c)` of each heater's start-now request. A scalar sim
      inject, when set, still wins (`tasks/sim_tick/tick.rs`, the EV and heater
      `apply_tick_overrides`). Tests: start-now heater cuts out
      at the request target, not at `temp_max_c`; it stays off until `target − thermostat_delta_c`
      and then reheats (no switching every tick over 600 simulated ticks); planned objectives
      unchanged; opportunistic plan with a 3 kW battery allocation commands 0 kW on a balanced
      site; toggle off → EV takes no surplus without a request; a planning tick reads no request
      state. Tick golden holds.
- [x] 3.4 Switch-back test: run the opportunistic arm for 3 600 ticks, switch to a `min_cost`
      plan; assert the residual tracker, `arbiter_active_lever` and `limit_active_lever` hold
      nothing the opportunistic pass wrote, and that the next tick's setpoints equal those of a
      `min_cost` VEN that never ran opportunistic, given the same asset states and last commands.
- [x] 3.3 `scripts/audit_file_sizes.py` and `scripts/audit_ven_architecture.py` pass; split
      `arbiter.rs` if it crosses 500 production lines (opportunistic pieces into
      `arbiter/opportunistic.rs`).

## 4. Planning cycle without a forecast yet

- [x] 4.1 `tasks/planning/cycle.rs`: under `!dispatches_from_plan()` skip the solve and adopt an
      empty-slot plan with `SolveStatus::Simulated` and `OpportunisticNoForecast` (removed again
      in 7.4).
- [x] 4.2 `evaluate_acceptance_gate`: one leading rule (D7) — adopt when `dispatches_from_plan()`
      differs between the two plans or the new plan is opportunistic. Tests: periodic
      opportunistic replan adopts despite higher cost; switching back adopts the first `min_cost`
      plan even when it failed; gate golden (0.3) unchanged for every planning pair.

## 5. Objective setting and source

- [x] 5.1 `ObjectiveSetting { objective, source, set_at }` replaces `ActiveObjective`'s value
      (`boot/mod.rs`, `app_ctx.rs`, `cycle_state.rs`); `services::objective::set_objective` is
      the one setter (writes, triggers `UserRequest`). `PUT /plan/objective` calls it with
      `VenUi`. New `GET /plan/objective` returns the setting plus `available_objectives`.
      Route tests for both.
- [x] 5.2 VEN UI: Planner page reads `GET /plan/objective`, shows source and set time; Dashboard
      objective chip shows the source. UI unit tests.

## 6. Fleet channel — VEN side

- [x] 6.1 Publish the `objective` leaf (retained, QoS 1) on connect and on every change, body =
      `GET /plan/objective`, with `try_publish` from a spawned task (D10). Unit tests: topic and
      body; with a full request queue the caller returns at once and logs the drop.
- [x] 6.2 Subscribe to `<topic_root>/command/objective` after every ConnAck; forward payloads over
      an mpsc channel to a `tasks/` consumer calling `set_objective(.., Fleet, ..)`. The consumer
      ignores a command whose `requested_at` it already applied (reconnect redelivery); an empty
      payload reverts to the profile objective only while the source is `Fleet`; malformed → warn
      and keep. Unit tests on the consumer's decision function: apply, redelivery ignored, clear
      while fleet-sourced, clear after a local change ignored, malformed.
- [x] 6.3 Publish the adopted plan on the `plan` leaf (retained, QoS 1) on every adoption, same
      non-blocking path as 6.1; serialization happens in the spawned task, not in the cycle.
- [x] 6.4 `report_timing`: `frequency: 0` = VEN's choice. Forecast obligations are re-armed as due
      on the first adoption after an objective change, otherwise at most once per 15 minutes,
      from a spawned task after the plan is in force; measurement obligations use
      `DEFAULT_REPORT_CADENCE_S`, logged. Tests in `openadr_interface.rs` and
      `services/obligation.rs`, including: other `frequency` values unchanged; throttle; immediate
      after an objective change.
- [x] 6.5 Move `energy_kwh` and a new `mean_power_kw` to a new `lab_core::units`;
      `VEN/src/entities/units.rs` re-exports; architecture audit still green. Unit tests in
      lab-core (round trip).

## 7. Opportunistic forecast

- [x] 7.1 Extract the per-slot skeleton (tariffs, caps, baseline, PV) shared by `fallback_plan`
      and `translate_to_plan` into one function; both call it. `plans_golden.rs` holds.
- [x] 7.2 `simulator/opportunistic_forecast.rs` (D6): per slot build snapshot, tick, base
      setpoints, call `opportunistic_step`, then `SimState::tick` with slot-mean PV and base load
      as measured overrides, `now` = slot start (the EV's own usage schedule decides plugging) and
      `dt_s` = slot length; request targets injected as in 3.2. Returns a per-slot, per-asset power and state trace. Tests: noon surplus
      charges battery, evening discharges to floor; scheduled 3 kW limit respected; forecast PV
      equals the input series (smoothing bypassed).
- [x] 7.3 `AssetMilpContext::solution_from_trace(net_kw, states)` per kind (each in its own
      `*_declared.rs`, unit-tested there), and `milp_planner::plan_from_forecast` building the
      `SolveOutput` from those and calling `translate_to_plan` with the same contexts
      (`Simulated`, friction 0, marginal values of D8). Test: exporting slot's
      marginal fields equal the export tariff; plan carries `objective: opportunistic`.
- [x] 7.4 `cycle.rs` guard calls the forecaster instead of 4.1's empty plan;
      `OpportunisticNoForecast` deleted. Guard test: one forecast step and one live tick given the
      same states, PV, base load, limits and requests command the same setpoints.
- [x] 7.5 Reporter test: under opportunistic, `USAGE_FORECAST` intervals equal the forecast's net
      power per slot.

## 8. Fleet side — BFF and VTN UI

- [ ] 8.0 `VTN/mosquitto/fleet-acl.conf`: `topic write openadr-lab/fleet/+/command/objective` for
      `openadr-bff` (that leaf only). Test first: the two new `scripts/test_fleet_acl.sh` cases
      (BFF's command reaches the VEN; a VEN cannot command another) fail on the old ACL, pass on
      the new. Deploy the broker config on Node1 and run the script before 8.2 goes live.
- [x] 8.1 `bff/src/fleet.rs::apply_message` ingests `objective`, `plan` and `command/objective`
      leaves (unit tests: each leaf, garbled body keeps the previous value, empty command = no
      command).
- [x] 8.2 Publish handle on the BFF MQTT client; `PUT /api/fleet/vens/:ven/objective` and `PUT
      /api/fleet/objective` (`{"objective": name | null}`; the published body adds
      `requested_at`), validated against each VEN's
      `available_objectives`, published retained QoS 1. A fleet-wide switch publishes one VEN at a
      time with a configurable spacing (default 10 s) so solves do not pile up. Route tests: 400
      on unknown value names VEN and value; null publishes an empty retained message; fleet-wide
      publishes one per known VEN, spaced.
- [x] 8.3 `GET /api/fleet/vens/:ven/plan` from the ingested `plan` leaf.
      (8.2/8.3 note: the BFF has no route-test harness, its handlers take the whole `AppCtx`
      with a VTN client; the rules are pure functions tested in `bff/src/fleet_command.rs`
      (validation naming VEN and value, empty retained release, one VEN per spacing in paused
      time), the handlers stay thin, and 9.2 exercises them end to end.)
- [x] 8.4 `scripts/seed_vtn.py`: `USAGE_FORECAST` descriptor on the fleet-monitoring event and
      `PAYLOAD_CONTRACT["USAGE_FORECAST"] = {"units": "KWH"}`; `docs/reference/WIRE_PROFILE.md`
      row if missing (the lab-core table test checks it).
- [x] 8.5 `GET /api/fleet/forecast`: newest `USAGE_FORECAST` report per online VEN from the
      recorder table, converted with `lab_core::units::mean_power_kw` per its declared unit and
      interval, summed with `group_and_resample` / `sum_over_grid`, each slot with VEN count and
      objectives. Tests: kWh→kW conversion, offline VEN excluded, mixed objectives listed, report
      and MQTT objective joined on `venName`, a VEN with no objective leaf counted as `unknown`.
      Also the `before` series (D11): a change of a VEN's `objective` leaf keeps its newest forecast
      report id in `fleet_store.rs`; tests: change keeps it, second change replaces it, VEN with no
      report omitted, offline VEN excluded, kWh→kW on the `before` series.
- [ ] 8.6 VTN UI Fleet page: Objective column (value, source, pending), per-row and fleet
      selectors, Release per row and for all; expandable row with the VEN's plan chart.
      `FleetPowerChart` forecast overlay with objective legend and the faint dashed `before` series. UI unit tests; eslint clean.

## 9. Use cases, BDD, docs

- [ ] 9.1 BDD `tests/features/ven_opportunistic_objective.feature`: switch objective via `PUT
      /plan/objective`; surplus absorbed in order; battery covers import, never exports; BY_DEADLINE
      EV starts now; OPPORTUNISTIC EV idle without surplus; import limit bounds start-now; switch
      back to `min_cost` and the plan's allocations drive the assets again; a `min_cost` VEN's
      no-plan startup is unchanged (battery idle). Prefer `{x:g}` numeric steps; run `audit_step_definitions.py`.
- [ ] 9.2 BDD `tests/features/fleet_objective_control.feature` (test stack has `mosquitto`,
      `FLEET_MQTT_HOST`, `test-bff`): switch one VEN and the fleet through the BFF; VEN reports
      source `fleet`; restart restores it; clearing returns to `profile`; forecast report arrives
      after adoption and the fleet forecast route sums it; after a second switch the route returns the
      first forecast as `before`.
- [ ] 9.3 Use cases: a new `UC-19: Running Without Planning (opportunistic objective)` in
      `docs/use-cases/HEMS-USE-CASE-OBSERVATION-MANUAL.md` (switch, surplus order, battery
      covering import, request modes, objective source); a new section `13. Comparing the Fleet
      With and Without Planning` in `docs/use-cases/SYSTEM-USE-CASES.md` (switch from the VTN UI,
      forecast overlay, release, restart behaviour), cross-linked from §11. Architecture: `VEN_ARCHITECTURE.md`
      arbiter section (opportunistic pass, D2–D5), planning section (forecast, D6–D7);
      `docs/plans/fleet-monitor/phase-0-foundation.md` §6.2 gains the `objective` and
      `command/objective` rows and marks `plan` as published.
- [ ] 9.4 `docs/reference/TECHNICAL_DEBTS.md` (Severity/Kind/Cost/Why-open each): lab-mqtt per-VEN
      ACL needs no debt row (8.0 extends it and tests it); per-kind
      `apply_ev_lever_opportunistic` (planning overlay) and `opportunistic_pass` answer "how
      much surplus does the EV take"
      under different rules, first step: an equivalence test that shows where they differ; the
      existing fleet telemetry publish awaits rumqttc's bounded queue and may stall the tick while
      the broker is down, needs-evidence (a test with an unreachable broker).
- [ ] 9.5 Remove this change's pointer from `docs/plans/strategic_roadmap.md`, delete this change
      directory once everything above is waved into the docs. Journal entry in
      `docs/history/project_journal.md`; key learnings if any.

## 10. Pre-merge

- [ ] 10.1 `cargo fmt --check`, `cargo clippy --all-targets --all-features -- -D warnings`
      (VEN, VTN/bff, lab-core), `cargo audit`, `python scripts/audit_file_sizes.py`,
      `python scripts/audit_duplication.py`, `python scripts/audit_ven_architecture.py`,
      `python scripts/audit_debt_gate.py`, VEN UI and VTN UI `npm test` + eslint.
- [ ] 10.2 Full `run_all_tests.sh` on Node2 (Rust, E2E, resilience) green.
- [ ] 10.3 After merge: deploy, switch the Node2 fleet to opportunistic from the VTN UI for one
      day (Mode A sanity check) and back; record in the journal.
