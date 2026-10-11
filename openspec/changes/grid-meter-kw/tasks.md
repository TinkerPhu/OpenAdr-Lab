# Tasks

## 1. VEN meter, snapshot and sensor (test-first)

- [ ] 1.1 Update the unit tests to the kW field names and values (`simulator/tests.rs`, `recording_tests.rs`, `telemetry_port.rs`, `snapshot.rs`, `history_sampling.rs`, the mocks in `services/test_support/`); run them and see them fail
- [ ] 1.2 Rename `GridMeter` fields and write kW directly in `grid_meter.rs`; `EnergyCounter::integrate` takes kW (`energy.rs`, its one caller in `simulator/mod.rs`)
- [ ] 1.3 `GridSnapshot.net_power_w` → `net_power_kw`; delete `GridSnapshot::net_power_kw()` and the conversion in `simulator/recording.rs`; follow the compiler through the remaining readers and fixtures
- [ ] 1.4 `SensorSnapshot` / `SensorInput` `power_w` → `power_kw`, `SensorRaw` `import_w` / `export_w` → `import_kw` / `export_kw` (`entities/sensor.rs`, `simulator/snapshot.rs`, `routes/events.rs`, `state/mod.rs`)
- [ ] 1.5 `#[serde(default)]` on `GridMeter`; a unit test loading an old-format `sim_state.json` blob: it parses, asset state and `import_kwh` / `export_kwh` survive (design D3)
- [ ] 1.6 Delete `kw_from_w` / `w_from_kw` and their tests from `entities/units.rs`; update the helper list in `scripts/audit_ven_architecture.py` and in `.claude/CLAUDE.md`'s invariant line

## 2. VEN UI

- [ ] 2.1 Regenerate the API types (`UPDATE_UI_TYPES=1 cargo test ui_types`); follow `tsc` through `Dashboard.tsx` (print kW with `formatKwOrDash`), `Controller.tsx`, `dataBuilders.ts`, `components/controller/types.ts` (comment), `SimProfileChart.tsx`
- [ ] 2.2 Update vitest fixtures and expectations (11 test files, `__tests__/fixtures/sim.ts`); `npm test` and eslint clean

## 3. VTN BFF

- [ ] 3.1 `fleet.rs` (`net_power_kw`, reads `grid.net_power_kw`), `fleet_store.rs` (row field, SQL), `fleet_reactions.rs` (`power_before_kw`, `power_after_kw`, `delta_kw`, `avg(net_power_kw)`), `routes/fleet.rs` (`netPowerKw`, `deltaKw`, `sum_kw`); their unit tests first
- [ ] 3.2 Recorder migration in `fleet_store::init_schema`: rename and rescale both columns, guarded by the old column's existence, one transaction per table (design D4)

## 4. VTN UI

- [ ] 4.1 `api/types.ts` (`netPowerKw`, `powerBeforeKw`, `powerAfterKw`, `deltaKw`); delete `utils/power.ts::formatKw` and `toKw`, call `formatKwOrDash` at the call sites (`FleetPowerChart`, `FleetReactions`, `Fleet`)
- [ ] 4.2 Update the four vitest files; `npm test` and eslint clean
- [ ] 4.3 On the test stack: start the new BFF on a `lab_recorder` schema the old BFF created and filled; both tables read back in kW and a second start changes nothing

## 5. BDD and docs

- [ ] 5.1 Update `alerts_steps.py`, `limit_enforcement_steps.py`, `fleet_steps.py`, `ven_sensors_steps.py`, `ven_integration_steps.py`, `ven_simulator.feature`, `ven_integration.feature`
- [ ] 5.2 New scenario in `ven_simulator.feature`: `grid.net_power_kw` equals the sum of the assets' `power_kw` (fails by 1000x on a watt value); run `python scripts/audit_step_definitions.py`
- [ ] 5.3 Docs: `docs/architecture/VEN_ARCHITECTURE.md` (the unit-helper list, and the kW/kWh convention stated once), `docs/plans/fleet-monitor/phase-0-foundation.md` (three mentions), `docs/reference/KEY_LEARNINGS.md` (the `grid.net_power_w` mention plus the lesson), the Dashboard reading in `docs/use-cases/HEMS-USE-CASE-OBSERVATION-MANUAL.md` if it quotes watts; journal entry

## 6. Gates

- [ ] 6.1 Grep gate over VEN, BFF, both UIs and the steps: no `net_power_w`, `import_w`, `export_w`, `power_w`, `netPowerW`, `powerBeforeW`, `powerAfterW`, `deltaW`, and no W-to-kW `/ 1000` left; then make it permanent as audit rule 10's `_w` check (design D7)
- [ ] 6.2 fmt, clippy `-D warnings`, `cargo audit`, `audit_file_sizes`, `audit_ven_architecture`, `audit_duplication`, `audit_debt_gate`
- [ ] 6.3 Full suites on Node2 (`--e2e` including the fleet features, `--resilience`)
- [ ] 6.4 Deploy in the order of design D5: dump `lab_recorder`, BFF and VTN UI, VENs on Node1 then Node2, VEN UI; check the fleet view sums in kW and one VEN Dashboard
- [ ] 6.5 Delete this change and its register row (R-138) per the workflow rule
