# Implementation Plan: VEN Controller Reform

**Branch**: `004-ven-controller-reform` | **Date**: 2026-03-15 | **Spec**: [spec.md](spec.md)
**Input**: Feature specification from `/specs/004-ven-controller-reform/spec.md`

## Summary

Refactor the VEN controller layer to eliminate the reactor parallel control path and replace it with a single authoritative planner-driven control path. The work spans seven areas: (1) delete `VEN/src/reactor/` entirely, (2) redesign `dispatcher.rs` to a pure setpoint builder, (3) expand `monitor.rs` to own packet energy accounting and asset history writes, (4) wire up `AssetHistoryBuffer` and add `ControllerEventLog` in `trace.rs`, (5) relocate and split `reporter.rs` into dual-mode (timer-driven + event-driven) under `controller/`, (6) rename `RateSnapshot` → `TariffSnapshot` across all Rust callers, and (7) update `main.rs` tick loop and routes to match the new architecture. All UC-01–UC-12 BDD scenarios must continue to pass; reactor/force-override BDD scenarios are deleted.

## Technical Context

**Language/Version**: Rust (stable, 2021 edition)
**Primary Dependencies**: tokio (async runtime), axum (HTTP), chrono (timestamps), serde/serde_json, uuid, VecDeque (std)
**Storage**: In-memory ring buffers (`VecDeque`); persisted fields use existing JSON persistence — no schema changes
**Testing**: cargo test (unit), Python behave BDD (integration), Docker Compose test stack on Pi4-Server
**Target Platform**: Linux ARM64 (Raspberry Pi 4), Docker Compose v2
**Project Type**: Backend Rust service (web-service)
**Performance Goals**: Tick loop must complete in <100ms; ring buffer operations are O(1); no new async blocking
**Constraints**: Pi4 ARM64 resource limits (`cpus: 1.5`, `memory: 1500M`); SQLx offline cache must not be invalidated (no SQL changes in this speckit)
**Scale/Scope**: Single VEN service; ~800+ BDD steps maintained; 5 reactor files deleted; ~10 Rust source files changed

## Constitution Check

### Principle I — OpenADR Spec Fidelity ✅

No OpenADR field names are changed. The `GET /tariffs` rename is a Rust-internal naming fix (tariff vs rate), not an OpenADR field rename. All VTN-facing report payloads use spec-verbatim field names.

### Principle II — BDD-First Testing ✅ (GATE)

**Requirement**: New behavior MUST be described in behave scenarios before implementation.

**Plan**: BDD changes are **Step 1** of implementation. Before writing any Rust code:
- Delete reactor/force-override scenarios.
- Write new scenarios for `GET /trace/events`, `GET /trace/history`, `GET /tariffs`.
- Update `POST /sim/override` body scenarios.
- Run the suite — it must be red (failing) before implementation begins.

### Principle III — Upstream Compatibility ✅

Not applicable — this speckit touches only `VEN/src/` (the local VEN service), not the `openleadr-rs` git submodule. No upstream PR needed.

### Principle IV — Lean Architecture ✅

This speckit *reduces* complexity: deletes 5 reactor files, removes the parallel control path, simplifies `dispatcher.rs` to a pure function. The `ControllerEventLog` is a straightforward ring buffer identical in structure to `AssetHistoryBuffer`. No new abstractions are introduced.

### Principle V — Infrastructure Parity ✅

All testing runs on Pi4-Server via `docker compose -f tests/docker-compose.test.yml run --build --rm test-runner`. Deploy via standard git push → git pull → compose up flow.

**No Complexity Tracking violations** — this plan reduces total complexity.

## Project Structure

### Documentation (this feature)

```text
specs/004-ven-controller-reform/
├── plan.md              # This file
├── research.md          # Phase 0 output
├── data-model.md        # Phase 1 output
├── contracts/
│   └── api-changes.md   # Phase 1 output
└── tasks.md             # Phase 2 output (/speckit.tasks)
```

### Source Code Changes

