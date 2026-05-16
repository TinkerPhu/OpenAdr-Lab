# Research: Reporter — Domain-Side Snapshot Types

**Feature**: 026-reporter-domain-types
**Date**: 2026-05-15
**Method**: Code archaeology (no external research needed — all unknowns resolved by reading source files)

---

## Decision 1: Which functions in reporter.rs hold `&SimState`?

**Decision**: 8 functions total (3 public, 5 private).

**Finding**: Reading `VEN/src/controller/reporter.rs`:

Public functions:
- `build_measurement_report(event, sim: &SimState, ven_name)` — line 107
- `build_measurement_reports_for_active_events(events, sim: &SimState, ven_name, now)` — line 186
- `build_measurement_report_for_obligation(obligation, sim: &SimState, ven_name, site_envelope)` — line 238
- `build_status_report(event, sim: &SimState, ven_name, program_id, now)` — line 525

Private functions:
- `latest_net_import_kw(sim: &SimState)` — line 80
- `latest_net_export_kw(sim: &SimState)` — line 90
- `build_net_site_power_ts(sim: &SimState)` — line 363
- `build_soc_intervals(sim: &SimState, interval_width, duration_iso)` — line 407

Additional `HistoryPoint` usage:
- `points_to_power_ts(points: &[HistoryPoint], interpolation)` — line 25
- `soc_from_point(p: &HistoryPoint)` — line 34

**Rationale**: Need to update all 8. FR-004 covers all of them.

---

## Decision 2: Who calls the public reporter functions from outside reporter.rs?

**Decision**: Three callers — `publish.rs`, `obligation.rs`, and `planning.rs`. The spec assumption of "only publish.rs" is incomplete.

**Finding**:
- `build_measurement_reports_for_active_events` → `tasks/sim_tick/publish.rs::run_measurement_reports` (line 129)
- `build_measurement_report_for_obligation` → `services/obligation.rs::check_and_report` (line 30)
- `build_status_report` → `tasks/planning.rs` (line 234)

**Rationale**: All three callers must be updated to pass domain types. Each is an adapter-ring file — outer imports inner, which is correct per hexagonal architecture.

---

## Decision 3: Does `build_measurement_report_for_obligation` need a different domain type than `HashMap<String, Vec<AssetReportSample>>`?

**Decision**: No — `HashMap<String, Vec<AssetReportSample>>` covers the obligation case.

**Finding**: The obligation reporter calls `build_net_site_power_ts(sim)` which slices a 2-hour window from each asset's history. `AssetReportSample { ts, power_kw, soc }` is exactly a `HistoryPoint` with the infra state stripped. Each `Vec<AssetReportSample>` for an asset represents its 2h history. The `TimeSeries` conversion and LOCF aggregation operate purely on `(ts, power_kw)` pairs — no infra types needed.

**Rationale**: Single domain type serves both measurement report paths (latest-point for `build_measurement_report`, full history for `build_measurement_report_for_obligation`).

---

## Decision 4: How does `build_status_report` obtain `net_import_kw` after the change?

**Decision**: Compute from `SimSnapshot.assets.values()`.

**Finding**: `SimSnapshot.assets: HashMap<String, AssetSnapshot>` where `AssetSnapshot.power_kw: f64` is the latest tick's power. After accepting `snap: &SimSnapshot` instead of `sim: &SimState`, the helper `latest_net_import_kw` can be reimplemented as:
```rust
fn latest_net_import_kw(snap: &SimSnapshot) -> f64 {
    snap.assets.values().map(|a| a.power_kw).filter(|&kw| kw > 0.0).sum()
}
```
Or inlined directly in `build_status_report`. No soc extraction needed in the status report path.

**Rationale**: `SimSnapshot` is already in the domain ring (`controller/simulator_port.rs`). `AssetSnapshot.power_kw` is the direct equivalent of `sim.assets.iter().filter_map(|e| e.history.latest().map(|p| p.power_kw))`.

---

