# Tasks: VEN Controller Reform

**Input**: Design documents from `/specs/004-ven-controller-reform/`
**Prerequisites**: plan.md ✅, spec.md ✅, research.md ✅, data-model.md ✅, contracts/api-changes.md ✅

**Organization**: Tasks grouped by implementation phase, mapped to user stories (US1–US5).

## Format: `[ID] [P?] [Story?] Description`

- **[P]**: Can run in parallel (different files, no dependencies on incomplete tasks)
- **[Story]**: User story this task belongs to (US1–US5)

---

## Phase 1: BDD First (Constitution Gate — MUST complete before any Rust code changes)

**Purpose**: Update the BDD test suite before implementation. Suite must be red on new endpoints before Rust changes begin. This phase covers BDD work for all 5 user stories.

**⚠️ BDD-FIRST GATE**: Do not touch any Rust source file until this phase is complete and the suite runs red on the new endpoints.

- [x] T001 Scan all `tests/features/` .feature files for: `GET /trace`, `GET /rates`, `ev_force_kw`, `heater_force_kw`, `battery_force_kw`, `pv_force_export_limit_kw`, FSM state names (`Delaying`, `Ramping`, `Holding`, `RampingBack`). Record which files contain each pattern.
- [x] T002 Delete all BDD scenarios referencing `ev_force_kw`, `heater_force_kw`, `battery_force_kw`, or `pv_force_export_limit_kw` from `tests/features/ven_dispatcher.feature` and any other affected .feature file.
- [x] T003 Delete all BDD scenarios testing FSM states (`Delaying`, `Ramping`, `Holding`, `RampingBack`) and reactor arbitration from all .feature files.
- [x] T004 [P] Rewrite all `GET /trace` references in existing BDD scenarios to `GET /trace/events` or `GET /trace/history?asset=<id>` as appropriate in affected .feature files and `tests/steps/` step definitions.
- [x] T005 [P] Rename all `GET /rates` references to `GET /tariffs` in all .feature files and `tests/steps/` step definitions. (Covers US5 BDD work.)
- [x] T006 Update `POST /sim/override` BDD scenarios in affected .feature files: remove force-override key assertions (`ev_force_kw`, etc.), verify remaining keys (`ambient_temp_c`, `ev_desired_kw`) still work.
- [x] T007 Add new BDD scenarios to `tests/features/ven_entity_model.feature`: `GET /trace/events` returns a list of typed controller events after VEN operation. (US2)
- [x] T008 [P] Add new BDD scenarios for `GET /trace/history?asset=ev&limit=5` returning rows with `power_kw` and `soc_pct` fields. (US2)
- [x] T009 Add new BDD scenario for `GET /tariffs` returning the same tariff data structure that `GET /rates` previously returned. (US5)
- [x] T010 Run the full BDD suite on Pi4-Server (`docker compose -f tests/docker-compose.test.yml run --build --rm test-runner`) — confirm the new `GET /trace/events`, `GET /trace/history`, and `GET /tariffs` scenarios fail (red phase). All existing passing scenarios must still pass.

**Checkpoint**: BDD suite is red on new endpoints. Phase 1 complete — Rust implementation may begin.

---

## Phase 2: Foundational (Blocking Prerequisites for US1–US5)

**Purpose**: Rename `RateSnapshot` → `TariffSnapshot` (US5), introduce `ControllerTrace` types (US2), update `state.rs` (shared by US1–US4), reorganise `controller/mod.rs`. No behavioral changes; must compile cleanly at end of this phase.

**⚠️ CRITICAL**: All user story Rust work depends on these foundations.

