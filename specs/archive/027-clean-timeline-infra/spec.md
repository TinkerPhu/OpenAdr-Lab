# Feature Specification: Clean Timeline Infra Imports

**Feature Branch**: `027-clean-timeline-infra`  
**Created**: 2026-05-15  
**Status**: Draft  
**Predecessor**: `026-reporter-domain-types` (Phase 1 — same refactoring plan)  
**Refactoring Plan**: `docs/plans/ven_backend_architecture_refactoring_v2.md` §Phase 2

---

## User Scenarios & Testing *(mandatory)*

### User Story 1 — Timeline rendering continues to work after refactoring (Priority: P1)

The asset timeline page in the UI shows past power history and future plan allocations for every
controllable asset (battery, EV, heater, PV, base load, grid). This must still work correctly —
same data, same time ranges, same state overlays (SoC, temperature) — after the internal
representation of `TimelineSnapshot` is changed to use only domain types.

**Why this priority**: P1 because this is the observable contract. The refactoring must be
invisible to the UI. If the timeline breaks, the refactoring has introduced a regression.

**Independent Test**: Can be verified by the existing BDD scenarios exercising the
`GET /timeline/{asset_id}` endpoint on the Pi4-Server docker stack, plus the existing unit tests in
`controller/timeline.rs` (which must pass without modification, or be updated to the new
domain-only fixture helpers).

**Acceptance Scenarios**:

1. **Given** the VEN stack is running with simulated assets, **When** the UI requests the timeline
   for an EV asset with 1 hour history and 2 hours forward, **Then** the response contains past
   power and SoC readings plus future plan allocations with cost and CO2 rate metadata, identical
   to today's response.

2. **Given** the VEN stack is running, **When** the UI requests the timeline for the virtual
   `"grid"` asset, **Then** the response contains past net-import/export history and future plan
   tariff data, identical to today's response.

3. **Given** a timeline unit test that constructs `TimelineAssetData` with plain domain values,
   **When** `build_asset_timeline` is called, **Then** the test compiles and passes without
   importing any type from `crate::assets` or `crate::simulator`.

---

### User Story 2 — Unit tests for timeline are independent of the simulator (Priority: P2)

After this change, a developer writing a new timeline unit test must be able to construct all
needed test fixtures using only domain primitives — no `AssetHistoryBuffer`, no `AssetConfig`, no
`AssetState`. All infra-to-domain type conversion must happen in the simulator layer, not inside
`controller/timeline.rs`.

**Why this priority**: This is the architectural goal. Without it, the feature is incomplete even
if the UI still works.

**Independent Test**: `cargo test` passes for the timeline module with zero `use crate::assets` or
`use crate::simulator` imports remaining in `controller/timeline.rs`.

**Acceptance Scenarios**:

1. **Given** the refactored `controller/timeline.rs`, **When** the codebase is searched for
   `use crate::assets` in that file, **Then** zero matches are returned.

2. **Given** the refactored test module inside `controller/timeline.rs`, **When** the tests use
   `TimelineAssetData` fixtures, **Then** those fixtures are constructed from domain-only types
   with no infra crate imports in the test helper functions.

---

### Edge Cases

- Asset with no history (empty history) must return a valid result from `build_asset_timeline` —
  same behaviour as today (returns `Some([])` when the asset is known).
- Grid virtual asset (`"grid"`) uses a separate history field rather than the `assets` map; its
  conversion must be handled at the infra boundary in `to_timeline_snapshot`.
- `build_now_point` today derives a 60-second rolling-average power to smooth oscillating assets
  (heater thermostat). The refactored `TimelineAssetData` must carry enough pre-computed data for
  this smoothing to remain correct — no regression in the now-point calculation.
- State overlay values (SoC for battery/EV, temperature for heater) appear in the past history
  portion of the timeline. These are currently extracted from `AssetState` via
  `AssetConfig::state_values()`. After the refactoring, they must be pre-computed at the infra
  boundary (`to_timeline_snapshot`) and carried into `TimelinePoint` so `build_asset_timeline`
  can still include them without touching `AssetState`.
- `plan_trajectory` (live heater temperature projection) is currently derived from
  `AssetConfig` + `AssetState`. The domain-side snapshot must carry enough pre-computed initial
  state to support this computation, or the feature must fall back to the already-computed
  `planned_state_by_asset` map from plan slots (consistent with how battery/EV SoC is handled).