```text
VEN/src/
├── main.rs                          # Major refactor: remove reactor, rewrite tick loop, update routes
├── state.rs                         # Remove trace field; add controller_trace; rename planned_rates
├── reactor/                         # DELETE entirely (5 files)
│   ├── mod.rs
│   ├── arbitration.rs
│   ├── fsm.rs
│   ├── interval.rs
│   └── trace.rs
├── reporter.rs                      # DELETE (moved to controller/reporter.rs)
├── entities/
│   ├── mod.rs                       # Update rate_snapshot → tariff_snapshot
│   └── rate_snapshot.rs             # RENAME to tariff_snapshot.rs; rename types inside
└── controller/
    ├── mod.rs                       # Add reporter, timeline; add group comments
    ├── trace.rs                     # ADD ControllerEvent, ControllerEventLog, ControllerTrace
    ├── dispatcher.rs                # REWRITE: build_setpoints (generic), remove update_packets
    ├── monitor.rs                   # EXPAND: record_tick (history + packet acctg + ledger)
    ├── reporter.rs                  # NEW (moved from root; split into two builder functions)
    └── timeline.rs                  # NEW stub

tests/features/
├── ven_dispatcher.feature           # Remove force-override scenarios; update GET /trace refs
├── ven_rate_system.feature          # Rename GET /rates → GET /tariffs
└── [any file with GET /trace]       # Rewrite to GET /trace/events or /trace/history
```

**Structure Decision**: Single-project Rust service. Backend only (no UI changes in this speckit).

## Implementation Phases

### Phase A: BDD First — Delete, Rewrite, Add Scenarios (BDD-FIRST GATE)

*Must complete before any Rust source changes. Run the suite — expect failures.*

**A1**: Scan all `.feature` files for:
- `GET /trace` (old endpoint)
- `GET /rates` (renamed)
- `ev_force_kw`, `heater_force_kw`, `battery_force_kw`, `pv_force_export_limit_kw`
- FSM state names: `Delaying`, `Ramping`, `Holding`, `RampingBack`

**A2**: Delete all scenarios found for force-overrides and FSM states.

**A3**: Rewrite `GET /trace` scenarios → `GET /trace/events` or `GET /trace/history`.

**A4**: Rename `GET /rates` → `GET /tariffs` in all feature files.

**A5**: Add new scenarios for `GET /trace/events` and `GET /trace/history?asset=ev`.

**A6**: Update `POST /sim/override` scenarios to use new generic body format (no force-override keys).

**A7**: Run BDD suite — expect failures on new endpoints and renamed endpoints.

---

### Phase B: Rename — TariffSnapshot

*Isolated rename with no behavioral change. Compile-test only.*

**B1**: Rename `VEN/src/entities/rate_snapshot.rs` → `VEN/src/entities/tariff_snapshot.rs`.

**B2**: Inside the file: `RateSnapshot` → `TariffSnapshot`, `PlannedRates` → `PlannedTariffs`, `PastRates` → `PastTariffs`, `RateHeuristic` → `TariffHeuristic`.

**B3**: Update `VEN/src/entities/mod.rs`: `pub mod rate_snapshot;` → `pub mod tariff_snapshot;`

**B4**: Fix all Rust callers of the old names:
- `state.rs`: `use crate::entities::rate_snapshot::RateSnapshot` → `tariff_snapshot::TariffSnapshot`; rename `planned_rates` field → `planned_tariffs`; update all getter/setter method names.
- `controller/openadr_interface.rs`: update return type.
- `controller/monitor.rs`: update parameter type.
- `controller/planner.rs`: update parameter type.
- `main.rs`: update all references.

**B5**: Rename `GET /rates` route → `GET /tariffs` in `main.rs`.

**B6**: `cargo build` must succeed cleanly after this phase.

---

### Phase C: Trace Reform — Add ControllerEventLog + ControllerTrace

*Add new types in trace.rs; update state.rs to store them.*