- [x] T011 [US5] Rename `VEN/src/entities/rate_snapshot.rs` → `VEN/src/entities/tariff_snapshot.rs`. Inside the file: rename `RateSnapshot` → `TariffSnapshot`, `PlannedRates` → `PlannedTariffs`, `PastRates` → `PastTariffs`, `RateHeuristic` → `TariffHeuristic`.
- [x] T012 [P] [US5] Update `VEN/src/entities/mod.rs`: change `pub mod rate_snapshot;` → `pub mod tariff_snapshot;`.
- [x] T013 [US5] Update all Rust files importing `RateSnapshot`/`PlannedRates`/`PastRates`: `VEN/src/state.rs`, `VEN/src/controller/openadr_interface.rs`, `VEN/src/controller/monitor.rs`, `VEN/src/controller/planner.rs`, `VEN/src/main.rs`. Replace every occurrence with the new `TariffSnapshot`/`PlannedTariffs`/`PastTariffs` names.
- [x] T014 [US5] In `VEN/src/state.rs`: rename field `planned_rates: Vec<RateSnapshot>` → `planned_tariffs: Vec<TariffSnapshot>`; rename accessor methods `planned_rates()` → `planned_tariffs()`, `set_planned_rates()` → `set_planned_tariffs()`.
- [x] T015 [US5] In `VEN/src/main.rs`: rename `GET /rates` route handler `get_rates` → `get_tariffs`; update route registration `.route("/rates", ...)` → `.route("/tariffs", ...)`.
- [x] T016 [US2] Add `ControllerEvent` enum (all 7 variants: `OpenAdrArrived`, `OpenAdrExpired`, `RateChange`, `CapacityChange`, `PlanCycle`, `PacketTransition`, `RequestTransition`) to `VEN/src/controller/trace.rs`. Derive `Debug, Clone, Serialize, Deserialize`. Use `serde(tag = "type")` for tagged JSON.
- [x] T017 [P] [US2] Add `ControllerEventLog` struct to `VEN/src/controller/trace.rs` with `push`, `entries`, `len` methods and a `capacity: usize` (default 500).
- [x] T018 [US2] Add `ControllerTrace` struct to `VEN/src/controller/trace.rs` holding `event_log: ControllerEventLog` and `asset_history: HashMap<String, AssetHistoryBuffer>`. Add methods: `push_event`, `push_asset_row`, `events`, `asset_history`.
- [x] T019 [US2] Update `VEN/src/state.rs`: remove `use crate::reactor::trace::TraceEntry;`; add `use crate::controller::trace::ControllerTrace;`. Remove field `trace: Vec<TraceEntry>`; add `controller_trace: ControllerTrace` with `#[serde(skip)]`. Update `InnerState::new()`.
- [x] T020 [P] [US2] Add AppState accessor methods to `VEN/src/state.rs`: `controller_trace() -> ControllerTrace`, `set_controller_trace(ControllerTrace)`, `push_controller_event(ControllerEvent)`.
- [x] T021 [US2] Replace `update_sim(sim: SimSnapshot, trace: Vec<TraceEntry>)` with `update_sim(sim: SimSnapshot)` in `VEN/src/state.rs`. Update all callers.
- [x] T022 Update `VEN/src/controller/mod.rs`: add `pub mod reporter;` and `pub mod timeline;`; add group comments: `// ── VTN protocol adapter ──`, `// ── Control logic ──`, `// ── Observability ──` above the appropriate module declarations.
- [x] T023 [P] Create `VEN/src/controller/timeline.rs` stub: single `pub fn build_asset_timeline(_history: &HashMap<String, AssetHistoryBuffer>) -> serde_json::Value { serde_json::Value::Null }`.
- [x] T024 Run `cargo build` on Pi4-Server (or locally) and fix all compile errors introduced by T011–T023. The build must succeed cleanly before proceeding.

**Checkpoint**: VEN compiles cleanly with renamed types, new trace types, and updated state. Behaviorally identical to before — no running behavior has changed.

---

## Phase 3: User Story 1 — Single Authoritative Control Path (Priority: P1) 🎯

**Goal**: Delete the reactor module and rewrite the tick loop so the planner/dispatcher is the sole control authority. All UC-01–UC-12 BDD scenarios must pass.

**Independent Test**: Run `tests/features/use_cases.feature` and `tests/features/ven_uc_normal.feature` — all 12 UC scenarios pass.

