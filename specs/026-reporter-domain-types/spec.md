# Feature Specification: Reporter — Domain-Side Snapshot Types

**Feature Branch**: `026-reporter-domain-types`
**Created**: 2026-05-15
**Status**: Draft
**Input**: Close the two remaining Phase 2 architecture gaps in `controller/reporter.rs`: the file imports `crate::assets::HistoryPoint` and `crate::simulator::SimState`, both of which are infra-ring types. The controller ring must never import the infra ring. The fix introduces a minimal domain-side sample type and changes all public function signatures to accept domain types only. The calling adapter (`publish.rs`) becomes responsible for extracting and mapping history data before calling reporter functions.

## Clarifications

### Session 2026-05-15

- Q: Should `build_measurement_report` accept both `grid_net_import_kw` and `grid_net_export_kw` as separate `f64` parameters, or only import? → A: Both — `grid_net_import_kw: f64, grid_net_export_kw: f64` as separate parameters.

## User Scenarios & Testing *(mandatory)*

### User Story 1 — Developer adds a unit test for build_measurement_report without a simulator (Priority: P1)

A developer writing a unit test for `controller/reporter.rs::build_measurement_report` constructs test inputs entirely from plain Rust structs — no `SimState`, no `AssetConfig`, no YAML profile. The test compiles and passes in milliseconds.

**Why this priority**: This is the direct payoff of the structural fix. If the function still depends on `SimState`, it cannot be tested without a live simulator. This story is the definition of done for the domain purity goal.

**Independent Test**: Add a `#[cfg(test)]` block in `reporter.rs` that calls `build_measurement_report` with a manually constructed `Vec<AssetReportSample>` and verifies the returned `OadrReportBody` shape.

**Acceptance Scenarios**:

1. **Given** `reporter.rs` has no infra imports, **When** a `#[cfg(test)]` block constructs `AssetReportSample { ts, power_kw, soc }` values inline, **Then** `build_measurement_report` accepts them and returns a correctly shaped `OadrReportBody` without requiring `SimState`.
2. **Given** `build_status_report` is called with a manually constructed `SimSnapshot`, **When** the function runs, **Then** it returns an `OadrReportBody` with the correct program ID and resource readings drawn from `SimSnapshot.assets`.
3. **Given** all reporter tests use only domain types, **When** `wsl cargo test -p ven controller::reporter`, **Then** all tests pass with no infra dependency.

---

### User Story 2 — publish.rs extracts history and calls the updated reporter API (Priority: P1)

The tick publish adapter extracts a `HashMap<String, Vec<AssetReportSample>>` from the live `SimState` history, releases the lock, then calls the updated reporter functions. The reporter never touches `SimState`.

**Why this priority**: This is the callsite fix. The reporter function signature change is only correct if its one caller (`publish.rs`) properly prepares the data at the infra boundary before crossing into the domain.

**Independent Test**: `wsl cargo check -p ven` compiles cleanly — confirms publish.rs satisfies the new reporter API.

**Acceptance Scenarios**:

1. **Given** `publish.rs` holds `Arc<Mutex<SimState>>`, **When** it needs to call `build_measurement_reports_for_active_events`, **Then** it first locks the sim, maps each `HistoryPoint` to `AssetReportSample { ts: p.ts, power_kw: p.power_kw, soc: p.state.soc() }`, builds the map, releases the lock, then calls the reporter with the map.
2. **Given** the lock is released before calling the reporter, **When** the (potentially slow) reporter function runs, **Then** the simulator lock is not held during report construction.
3. **Given** `grid_net_import_kw` and `grid_net_export_kw` scalars are needed by the reporter, **When** `publish.rs` computes them, **Then** it derives them from `SimSnapshot.assets.values()` by summing positive and negative `power_kw` fields respectively.

---

### User Story 3 — Architecture invariant grep returns empty (Priority: P2)

The invariant check from the v2 architecture plan returns no matches.

**Why this priority**: This is the machine-verifiable proof that the fix is complete and will not regress.

**Independent Test**: Run the invariant grep; it must return empty.

**Acceptance Scenarios**:

1. **Given** the change is applied, **When** `grep "use crate::simulator\|use crate::assets" VEN/src/controller/reporter.rs` is run, **Then** it returns empty (exit 1 / no matches).
2. **Given** no other controller-ring files import simulator or assets, **When** `grep -r "use crate::simulator\|use crate::assets" VEN/src/controller` is run, **Then** it returns empty.

---

### Edge Cases

- `build_measurement_reports_for_active_events` (the plural outer function in `reporter.rs`) calls `build_measurement_report` in a loop — its signature must also be updated to accept the pre-extracted map.
- If any other file besides `publish.rs` calls reporter public functions, those callers must also be updated to pass `AssetReportSample` or `SimSnapshot` rather than `SimState`.
- `soc_from_point` in `reporter.rs` currently calls `p.state.soc()` where `state` is the concrete `AssetState` enum. After the change, `AssetReportSample.soc` is an `Option<f64>` computed at the infra boundary — the `soc_from_point` helper is no longer needed and should be removed.
- `latest_net_import_kw` and `latest_net_export_kw` become private helpers that accept `f64` scalars; they may be inlined entirely if the bodies become trivial.