**C1**: In `controller/trace.rs`, add:
- `ControllerEvent` enum (all variants from data-model.md)
- `ControllerEventLog` struct with `push`, `entries`, `len` methods
- `ControllerTrace` struct holding both `event_log: ControllerEventLog` and `asset_history: HashMap<String, AssetHistoryBuffer>` with methods `push_event`, `push_asset_row`, `events`, `asset_history`.

**C2**: In `state.rs`:
- Add `use crate::controller::trace::ControllerTrace;`
- Remove `use crate::reactor::trace::TraceEntry;`
- In `InnerState`: remove `trace: Vec<TraceEntry>`; add `controller_trace: ControllerTrace` with `#[serde(skip)]`
- Update `InnerState::new()` initializer.
- Replace `update_sim(sim, trace)` with `update_sim(sim: SimSnapshot)`.
- Add new AppState methods: `controller_trace()`, `set_controller_trace()`, `push_controller_event(ControllerEvent)`, `planned_tariffs()`, `set_planned_tariffs()`.

**C3**: `cargo build` must succeed.

---

### Phase D: Dispatcher Redesign

*Rewrite dispatcher.rs to a pure setpoint builder.*

**D1**: Replace `get_setpoints` and `update_packets` with the new `build_setpoints` function:
```rust
pub fn build_setpoints(
    plan: &Plan,
    assets: &[AssetEntry],
    capacity: &OadrCapacityState,
    now: DateTime<Utc>,
) -> HashMap<String, f64>
```
- Scans FIRM and FLEXIBLE slots for the one covering `now`.
- Inserts `alloc.power_kw` for each allocation in the matching slot.
- Fills gaps with `asset.state.default_setpoint()` for every asset not covered.
- If `capacity.export_limit_kw.is_some()`, overrides the `pv` key.

**D2**: Delete `DispatcherSetpoints` struct.

**D3**: `cargo build` — expected compile errors in main.rs (dispatcher callers); fix forward by temporarily stubbing if needed.

---

### Phase E: Monitor Expansion — record_tick

*Expand monitor.rs to own packet accounting and asset history.*

**E1**: Replace `update_ledger` with `record_tick`:
```rust
pub fn record_tick(
    trace: &mut ControllerTrace,
    ledger: &mut HashMap<String, AssetLedgerEntry>,
    packets: &mut Vec<EnergyPacket>,
    snapshot: &SimSnapshot,
    tariffs: &[TariffSnapshot],
    dt_s: f64,
    now: DateTime<Utc>,
) -> Option<PlanTrigger>
```
Implementation details per data-model.md.

**E2**: Update `monitor.rs` imports: remove `RateSnapshot`, add `TariffSnapshot`, `AssetHistoryBuffer` via `ControllerTrace`.

**E3**: `cargo build` must succeed (main.rs callers still need updating — Phase G fixes these).

---

### Phase F: Reporter Reform

*Move and split reporter.rs into controller/reporter.rs.*

**F1**: Create `VEN/src/controller/reporter.rs` with:
- `pub fn build_measurement_report(asset_history: &HashMap<String, AssetHistoryBuffer>, ven_name: &str, now: DateTime<Utc>) -> Option<serde_json::Value>` — TELEMETRY_USAGE/READING from history rows.
- `pub fn build_status_report(event: &ControllerEvent, asset_history: &HashMap<String, AssetHistoryBuffer>, ven_name: &str, now: DateTime<Utc>) -> Option<serde_json::Value>` — TELEMETRY_STATUS.
- No `reactor_mode` parameter.
- No import from `reactor::interval`.

**F2**: Delete `VEN/src/reporter.rs`.

**F3**: Update `controller/mod.rs`: add `pub mod reporter;` and `pub mod timeline;`; add group comments.

**F4**: Create `VEN/src/controller/timeline.rs` stub.

**F5**: `cargo build` — fix any import errors.

---

### Phase G: Reactor Deletion + main.rs Rewrite

*The big step: remove reactor, rewrite tick loop, update routes.*

**G1**: Remove `VEN/src/reactor/` directory (all 5 files).