- [x] T025 [P] [US1] Delete `VEN/src/reactor/` directory entirely (all 5 files: `mod.rs`, `arbitration.rs`, `fsm.rs`, `interval.rs`, `trace.rs`).
- [x] T026 [P] [US1] Rewrite `VEN/src/controller/dispatcher.rs`: replace `get_setpoints` and `update_packets` with new `pub fn build_setpoints(plan: &Plan, assets: &[AssetEntry], capacity: &OadrCapacityState, now: DateTime<Utc>) -> HashMap<String, f64>`. Scan FIRM and FLEXIBLE slots for the slot covering `now`; fill gaps with `asset.state.default_setpoint()`; enforce `ExportCapLimit` on `pv` key if active. Delete `DispatcherSetpoints` struct.
- [x] T027 [US1] Remove `mod reactor;` and `mod reporter;` from `VEN/src/main.rs`. Remove `reactor_state: Arc<Mutex<Reactor>>` initialization. Remove `use reactor::Reactor;`.
- [x] T028 [US1] Rewrite the tick loop in `VEN/src/main.rs` to the new sequence: (1) `dispatcher::build_setpoints(plan, assets, capacity, now)` → setpoints map; (2) `sim.tick(dt_s, setpoints, now, &overrides)`; (3) `state.update_sim(sim_snap)`; (4) timer check for measurement reports. Remove the reactor evaluate call and all force-override overlay lines (`ev_force_kw`, `heater_force_kw`, `battery_force_kw`, `pv_force_export_limit_kw`).
- [x] T029 [US1] Update the planning loop in `VEN/src/main.rs`: after `run_planner(...)` completes, push a `ControllerEvent::PlanCycle { ts, trigger_reason, firm_slots, flexible_slots }` via `state.push_controller_event(...)`.
- [x] T030 [US1] Run `cargo build` and fix all remaining compile errors from the reactor deletion and tick loop rewrite in `VEN/src/main.rs` and related files.
- [x] T031 [US1] Deploy updated VEN to Pi4-Server (`git push` → `git pull` → `docker compose up -d --build`). Run `docker compose -f tests/docker-compose.test.yml run --build --rm test-runner features/use_cases.feature features/ven_uc_normal.feature` and verify all UC-01–UC-12 scenarios pass.

**Checkpoint**: Single control path confirmed. UC-01–UC-12 all green. Reactor is gone.

---

## Phase 4: User Story 2 — Transparent Controller Observability (Priority: P2)

**Goal**: Wire `AssetHistoryBuffer` in the tick loop, emit `ControllerEvent` entries from key modules, and expose `GET /trace/events` + `GET /trace/history` endpoints.

**Independent Test**: After 10s of VEN operation, `GET /trace/events` returns a non-empty list including at least a `PlanCycle` entry; `GET /trace/history?asset=ev&limit=5` returns 5 rows each with `power_kw` and `soc_pct`.

- [ ] T032 [US2] Wire asset history writes into the tick loop in `VEN/src/main.rs`: after `sim.tick()`, call `state.push_asset_history_row(asset_id, now, values)` for each asset in the sim snapshot. Each row must include `power_kw`, all `state_values()` keys from the asset snapshot, `cost_rate_eur_h` (= `|power_kw|` × applicable tariff), and `co2_rate_g_h`. The grid asset row additionally includes `import_price_eur_kwh`, `export_price_eur_kwh`, `import_limit_kw`, `export_limit_kw`.
- [ ] T033 [P] [US2] Add `push_asset_history_row(asset_id: &str, ts: DateTime<Utc>, values: HashMap<String, f64>)` accessor method to `VEN/src/state.rs` (acquires write lock on `controller_trace`, delegates to `trace.push_asset_row`).
- [ ] T034 [P] [US2] Emit `ControllerEvent::OpenAdrArrived` and `ControllerEvent::OpenAdrExpired` from `VEN/src/controller/openadr_interface.rs` on event arrival/expiry detection. Return events to emit or accept a `push_fn` callback — choose whichever fits the existing function signature cleanly.
- [ ] T035 [P] [US2] Emit `ControllerEvent::RateChange` from `VEN/src/controller/openadr_interface.rs` when tariff snapshots are parsed. Emit `ControllerEvent::CapacityChange` when capacity state changes.
- [ ] T036 [US2] In `VEN/src/main.rs` poll-events task: call `state.push_controller_event(...)` for each `ControllerEvent` returned / signalled by `openadr_interface` functions (T034, T035).
- [ ] T037 [US2] Add `GET /trace/events` route handler `get_trace_events` in `VEN/src/main.rs`: reads `ctx.state.controller_trace().await.events()`, supports `?limit=N` query param (default 100), returns newest-first.
- [ ] T038 [P] [US2] Add `GET /trace/history` route handler `get_trace_history` in `VEN/src/main.rs`: reads `ctx.state.controller_trace().await.asset_history(&asset_id)`, converts to `Vec<AssetTimelinePoint>` via `to_timeline(None)`, truncates to `limit` (default 100). Returns 404 if asset not found or has no rows.
- [ ] T039 [US2] Update route registrations in `VEN/src/main.rs`: replace `.route("/trace", get(get_trace))` with `.route("/trace/events", get(get_trace_events))` and `.route("/trace/history", get(get_trace_history))`. Remove old `get_trace` handler function.
- [ ] T040 [US2] Deploy to Pi4-Server and run `GET /trace/events` + `GET /trace/history` BDD scenarios to verify green.

