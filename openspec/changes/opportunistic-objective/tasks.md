# Tasks

Branch: `051-opportunistic-objective` (next free feature id — confirm before creating), started
after `fix/r94-heater-latch-pin` has merged (it changes the heater thermostat that 1.4 pins). Groups
1–6 and 8–9 do not depend on W1 and ship first; group 7 waits for
`openspec/changes/asset-declared-planner-model` to land. Every group is test-first: write the
test, run it to see it fail, then implement.

"VEN tests green" means `cargo check --tests -j 2` in WSL under `scripts/wsl_lock.sh` after a
free-memory check, then `DOCKER_HOST=Node2 bash run_all_tests.sh --rust` with the Node2 lock held
and the host checked out on the branch (`git log -1` verified). "BDD green" means
`python scripts/audit_step_definitions.py` locally, then `--e2e` on Node2 the same way.

## 1. Declarations (no behaviour change)

- [ ] 1.1 `PlannerObjective::Opportunistic` (wire `opportunistic`) with `dispatches_from_plan()`
      declared beside the variants; extend `planner_objective_ts_spelling_is_the_wire_spelling`;
      regenerate TS; add `OBJECTIVE_LABELS.opportunistic` and the Planner page option.
      `build_phase1_weights` returns `MinCost` weights for it with a comment: the cycle guard
      (4.1) never sends this objective to the MILP.
- [ ] 1.2 `SolveStatus::Simulated` (`solved()` true) and `WarningKind::OpportunisticNoForecast`;
      regenerate TS; VEN UI shows both (Plan history status, warnings list).
- [ ] 1.3 `UserRequestMode::non_planning_dispatch()` → `NonPlanningDispatch::{StartNow,
      SurplusOnly}` with the mapping of D4, and a test over every variant.
- [ ] 1.4 `Asset::surplus_claim_rank() -> Option<u8>` (EV 0, heater 1, shiftable 2, battery 3,
      PV/base load `None`), carried on `AssetSnapshot` via `simulator/snapshot.rs::to_sim_snapshot`.
      Pin, per asset kind, that the snapshot fields the generic levers read already say "nothing
      to absorb / discharge" when they must (D2): battery at full / at `min_soc_frac`; EV unplugged
      or at target; heater at `temp_max_c` and while latched; shiftable load finished or past its
      window; EV below its minimum rate takes nothing. Fix the asset, not the lever, where one
      does not.
- [ ] 1.5 Delete `RequestResolvable::surplus_charge_kw` and its tests (test-only; D2 makes the
      snapshot fields the one answer).

## 2. The opportunistic pass (controller, unit level)

- [ ] 2.1 `LeverPolicy` gains a ranking basis (`Cost` | `DeclaredRank`); `rank_levers` sorts by it,
      hysteresis unchanged. `Lever` gains `asset_id: Option<&str>`. Tests: ranking by declared
      rank; existing ranking tests unchanged under `Cost`.
