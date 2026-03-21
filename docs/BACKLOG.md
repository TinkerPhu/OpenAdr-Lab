# VEN Backend Refactoring Backlog

Generated from architecture analysis on 2026-03-21.
Items ordered by execution priority: safe deletions first, then consolidations, then structural moves.

---

## RF-B01: Delete dead `dispatcher::update_packets`

**Status**: BACKLOG
**Risk**: Low (dead code removal)
**Files**: `VEN/src/controller/dispatcher.rs` (lines 81-131)

The function `update_packets` is marked with the comment "NOTE: This function will be superseded by `monitor::record_tick`" — and it was. `monitor::record_tick` is the only caller in the tick loop. `update_packets` is never called anywhere.

**Work**:
- Delete `update_packets` and its unused imports (`EnergyPacket`, `EnergySnapshot`, `PacketStatus`, `SimSnapshot`, `Uuid` — verify each isn't used by `build_setpoints`).
- Remove the now-unnecessary `use` of `EnergySnapshot` and `PacketStatus` if they become orphaned.

**Test**:
- `cargo build --workspace` compiles without errors.
- `cargo test --workspace` passes (no test references `update_packets`).
- `grep -r "update_packets" VEN/src/` returns zero hits.
- Full BDD suite passes unchanged (the function was never called at runtime).

---

## RF-B02: Delete unused `PlannedTariffs`, `PastTariffs`, `TariffHeuristic`

**Status**: BACKLOG
**Risk**: Low (dead code removal)
**Files**: `VEN/src/entities/tariff_snapshot.rs`

Three types are declared but never referenced outside their own file:
- `PlannedTariffs` (lines 18-43) — has `tariff_at()` and `avg_import_tariff()`, never called.
- `PastTariffs` (lines 129-133) — empty wrapper, never instantiated.
- `TariffHeuristic` (lines 136-150) — never instantiated.

**Work**:
- Delete the three structs and their `impl` blocks.
- Keep `TariffSnapshot` and `TariffTimeSeries` (those are actively used).

**Test**:
- `cargo build --workspace` compiles without errors.
- `grep -r "PlannedTariffs\|PastTariffs\|TariffHeuristic" VEN/src/` returns zero hits.
- All existing `tariff_snapshot::tests` still pass.
- BDD suite passes unchanged.

---

## RF-B03: Remove `obligations()` alias from `AppState`

**Status**: BACKLOG
**Risk**: Low (rename only)
**Files**: `VEN/src/state.rs`, callers in `main.rs`

`obligations()` (line 248) is a byte-for-byte duplicate of `report_obligations()` (line 239). Both return `self.inner.read().await.report_obligations.clone()`.

**Work**:
- Find all callers of `obligations()` (expected: `main.rs` event poll loop, obligation check loop).
- Replace with `report_obligations()`.
- Delete the `obligations()` method.

**Test**:
- `cargo build --workspace` compiles.
- `grep -r "\.obligations()" VEN/src/` returns only `report_obligations()` references and `due_obligations()`.
- BDD suite passes unchanged.

---

## RF-B04: Consolidate ISO 8601 duration parsers

**Status**: BACKLOG
**Risk**: Low (shared utility extraction)
**Files**: `VEN/src/controller/openadr_interface.rs`, `VEN/src/controller/reporter.rs`, `VEN/src/common/mod.rs`

Two independent implementations exist:
- `openadr_interface::parse_iso8601_duration` — full (Y/M/D/H/M/S), 80 lines.
- `reporter::parse_duration_secs` — partial (H/M/S only), 28 lines.

The reporter version silently ignores day-level durations, which is a latent bug if a report descriptor ever uses `P1D` style durations.

**Work**:
- Move `parse_iso8601_duration` to `common/mod.rs` as `pub(crate) fn parse_iso8601_duration_secs(s: &str) -> i64`.
- Replace both call sites with the shared version.
- Delete both local copies.

**Test**:
- All existing `openadr_interface::tests::test_parse_iso8601_duration_*` tests still pass (move them to `common::tests`).
- Add one test confirming `parse_iso8601_duration_secs("P1D")` returns 86400 (this was a gap in the reporter's version).
- `cargo test --workspace` passes.
- BDD suite passes unchanged.

---

## RF-B05: Extract `net_import_kw` helper in reporter

**Status**: BACKLOG
**Risk**: Low (local refactor within one module)
**Files**: `VEN/src/controller/reporter.rs`

The pattern:
```rust
let net_import_kw: f64 = asset_history
    .values()
    .filter_map(|buf| { buf.to_timeline(None).last()... })
    .filter(|&kw| kw > 0.0)
    .sum();
```
appears 3 times in `reporter.rs` (lines 108-115, 133-140, 250-257).

**Work**:
- Extract `fn latest_net_import_kw(asset_history: &HashMap<String, AssetHistoryBuffer>) -> f64`.
- Replace all three sites.

**Test**:
- `cargo build --workspace` compiles.
- Add a unit test: build an `AssetHistoryBuffer` with known values, verify `latest_net_import_kw` returns the correct sum of positive-power assets.
- BDD suite passes unchanged (reporter output is identical).

---

## RF-B06: Add `EnergyPacket::builder()` or `EnergyPacket::new()` constructor

**Status**: BACKLOG
**Risk**: Medium (touches 3 construction sites)
**Files**: `VEN/src/entities/energy_packet.rs`, `VEN/src/main.rs` (post_packets), `VEN/src/controller/planner.rs` (seed_to_packet), `VEN/src/controller/user_request.rs` (create_from_body)

Creating an EnergyPacket requires setting ~25 fields. Three sites duplicate the full struct literal:
- `post_packets` handler (main.rs:1326-1366)
- `seed_to_packet` (planner.rs:628-661)
- `create_from_body` (user_request.rs:118-145)

**Work**:
- Add `EnergyPacket::new(asset_id, target_energy_kwh, desired_power_kw, value_curve, now) -> EnergyPacket` that sets all defaults (Pending status, empty profiles, zero accumulators, etc.).
- Refactor all three call sites to use it, then override specific fields as needed.

**Test**:
- Unit test: `EnergyPacket::new(...)` returns a packet with `status == Pending`, empty `past_power_profile`, zero `accumulated_cost_eur`, correct `created_at`.
- `cargo test --workspace` passes.
- BDD suite passes unchanged — the constructed packets must be functionally identical.
- Specifically verify UC-07 (user request) and UC-01 (event-driven scheduling) BDD scenarios still pass.

---

## RF-B07: Consolidate `finalize_packets` into single-pass per packet

**Status**: BACKLOG
**Risk**: Low (local optimization in planner)
**Files**: `VEN/src/controller/planner.rs` (lines 508-543)

`finalize_packets` does three separate `firm_slots.iter().flat_map().filter().map().sum()` passes per packet to compute `allocated_kwh`, `cost`, and `co2`. These can be a single pass.

**Work**:
- Replace the three passes with one loop accumulating all three values.

**Test**:
- Existing `planner::tests` pass.
- Add a targeted test: create a plan with 2 packets across 3 slots, verify `estimated_cost_eur`, `estimated_co2_g`, and `estimated_completion` match the expected values.
- BDD suite passes unchanged.

---

## RF-B08: Extract event poll change-detection into a function

**Status**: BACKLOG
**Risk**: Medium (touches the critical event poll loop)
**Files**: `VEN/src/main.rs` (lines 125-273)

The event poll loop contains ~150 lines of inline logic: JSON traversal for trace events (OpenAdrArrived/Expired), rate change detection, capacity change detection. This logic is interleaved with state updates and should be a testable function.

**Work**:
- Extract a pure function: `fn detect_event_changes(events: &[Value], prev_ids: &HashSet<String>, prev_tariff_count: usize, prev_import_limit: Option<f64>, now: DateTime<Utc>) -> EventChanges` where `EventChanges` holds the lists of arrived/expired events, rate/capacity change flags, and parsed trace events.
- The loop body becomes: call `detect_event_changes`, push returned trace events, update prev state, store rates/capacity/obligations.

**Test**:
- Unit test `detect_event_changes` with: (a) new event appears -> OpenAdrArrived emitted, (b) event disappears -> OpenAdrExpired emitted, (c) tariff count changes -> RateChange emitted, (d) import limit changes -> CapacityChange emitted, (e) no changes -> no events emitted.
- BDD suite passes unchanged.

---

## RF-B09: Extract HTTP handlers and DTOs from `main.rs`

**Status**: BACKLOG
**Risk**: Medium (large structural move, no logic changes)
**Files**: `VEN/src/main.rs` -> new `VEN/src/routes.rs` (or `VEN/src/routes/` module)

`main.rs` is 1473 lines, of which ~800 are HTTP handler functions and their request/response DTOs. The `main()` function itself (startup + background loops) is the remaining ~600 lines.

**Work**:
- Create `VEN/src/routes.rs` (or `routes/mod.rs` with sub-modules by domain: `sim.rs`, `timeline.rs`, `hems.rs`, `trace.rs`).
- Move `AppCtx` to a shared location (e.g., keep in `main.rs` or put in `state.rs`).
- Move all handler functions and their query/body DTOs.
- `main.rs` retains: module declarations, `main()` with startup and background loops, router construction.

**Test**:
- `cargo build --workspace` compiles.
- `cargo test --workspace` passes.
- Full BDD suite passes unchanged (all 33+ features, 143+ scenarios).
- Verify every endpoint responds identically by running the E2E test suite.

---

## RF-B10: Extract background loops from `main.rs`

**Status**: BACKLOG
**Risk**: Medium (structural move, no logic changes)
**Depends on**: RF-B08, RF-B09 (cleaner after handlers and change-detection are extracted)
**Files**: `VEN/src/main.rs` -> new `VEN/src/loops.rs`

After RF-B09, `main.rs` still has ~600 lines of background loop setup. Each loop (program poll, event poll, report poll, sim tick, obligation check, planning, persistence) can be a function that takes its dependencies and returns a `JoinHandle`.

**Work**:
- Create `VEN/src/loops.rs` with functions like `spawn_program_poll(state, vtn, interval_secs) -> JoinHandle<()>`.
- `main()` becomes: config + state init + spawn calls + router + serve. Target: ~100 lines.

**Test**:
- `cargo build --workspace` compiles.
- `cargo test --workspace` passes.
- Full BDD suite passes unchanged.
- `wc -l VEN/src/main.rs` is under 150 lines.




---

## General Backlog

clean up docker orphans

ven-1 differs in naming scheme from othe VENs. this causes confusion and sometimes errors. can we unify them?

make the ven-1 id a uuid and change it in all test and seed references.

DB-level optimization for active event filter: add `ends_at timestamptz` computed column + index so the `?active=true` filter can run in SQL instead of post-filtering in Rust. Not needed until event tables grow large.


Add a filter in VTN UI event table to omit the past events.

Add a DB-Reset script so it can be re-seeded easily.


add a setup script that docker composes all required containers.


add code coverage tools to tests and formater and linter tools to be applied for each code change.


check and remove warnings in all builds.

check for code quality and refactoring possibilities.

write down all your findings to the test errors around VEN UI simulation tests into ven_ui_simulation_test_issues.md. 

The fix is there. Docker's layer cache is stale — it doesn't see the change to Simulation.tsx. Need to force a rebuild without cache


add time provider for simulation: 
pub trait TimeContext: Clone + Send + Sync + 'static {
    type Instant: Copy + Ord + Send + 'static;

    fn now(&self) -> Self::Instant;
    fn sleep_until(&self, deadline: Self::Instant) -> Pin<Box<dyn Future<Output = ()> + Send>>;
    fn sleep(&self, duration: Duration) -> Pin<Box<dyn Future<Output = ()> + Send>>;

    fn pause(&self);
    fn resume(&self);
    fn set_rate(&self, rate: f64);
    fn advance(&self, delta: Duration);
}


how can I test the ven controller in ui?


also add ui tests for UserRequests and Controller in VEN\ui\src\__tests__   


the ven poll interval should be configurable in the config file so during test we can easily shorten it. or is there a better option? 

reactor still there?