**Checkpoint**: Observability endpoints live. `GET /trace/events` and `GET /trace/history` both return data after VEN operation.

---

## Phase 5: User Story 3 — Correct Packet Energy Accounting (Priority: P3)

**Goal**: Move packet energy accounting from dispatcher into `monitor.record_tick`, so energy is attributed from measured (post-tick) sim values rather than commanded values.

**Independent Test**: `GET /ledger` energy totals after several sim ticks match cumulative energy from `GET /sim`. Packet status transitions (Scheduled→Active, Active→Completed) still fire correctly.

- [ ] T041 [US3] Expand `VEN/src/controller/monitor.rs`: replace `update_ledger` with `record_tick(trace: &mut ControllerTrace, ledger: &mut HashMap<String, AssetLedgerEntry>, packets: &mut Vec<EnergyPacket>, snapshot: &SimSnapshot, tariffs: &[TariffSnapshot], dt_s: f64, now: DateTime<Utc>) -> Option<PlanTrigger>`. This function: (a) writes asset history rows (if not already done in tick loop — choose one location); (b) attributes `|power_kw| * (dt_s / 3600.0)` kWh to the active packet per asset, handling Scheduled→Active transition and Completed/PartialCompleted check; (c) updates `AssetLedgerEntry` in ledger. Returns `Some(PlanTrigger::DeviceDeviation)` if any packet completed.
- [ ] T042 [US3] Emit `ControllerEvent::PacketTransition { ts, packet_id, asset_id, from_status, to_status }` from `monitor.rs` whenever a packet status changes (Scheduled→Active, Active→Completed, Active→PartialCompleted).
- [ ] T043 [US3] Update `VEN/src/main.rs` tick loop: replace the separate `dispatcher::update_packets(...)` call and `monitor::update_ledger(...)` call with a single `monitor::record_tick(...)` call. Forward the returned `Option<PlanTrigger>` to `trigger_tx`.
- [ ] T044 [US3] Emit `ControllerEvent::RequestTransition` from `VEN/src/controller/user_request.rs` when a user request transitions status (Created, Scheduled, Active, Completed, Cancelled, Abandoned). Push event via the AppState `push_controller_event` method or return events to caller.
- [ ] T045 [US3] Deploy to Pi4-Server and run ledger/packet BDD scenarios to verify energy totals remain correct after the accounting move. Confirm `GET /ledger` values match `GET /sim` cumulative energy.

**Checkpoint**: Packet energy accounting is authoritative (from measured values). Ledger and packet lifecycle BDD scenarios pass.

---

## Phase 6: User Story 4 — Dual-Mode VTN Reporting (Priority: P4)

**Goal**: Relocate `reporter.rs` to `controller/reporter.rs` with two distinct builder functions. Timer-driven measurement reports use `AssetHistoryBuffer`; event-driven status reports fire on `PlanCycle` and `PacketTransition` events.

**Independent Test**: With a VTN connected, a `PlanCycle` event triggers a TELEMETRY_STATUS report submission visible in VTN reports. The timer-driven TELEMETRY_USAGE report fires on schedule.