- [ ] 2.2 Generic `LEVER_ABSORB` / `LEVER_DISCHARGE` (capacity and apply from `AssetSnapshot`
      only, D2), dispatched by data in `apply_ranked_levers`; `apply_ev_lever_opportunistic`
      becomes the EV case of `LEVER_ABSORB` and `apply_surplus_ev_overlay` calls it.
      `OPPORTUNISTIC_POLICY` and `arbiter::opportunistic_step(tick, base) -> ArbiterOutcome` (pass
      + limit pass), memoryless from the base setpoints. Tests (one per spec scenario of "Surplus
      is absorbed…" and "Import is covered…"): scarce surplus in order, ample surplus fills all,
      full asset passes on, night base load covered, never exports, never charges from grid,
      empty battery leaves import, balanced site moves nothing, a lagging EV's excess goes to the
      battery this tick and comes back next tick (no wind-up over 20 ticks). Existing EV overlay
      and arbiter tests stay green unchanged.
- [ ] 2.3 `ArbiterTick.active_requests`; start-now base setpoint = `cap_max_import_kw` for every
      asset kind (D4; the targets that make it fall to zero are injected in 3.2); a
      start-now asset is not also an absorb candidate; surplus-only requests make their asset an
      absorb candidate regardless of the EV toggle.
      Tests: BY_DEADLINE EV starts now and stops at target, OPPORTUNISTIC EV idle without
      surplus, ASAP shiftable starts at earliest start, BY_DEADLINE heater drives toward target,
      MAX_COST is surplus-only, start-now EV bounded by the limit pass.
- [ ] 2.4 Opportunistic decisions in the decision trace and `/arbiter-diagnostics` (pass, direction,
      leading lever); extend the diagnostics route test and the VEN UI arbiter diagnostics panel.

## 3. Live dispatch

- [ ] 3.1 `dispatcher::build_setpoints(.., follow_allocations)`; the no-plan branch in
      `build_tick_setpoints` calls it with `false`. Test: forecast allocations ignored when
      `false`; existing build_setpoints tests unchanged with `true`.
- [ ] 3.2 `build_tick_setpoints` picks `opportunistic_step` when there is no plan slot or
      `!dispatches_from_plan()`, independent of `deviation_arbiter_enabled`; deletes the EV-only
      early return in `reconcile`. Collect `active_requests` in `tasks/sim_tick` from
      `current_ev_session`, the active heater target and active shiftable loads. Under
      opportunistic dispatch only, and only when no sim inject sets them: the EV session target goes
      to `ev_soc_target_override`, `min(target_temp_c, temp_max_c)` of a start-now heater request
      to `heater_temp_max_override` (`tasks/sim_tick/tick.rs`). Tests: start-now heater cuts out
      at the request target, not at `temp_max_c`; it stays off until `target − thermostat_delta_c`
      and then reheats (no switching every tick over 600 simulated ticks); planned objectives
      unchanged; no-plan
      startup covers import from the battery; opportunistic plan with a 3 kW battery allocation
      commands 0 kW on a balanced site; toggle off → EV takes no surplus without a request.
- [ ] 3.3 `scripts/audit_file_sizes.py` and `scripts/audit_ven_architecture.py` pass; split
      `arbiter.rs` if it crosses 500 production lines (opportunistic pieces into
      `arbiter/opportunistic.rs`).

## 4. Planning cycle without a forecast yet

- [ ] 4.1 `tasks/planning/cycle.rs`: under `!dispatches_from_plan()` skip the solve and adopt an
      empty-slot plan with `SolveStatus::Simulated` and `OpportunisticNoForecast` (removed again
      in 7.4).
- [ ] 4.2 `evaluate_acceptance_gate`: a non-planning objective's plan always adopts. Tests:
      periodic replan adopts despite higher cost; switching back to `min_cost` adopts the first
      solved plan.

## 5. Objective setting and source

- [ ] 5.1 `ObjectiveSetting { objective, source, set_at }` replaces `ActiveObjective`'s value
      (`boot/mod.rs`, `app_ctx.rs`, `cycle_state.rs`); `services::objective::set_objective` is
      the one setter (writes, triggers `UserRequest`). `PUT /plan/objective` calls it with
      `VenUi`. New `GET /plan/objective` returns the setting plus `available_objectives`.
      Route tests for both.
- [ ] 5.2 VEN UI: Planner page reads `GET /plan/objective`, shows source and set time; Dashboard
      objective chip shows the source. UI unit tests.

## 6. Fleet channel — VEN side

- [ ] 6.1 Publish the `objective` leaf (retained, QoS 1) on connect and on every change, body =
      `GET /plan/objective`. Unit test on the topic and body.
- [ ] 6.2 Subscribe to `<topic_root>/command/objective` after every ConnAck; forward payloads over
      an mpsc channel to a `tasks/` consumer calling `set_objective(.., Fleet, ..)`. The consumer
      ignores a command whose `requested_at` it already applied (reconnect redelivery); an empty
      payload reverts to the profile objective only while the source is `Fleet`; malformed → warn
      and keep. Unit tests on the consumer's decision function: apply, redelivery ignored, clear
      while fleet-sourced, clear after a local change ignored, malformed.
- [ ] 6.3 Publish the adopted plan on the `plan` leaf (retained, QoS 1) on every adoption.
- [ ] 6.4 `report_timing`: `frequency: 0` = VEN's choice. Forecast obligations are re-armed as due
      on every adoption; measurement obligations use `DEFAULT_REPORT_CADENCE_S`, logged. Tests in
      `openadr_interface.rs` and `services/obligation.rs`.
- [ ] 6.5 Move `energy_kwh` and a new `mean_power_kw` to a new `lab_core::units`;
      `VEN/src/entities/units.rs` re-exports; architecture audit still green. Unit tests in
      lab-core (round trip).

## 7. Opportunistic forecast (after W1)

- [ ] 7.1 Extract the per-slot skeleton (tariffs, caps, baseline, PV) shared by `fallback_plan`
      and `translate_to_plan` into one function; both call it. `plans_golden` (from W1) holds.
- [ ] 7.2 `simulator/opportunistic_forecast.rs` (D6): per slot build snapshot, tick, base
      setpoints, call `opportunistic_step`, then `SimState::tick` with slot-mean PV and base load
      as measured overrides, `now` = slot start (the EV's own usage schedule decides plugging) and
      `dt_s` = slot length; request targets injected as in 3.2. Returns a per-slot, per-asset power and state trace. Tests: noon surplus
      charges battery, evening discharges to floor; scheduled 3 kW limit respected; forecast PV
      equals the input series (smoothing bypassed).
- [ ] 7.3 `milp_planner::plan_from_forecast` builds a W1 `SolveOutput` from the trace and calls
      `translate_to_plan` (`Simulated`, friction 0, marginal values of D8). Test: exporting slot's
      marginal fields equal the export tariff; plan carries `objective: opportunistic`.
- [ ] 7.4 `cycle.rs` guard calls the forecaster instead of 4.1's empty plan;
      `OpportunisticNoForecast` deleted. Guard test: one forecast step and one live tick given the
      same states, PV, base load, limits and requests command the same setpoints.
- [ ] 7.5 Reporter test: under opportunistic, `USAGE_FORECAST` intervals equal the forecast's net
      power per slot.

## 8. Fleet side — BFF and VTN UI

- [ ] 8.1 `bff/src/fleet.rs::apply_message` ingests `objective`, `plan` and `command/objective`
      leaves (unit tests: each leaf, garbled body keeps the previous value, empty command = no
      command).
- [ ] 8.2 Publish handle on the BFF MQTT client; `PUT /api/fleet/vens/:ven/objective` and `PUT
      /api/fleet/objective` (`{"objective": name | null}`; the published body adds
      `requested_at`), validated against each VEN's
      `available_objectives`, published retained QoS 1. Route tests: 400 on unknown value names
      VEN and value; null publishes an empty retained message; fleet-wide publishes one per known
      VEN.
- [ ] 8.3 `GET /api/fleet/vens/:ven/plan` from the ingested `plan` leaf.
- [ ] 8.4 `scripts/seed_vtn.py`: `USAGE_FORECAST` descriptor on the fleet-monitoring event and
      `PAYLOAD_CONTRACT["USAGE_FORECAST"] = {"units": "KWH"}`; `docs/reference/WIRE_PROFILE.md`
      row if missing (the lab-core table test checks it).
- [ ] 8.5 `GET /api/fleet/forecast`: newest `USAGE_FORECAST` report per online VEN from the
      recorder table, converted with `lab_core::units::mean_power_kw` per its declared unit and
      interval, summed with `group_and_resample` / `sum_over_grid`, each slot with VEN count and
      objectives. Tests: kWh→kW conversion, offline VEN excluded, mixed objectives listed, report
      and MQTT objective joined on `venName`, a VEN with no objective leaf counted as `unknown`.
- [ ] 8.6 VTN UI Fleet page: Objective column (value, source, pending), per-row and fleet
      selectors, Release per row and for all; expandable row with the VEN's plan chart.
      `FleetPowerChart` forecast overlay with objective legend. UI unit tests; eslint clean.

## 9. Use cases, BDD, docs

- [ ] 9.1 BDD `tests/features/ven_opportunistic_objective.feature`: switch objective via `PUT
      /plan/objective`; surplus absorbed in order; battery covers import, never exports; BY_DEADLINE
      EV starts now; OPPORTUNISTIC EV idle without surplus; import limit bounds start-now; no-plan
      startup. Prefer `{x:g}` numeric steps; run `audit_step_definitions.py`.
- [ ] 9.2 BDD `tests/features/fleet_objective_control.feature` (test stack has `mosquitto`,
      `FLEET_MQTT_HOST`, `test-bff`): switch one VEN and the fleet through the BFF; VEN reports
      source `fleet`; restart restores it; clearing returns to `profile`; forecast report arrives
      after adoption and the fleet forecast route sums it.
- [ ] 9.3 Use cases: a new `UC-19: Running Without Planning (opportunistic objective)` in
      `docs/use-cases/HEMS-USE-CASE-OBSERVATION-MANUAL.md` (switch, surplus order, battery
      covering import, request modes, objective source); a new section `13. Comparing the Fleet
      With and Without Planning` in `docs/use-cases/SYSTEM-USE-CASES.md` (switch from the VTN UI,
      forecast overlay, release, restart behaviour), cross-linked from §11. Architecture: `VEN_ARCHITECTURE.md`
      arbiter section (opportunistic pass, D2–D5), planning section (forecast, D6–D7);
      `docs/plans/fleet-monitor/phase-0-foundation.md` §6.2 gains the `objective` and
      `command/objective` rows and marks `plan` as published.
- [ ] 9.4 `docs/reference/TECHNICAL_DEBTS.md` (Severity/Kind/Cost/Why-open each): lab-mqtt per-VEN
      ACL now also guards objective commands (extend the existing row or file one); per-kind
      `battery_lever`/`ev_lever` are instances of the generic lever shape (D2), first step: move
      the battery's capacity calculation onto `LEVER_DISCHARGE`/`LEVER_ABSORB`.
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