---

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: A new struct `AssetReportSample { pub ts: DateTime<Utc>, pub power_kw: f64, pub soc: Option<f64> }` MUST be defined in `controller/reporter.rs`. It MUST NOT reference any type from `crate::assets` or `crate::simulator`.
- **FR-002**: `build_measurement_report` MUST accept `asset_samples: &HashMap<String, Vec<AssetReportSample>>`, `grid_net_import_kw: f64`, and `grid_net_export_kw: f64` instead of `sim: &SimState`.
- **FR-003**: `build_status_report` MUST accept `snap: &SimSnapshot` (from `controller/simulator_port.rs`) instead of `sim: &SimState`.
- **FR-004**: All other public or private functions in `reporter.rs` that currently accept `sim: &SimState` MUST be updated to accept only domain-ring types.
- **FR-005**: `reporter.rs` MUST NOT contain `use crate::assets::HistoryPoint` or `use crate::simulator::SimState` (or any other `use crate::simulator` or `use crate::assets` import).
- **FR-006**: `tasks/sim_tick/publish.rs` MUST extract `HashMap<String, Vec<AssetReportSample>>` from the locked `SimState` and release the lock before calling any reporter function.
- **FR-007**: `publish.rs` MUST derive `grid_net_import_kw` and `grid_net_export_kw` from the available `SimSnapshot` (sum of positive/negative `AssetSnapshot.power_kw` values) and pass these scalars to the reporter.
- **FR-008**: No logic changes are permitted in the reporter's report-building algorithms — only type substitutions and call-site adaptations. The produced `OadrReportBody` payloads MUST remain structurally identical to pre-change output.
- **FR-009**: The `soc_from_point` helper in `reporter.rs` MUST be removed or replaced — it currently reads the concrete `AssetState` enum, which will no longer exist in the reporter after this change. Its equivalent logic moves to the infra boundary in `publish.rs` (computing `soc: p.state.soc()` when building `AssetReportSample`).

### Key Entities

- **`AssetReportSample`** (new, domain ring): minimal per-asset record carrying a timestamp, power reading, and optional SoC. Defined in `controller/reporter.rs`. No physics types.
- **`SimSnapshot`** (existing, `controller/simulator_port.rs`): point-in-time snapshot of current simulator state; used by `build_status_report`. Already in domain ring.
- **`SimState`** (existing, `simulator/mod.rs`): concrete infra type. After this change, referenced only in `publish.rs` (adapter ring) — never in `controller/reporter.rs`.
- **`HistoryPoint`** (existing, `assets/mod.rs`): infra type. After this change, used only in `publish.rs` when extracting history — never in `controller/reporter.rs`.

### Out of Scope

- Moving `HistoryPoint` itself to the domain ring — that is Phase 2 work (`controller/timeline.rs`). Phase 1 leaves `HistoryPoint` in `assets/mod.rs` and keeps it out of `reporter.rs` by mapping it at the boundary in `publish.rs`.
- Any changes to report wire format, HTTP endpoints, or VTN API contracts.
- Changes to `controller/reporter.rs` test logic beyond adding the new `#[cfg(test)]` tests required by this spec.

---

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: `grep "use crate::simulator\|use crate::assets" VEN/src/controller/reporter.rs` returns empty (exit 1).
- **SC-002**: `wsl cargo check -p ven` exits 0 with zero errors and zero new warnings.
- **SC-003**: `wsl cargo test -p ven` exits 0 with all existing tests still passing.
- **SC-004**: At least one new `#[cfg(test)]` test exists in `reporter.rs` that calls `build_measurement_report` with an `AssetReportSample` fixture and asserts the returned `OadrReportBody` is `Some`.
- **SC-005**: At least one new `#[cfg(test)]` test exists in `reporter.rs` that calls `build_status_report` with a manually constructed `SimSnapshot` and asserts the result.
- **SC-006**: `publish.rs` acquires the sim lock, builds `HashMap<String, Vec<AssetReportSample>>`, releases the lock, then calls the reporter — the lock is not held across the reporter call.
- **SC-007**: No `OadrReportBody` field names, value types, or serialisation shapes are changed — existing BDD scenarios that assert on report payloads continue to pass.

---

## Assumptions

- `publish.rs` is the only caller of reporter public functions (`build_measurement_report`, `build_status_report`, `build_measurement_reports_for_active_events`). If other callers exist they must also be updated.
- `SimSnapshot` (in `controller/simulator_port.rs`) already contains `assets: HashMap<String, AssetSnapshot>` where `AssetSnapshot.power_kw` is the most-recent power reading — sufficient for the `grid_net_import_kw` / `grid_net_export_kw` scalar computation.
- `HistoryPoint.state.soc()` returns `Option<f64>` — the same value that `AssetReportSample.soc` will carry after extraction in `publish.rs`.
- No BDD scenario directly asserts on the internal function signatures of `reporter.rs` — only on the HTTP-level report payloads.
- The `tick.rs` 193/200 line constraint does not apply to this phase — all changes are in `reporter.rs` and `publish.rs`.