- [ ] T046 [US4] Create `VEN/src/controller/reporter.rs` with `pub fn build_measurement_report(asset_history: &HashMap<String, AssetHistoryBuffer>, ven_name: &str, now: DateTime<Utc>) -> Option<serde_json::Value>`. Content: TELEMETRY_USAGE / READING — last row of each asset's history (power_kw, cumulative energy, soc_pct where available). No `reactor_mode` parameter.
- [ ] T047 [P] [US4] Add `pub fn build_status_report(event: &ControllerEvent, asset_history: &HashMap<String, AssetHistoryBuffer>, ven_name: &str, now: DateTime<Utc>) -> Option<serde_json::Value>` to `VEN/src/controller/reporter.rs`. Content: TELEMETRY_STATUS — describes VEN's current response from the most recent asset history rows. Only emits a report for `PlanCycle` and `PacketTransition` event types; returns `None` for all others.
- [ ] T048 [US4] Delete `VEN/src/reporter.rs` (old top-level reporter).
- [ ] T049 [US4] Update `VEN/src/main.rs` timer-driven report block: replace `reporter::build_reports_for_active_events(...)` call with `controller::reporter::build_measurement_report(asset_history, &ven_name, now)`. Update import.
- [ ] T050 [US4] Add event-driven status report dispatch in `VEN/src/main.rs`: in the planning loop, after `push_controller_event(PlanCycle {...})`, call `controller::reporter::build_status_report(&event, &history, &ven_name, now)` and if `Some(report)`, submit via `vtn.upsert_report(report)`. Log failure without retry.
- [ ] T051 [US4] In the tick loop, after `monitor::record_tick` returns and `PacketTransition` events have been pushed, fire event-driven status reports for each `PacketTransition` event (same pattern as T050).
- [ ] T052 [US4] Run `cargo build` and fix any compile errors from the reporter relocation and old `reporter::` call sites.
- [ ] T053 [US4] Deploy to Pi4-Server and run reporter-related BDD scenarios to verify TELEMETRY_USAGE and TELEMETRY_STATUS reports are submitted correctly.

**Checkpoint**: Dual-mode reporting working. No `reactor_mode` parameter anywhere in reporter code.

---

## Phase 7: User Story 5 — Tariff Nomenclature Verification (Priority: P5)

**Purpose**: Verify the tariff rename (implemented in Phase 2) is complete and all BDD scenarios for `GET /tariffs` pass. No additional Rust implementation — this phase is verification and cleanup only.

**Independent Test**: `GET /tariffs` returns the same data structure as the old `GET /rates`. Zero occurrences of `RateSnapshot`, `PlannedRates`, `PastRates` in `VEN/src/`. `GET /rates` returns 404.

- [ ] T054 [P] [US5] Run grep over `VEN/src/` for `RateSnapshot`, `PlannedRates`, `PastRates`, `rate_snapshot` — verify zero occurrences. Fix any missed callers if found.
- [ ] T055 [US5] Deploy to Pi4-Server and run the `GET /tariffs` BDD scenario added in T009. Verify 200 response with correct tariff structure. Verify `GET /rates` returns 404.

**Checkpoint**: Tariff rename verified end-to-end.

---

## Phase 8: Polish & Cross-Cutting Concerns

**Purpose**: Full BDD suite verification, cargo tests, documentation, and final cleanup.

- [ ] T056 Run the complete BDD suite on Pi4-Server (`docker compose -f tests/docker-compose.test.yml run --build --rm test-runner`) — all scenarios must pass, zero failures.
- [ ] T057 [P] Run cargo tests for the VEN service (`docker compose -f tests/docker-compose.openleadr-test.yml run --rm cargo-test cargo test --workspace --jobs 2`) — all tests pass.
- [ ] T058 [P] Verify `VEN/src/reactor/` directory does not exist (`ls VEN/src/reactor/` returns error).
- [ ] T059 [P] Grep `VEN/src/main.rs` and `VEN/src/state.rs` for `reactor` — verify zero remaining references outside comments.
- [ ] T060 [P] Verify `UserOverrides` in `VEN/src/state.rs` has no force-override fields (`ev_force_kw`, `heater_force_kw`, `battery_force_kw`, `pv_force_export_limit_kw`).
- [ ] T061 Write `docs/history/project_journal.md` entry for Phase 004: what was done, why, issues encountered, key learnings.
- [ ] T062 [P] Update `docs/reference/KEY_LEARNINGS.md` with any new learnings from this refactor (tick loop sequencing, ring buffer design, BDD-first gate enforcement).

---

## Dependencies & Execution Order

### Phase Dependencies