**G2**: Remove `mod reactor;` and `mod reporter;` from `main.rs` top.

**G3**: Rewrite the tick loop in `main.rs` to match the new sequence:
```
1. dispatcher::build_setpoints(plan, assets, capacity, now) → HashMap<String,f64>
2. sim.tick(dt_s, setpoints, now, &overrides)
3. monitor::record_tick(trace, ledger, packets, &sim_snap, tariffs, dt_s, now) → trigger_opt
4. state.update_sim(sim_snap)
5. reporter::maybe_send_measurement(history, now, vtn) — timer check
```

**G4**: Remove `reactor_state: Arc<Mutex<Reactor>>` from `main.rs`.

**G5**: Remove all force-override overlay code from tick loop.

**G6**: Update `AppCtx` — remove any reactor-related fields if any existed (none expected).

**G7**: Update `state.rs` `update_sim` call — remove `trace` parameter.

**G8**: Update route handlers:
- `get_trace` handler → `get_trace_events` returning `ctx.state.controller_trace().await.events()`
- Add `get_trace_history` handler with `asset` and `limit` query params
- Update route registrations: `.route("/trace", ...)` → `.route("/trace/events", ...)` + `.route("/trace/history", ...)`

**G9**: Add event-driven status report dispatch: after planning loop produces a `PlanCycle` event or after `PacketTransition` events, call `controller::reporter::build_status_report(...)` and submit to VTN.

**G10**: `cargo build` must succeed with zero warnings about unused reactor imports.

---

### Phase H: BDD Suite Green

*Fix any remaining BDD failures introduced by the refactor.*

**H1**: Deploy updated VEN to Pi4-Server.

**H2**: Run full BDD suite: `docker compose -f tests/docker-compose.test.yml run --build --rm test-runner`

**H3**: Expected passing: all UC-01–UC-12 scenarios.
Expected passing: new `GET /trace/events` and `GET /trace/history` scenarios.
Expected passing: `GET /tariffs` scenarios.

**H4**: Debug any failures. Common expected issues:
- Step definitions referencing old `GET /trace` URL — fix in step files.
- Step definitions referencing `GET /rates` — fix in step files.
- JSON field name mismatches in new trace endpoints — fix response serialization.

**H5**: All scenarios pass → zero failures → proceed to Phase I.

---

### Phase I: Cargo Tests + Cleanup

**I1**: Run `docker compose -f tests/docker-compose.openleadr-test.yml run --rm cargo-test cargo test --workspace --jobs 2` (openleadr-rs cargo tests are unaffected; VEN has no standalone cargo tests beyond the ones in trace.rs).

**I2**: Verify no `RateSnapshot`, `PlannedRates`, `PastRates` identifiers remain in `VEN/src/` via grep.

**I3**: Verify no `reactor` module reference remains in `main.rs`, `state.rs`, or any controller file.

**I4**: Verify `VEN/src/reactor/` directory does not exist.

**I5**: Update `docs/history/project_journal.md`.

**I6**: Update `docs/reference/KEY_LEARNINGS.md` if any new learnings.

---

## Risk Register

| Risk | Likelihood | Mitigation |
|------|-----------|------------|
| BDD step definitions still import reactor helpers | Medium | Grep `tests/steps/` for `reactor`, `fsm`, `force_kw` before Phase H |
| `reporter.rs` tests reference `find_active_intervals` from deleted reactor | High | All reporter unit tests must be rewritten in Phase F |
| Packet energy accounting behavior differs between dispatcher and monitor | Medium | Keep the same arithmetic (`|power_kw| * dt_h`); compare ledger totals via BDD in Phase H |
| `AssetHistoryBuffer` rows missing expected columns for some assets | Low | Ensure `state_values()` is called on all asset types in monitor; check PV (no `soc_pct`) |
| `main.rs` event-driven status report fires on every tick instead of on events | Medium | Event-driven reports are emitted from planning loop, not tick loop — enforce in code review |
| SQLx offline cache invalidated | None | No SQL changes in this speckit |