## Decision 5: How does `publish.rs::run_measurement_reports` obtain a `SimSnapshot`?

**Decision**: Accept `sim_snap: SimSnapshot` as a new parameter. The caller `tick.rs` already has `tick_sim_snap: SimSnapshot` computed at line 127.

**Finding**: `tick.rs::tick_once` calls `super::publish::run_measurement_reports(&state, &sim, &vtn, &ven_name, now)` at line 181. Before this call, `tick_sim_snap: SimSnapshot` is available in scope (computed at line 127 inside the lock block, returned as a tuple member). Adding it as a parameter is zero-cost — no new lock acquisition needed.

**Rationale**: Avoids re-locking sim or deriving scalars from the sample map (which would be equivalent but less explicit). Aligns with FR-007 requirement to derive from SimSnapshot.

---

## Decision 6: What is the EV SoC extraction path after removing `soc_from_point`?

**Decision**: `AssetReportSample.soc` is set to `p.state.soc()` at the infra boundary; reporter simply reads `sample.soc`.

**Finding**: Current flow in `build_measurement_report` (line 152):
```rust
if let Some(ev_entry) = sim.asset("ev") {
    if let Some(last) = ev_entry.history.latest() {
        if let Some(soc) = soc_from_point(last) {
```
After change:
```rust
if let Some(ev_samples) = asset_samples.get("ev") {
    if let Some(last) = ev_samples.last() {
        if let Some(soc) = last.soc {
```
The `soc_from_point` helper is no longer needed. `p.state.soc()` is called once, at the boundary in `publish.rs` / `obligation.rs`, when building `AssetReportSample`.

**Rationale**: Confirms FR-009. `soc_from_point` becomes dead code immediately after the change and must be removed.

---

## Decision 7: What happens to the existing `#[cfg(test)]` block?

**Decision**: Full rewrite. The helpers `make_sim`, `make_entry`, `make_ev_entry` are removed. New helpers build `AssetReportSample` fixtures directly.

**Finding**: The existing test block (~560 lines, lines 589–1153) makes heavy use of `SimState`, `AssetEntry`, `AssetHistoryBuffer`, `HistoryPoint`, `AssetState`, and `EnergyCounter`. After the change, none of these types exist in reporter.rs. All tests must be rewritten to construct `HashMap<String, Vec<AssetReportSample>>` and `SimSnapshot` directly.

Tests that were testing `soc_from_point` directly (2 tests) are removed with the function. Tests for `points_to_power_ts` are rewritten for `samples_to_power_ts`.

**Rationale**: FR-008 says logic unchanged — all business-logic assertions (payload types, field values, math) are preserved. Only the test setup (SimState construction) changes to AssetReportSample construction.

---

## Decision 8: Does the history window (2h) need to be parameterized?

**Decision**: No — hardcode `Duration::hours(2)` in both callers, matching the current implementation.

**Finding**: `build_net_site_power_ts` and `build_soc_intervals` both use `let full_window = Duration::hours(2)`. This is a constant in the reporter logic. When the callers extract history, they should use the same 2h window. No spec requirement to make this configurable.

**Rationale**: Lean architecture (Principle IV). Parameterizing adds complexity without a current requirement.

---

## Alternatives Considered

| Alternative | Why Rejected |
|-------------|-------------|
| Move `HistoryPoint` to domain ring | Out of scope (Phase 2 work, per spec Out of Scope section) |
| Pass full `SimState` clone to reporter (copied before lock release) | Keeps infra type in domain ring — defeats the purpose |
| Define `AssetReportSample` in a shared domain module (`controller/types.rs`) | Over-engineering: only `reporter.rs` uses it. Constitution Principle IV. |
| Keep `latest_net_import_kw` and `latest_net_export_kw` as `f64` scalar params to ALL functions | FR-003 specifies `&SimSnapshot` for `build_status_report`. The two approaches are different: scalar params for measurement functions (because `publish.rs` computes from snap and passes); `&SimSnapshot` directly for status report (planning.rs already has the snap). |