- **Phase 1 (BDD First)**: No dependencies — start immediately. BLOCKS all Rust phases.
- **Phase 2 (Foundational)**: Depends on Phase 1 completion. BLOCKS all user story phases.
- **Phase 3 (US1)**: Depends on Phase 2. Foundational reactor deletion — recommend completing before US2–US4.
- **Phase 4 (US2)**: Depends on Phase 2 + Phase 3. Requires tick loop and state.rs in final form.
- **Phase 5 (US3)**: Depends on Phase 4 (AssetHistoryBuffer wired). `record_tick` supersedes tick-loop history writes from T032.
- **Phase 6 (US4)**: Depends on Phase 4 (asset history available for measurement reports) and Phase 5 (PacketTransition events available).
- **Phase 7 (US5)**: Depends on Phase 2 (rename done). Verification only.
- **Phase 8 (Polish)**: Depends on all prior phases.

### User Story Dependencies

- **US5 (Tariff Rename)**: Independent rename — implemented in Phase 2, verified in Phase 7. No dependency on US1–US4.
- **US1 (Single Control Path)**: Depends on Foundational (Phase 2). No dependency on US2–US5.
- **US2 (Observability)**: Depends on US1 tick loop being in final form.
- **US3 (Packet Accounting)**: Depends on US2 (AssetHistoryBuffer wired in tick loop; record_tick supersedes T032 history writes).
- **US4 (Dual Reporting)**: Depends on US2 (asset history available) + US3 (PacketTransition events).

### Parallel Opportunities Within Phases

**Phase 1**: T004 and T005 can run in parallel (different files). T007 and T008 can run in parallel.

**Phase 2**: T012 (mod.rs) and T016 (ControllerEvent) and T023 (timeline.rs) are fully independent — run in parallel.

**Phase 3**: T025 (delete reactor files) and T026 (dispatcher rewrite) are in different files — run in parallel.

**Phase 4**: T034, T035, T037, T038 all touch different files — can run in parallel. T036, T039 depend on T034/T035/T037/T038 completing first.

**Phase 6**: T046 and T047 (two builder functions) can be written in parallel.

---

## Parallel Example: Phase 3 (US1)

```bash
# These two tasks touch different files and can run in parallel:
Task T025: Delete VEN/src/reactor/ (5 files)
Task T026: Rewrite VEN/src/controller/dispatcher.rs build_setpoints
# Then sequentially:
Task T027: Remove reactor declarations from main.rs
Task T028: Rewrite tick loop in main.rs (depends on T025, T026 complete)
```

## Parallel Example: Phase 4 (US2)

```bash
# These touch different files and can run in parallel:
Task T034: Emit OpenAdrArrived/Expired from openadr_interface.rs
Task T035: Emit RateChange/CapacityChange from openadr_interface.rs
Task T037: Add GET /trace/events handler in main.rs
Task T038: Add GET /trace/history handler in main.rs
# Then:
Task T036: Wire event emission calls in main.rs poll-events task
Task T039: Register new routes, remove old /trace route
```

---

## Implementation Strategy

### MVP First (US1 Only — Phase 1 + 2 + 3)

1. Complete Phase 1: BDD first (red phase)
2. Complete Phase 2: Foundational (tariff rename + trace types + state.rs)
3. Complete Phase 3: US1 (reactor deletion + dispatcher redesign + tick loop rewrite)
4. **STOP and VALIDATE**: Run UC-01–UC-12 — all must pass. Reactor is gone. Single control path confirmed.

### Incremental Delivery

1. Phase 1 + 2 → Foundation and BDD gate satisfied
2. Phase 3 → US1 complete → UC-01–UC-12 green → deploy
3. Phase 4 → US2 complete → trace endpoints live → deploy
4. Phase 5 → US3 complete → ledger authoritative → deploy
5. Phase 6 → US4 complete → dual-mode reporting active → deploy
6. Phase 7 → US5 verified → tariff rename end-to-end confirmed
7. Phase 8 → Full validation and documentation

---

## Notes

- Total tasks: 62
- [P] parallelizable tasks: 22
- BDD-first gate (Phase 1) is enforced by constitution Principle II — no Rust changes until T010 confirms red suite
- US5 (tariff rename) has no separate implementation phase — renamed in Phase 2, BDD written in Phase 1, verified in Phase 7
- `monitor.record_tick` (T041) supersedes the history row writes in the tick loop (T032) — when implementing T041, remove the per-asset history write from T032 or consolidate into `record_tick`
- No SQLx changes — offline cache is not invalidated in this speckit
- After each deploy, run with `--build` flag to ensure new image is built