---

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: `controller/timeline.rs` MUST contain zero imports from `crate::assets` after this
  change. Verified by `grep "use crate::assets" VEN/src/controller/timeline.rs` returning empty.

- **FR-002**: `controller/timeline.rs` MUST contain zero imports from `crate::simulator` after
  this change.

- **FR-003**: All timeline data currently surfaced to the UI (past power, past state overlays such
  as SoC and temperature, future plan allocations, cost and CO2 rates) MUST remain present and
  numerically correct after the change.

- **FR-004**: New domain-side snapshot types (`TimelinePoint` or equivalent, and the updated
  `TimelineAssetData`) MUST be defined inside `controller/timeline.rs` and use only types from
  the domain and entities rings (`entities/`, `controller/`).

- **FR-005**: `simulator/mod.rs` — `to_timeline_snapshot()` — MUST perform all infra-to-domain
  type conversions (mapping history buffer entries to `TimelinePoint`, converting asset config
  variant to `AssetType`, calling `state_values()` for each history point to preserve SoC and
  temperature overlays) before returning the snapshot. This function remains in the infra layer
  and is the only place where infra types are consumed for timeline purposes.

- **FR-006**: `build_asset_timeline()` and `build_now_point()` MUST remain callable from a unit
  test using only domain-side fixtures — no `SimState`, no `AssetConfig`, no `AssetHistoryBuffer`.

- **FR-007**: File size constraints MUST be respected: `controller/timeline.rs` stays at or below
  500 lines; `simulator/mod.rs` stays at or below 500 lines after the conversion logic is added.

- **FR-008**: All existing timeline unit tests MUST pass after the change. Tests may be updated to
  use new domain-only fixture helpers but must not be weakened or removed.

### Key Entities

- **TimelinePoint**: New domain-side record representing one sampled moment in an asset's history.
  Carries at minimum `ts` (timestamp) and `power_kw` plus any state-overlay values (SoC,
  temperature) pre-computed from the infra layer. Defined in `controller/timeline.rs`.

- **TimelineAssetData** (updated): Domain-level bundle of an asset's timeline data. Replaces the
  current infra fields (`AssetHistoryBuffer`, `AssetConfig`, `AssetState`) with domain-only
  equivalents: asset identifier, asset type label (`AssetType` from entities ring), history as
  `Vec<TimelinePoint>`, and pre-computed current values needed for the now-point calculation.

- **TimelineSnapshot** (updated): Top-level snapshot used by `build_asset_timeline` and
  `build_now_point`. The `grid_history` field changes from `AssetHistoryBuffer` to
  `Vec<TimelinePoint>`.

- **AssetType**: Enum already in `entities/asset.rs` (domain ring). Used by `TimelineAssetData` to
  identify asset kind without coupling to physics config.

---

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: `grep "use crate::assets" VEN/src/controller/timeline.rs` returns zero matches.

- **SC-002**: `grep "use crate::simulator" VEN/src/controller/timeline.rs` returns zero matches.

- **SC-003**: All timeline unit tests in `controller/timeline.rs` pass green with zero failures.

- **SC-004**: The full BDD suite on Pi4-Server docker passes without new failures — specifically
  all scenarios exercising the timeline API endpoints.

- **SC-005**: A developer can construct a `TimelineAssetData` value in a unit test using zero
  imports from `crate::assets` or `crate::simulator`; that test compiles and passes.

- **SC-006**: `controller/timeline.rs` stays at or below 500 lines and `simulator/mod.rs` stays
  at or below 500 lines after the change.

---

## Assumptions

- Phase 1 (`026-reporter-domain-types`) is fully merged and green before this work starts — the
  `AssetReportSample` pattern established there is the direct precedent for `TimelinePoint`.
- `AssetType` (in `entities/asset.rs`) already covers all asset variants present in the simulator;
  no new enum variants are needed.
- The caller chain `routes/timeline.rs` to `SimState::to_timeline_snapshot()` to `build_asset_timeline()`
  is unchanged at the API level; only the internal types passed through that chain change.
- The `plan_trajectory` feature (live heater temperature projection) can be supported by carrying
  a pre-computed initial state value into `TimelineAssetData`. If the domain-side representation
  cannot carry enough information, the acceptable fallback is to use `planned_state_by_asset`
  values from plan slots, consistent with how battery and EV SoC trajectories are handled today.
