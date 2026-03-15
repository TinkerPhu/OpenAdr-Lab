# Feature Specification: Asset Request Dispatch Refactor

**Feature Branch**: `003-asset-request-dispatch`
**Created**: 2026-03-15
**Status**: Draft

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Submit Charging Request for Any Storage Asset (Priority: P1)

A developer or operator submits a user request targeting a storage asset (EV charger or battery) by specifying a desired state-of-charge target. The system correctly determines the required energy quantity and charging power for that specific asset based on its own configuration and current state — without the request controller needing to know the asset type.

**Why this priority**: This is the core existing behavior that must continue to work correctly after the refactor. Any regression here is a critical failure.

**Independent Test**: Submit `POST /requests` for `asset_id: "ev"` with `target_soc: 0.9` and verify the response contains a correctly computed `target_energy_kwh` value matching the EV's configured battery capacity and current SoC.

**Acceptance Scenarios**:

1. **Given** an EV charger asset with known current SoC and battery capacity, **When** a user request is submitted with a `target_soc` value higher than the current SoC, **Then** the request is accepted with a `target_energy_kwh` value equal to the SoC delta multiplied by the battery capacity.
2. **Given** a battery asset with known current SoC and capacity, **When** a user request is submitted with a `target_soc` value higher than the current SoC, **Then** the request is accepted with a `target_energy_kwh` value equal to the SoC delta multiplied by the capacity.
3. **Given** a storage asset already at or above the requested `target_soc`, **When** a user request is submitted, **Then** the request is rejected with a zero-energy error (no useful work to do).

---

### User Story 2 - Reject Request for Non-Storage Asset (Priority: P2)

A developer or operator submits a user request targeting an asset that does not support SoC-based charging (e.g., a solar inverter or base load). The system returns a clear error rather than silently ignoring or mishandling the request.

**Why this priority**: Correct boundary rejection prevents confusing partial states and ensures the API contract is well-defined for all asset types.

**Independent Test**: Submit `POST /requests` for `asset_id: "pv"` and verify a 400-level error response is returned.

**Acceptance Scenarios**:

1. **Given** a PV inverter asset exists, **When** a user request is submitted targeting that asset, **Then** the system returns a rejection indicating the asset is not requestable.
2. **Given** a heater asset exists, **When** a user request is submitted targeting that asset via SoC target, **Then** the system returns a rejection indicating the asset is not requestable.

---

### User Story 3 - Add New Storage Asset Type Without Modifying the Request Controller (Priority: P3)

A developer adds a new energy-storage asset type to the system (e.g., a hot water tank with SoC-like capacity). The developer only needs to implement the asset's own behavior — they do not need to edit the request controller or any dispatch switch to enable request support.

**Why this priority**: This is the long-term maintainability goal of the refactor. Without it, every new asset type creates a hidden coupling point.

**Independent Test**: Verify that the request controller module contains no per-asset-type switch or named config lookups for specific asset IDs. A new asset type can be made requestable by adding logic only within that asset's own module.

**Acceptance Scenarios**:

1. **Given** the codebase after refactoring, **When** a developer reviews the request controller, **Then** no asset-type-specific branch or named config accessor (e.g., `ev_config()`, `battery_config()`) appears in that module.
2. **Given** the codebase after refactoring, **When** the request controller is asked to resolve a target for an asset, **Then** it delegates entirely to the asset's own self-describing behavior.

---

### Edge Cases

- What happens when `target_soc` is omitted and `target_energy_kwh` is provided directly? The system accepts the explicit energy value without consulting the asset's SoC at all.
- What happens when `desired_power_kw` is omitted? The system falls back to the asset's own default maximum charge rate.
- What happens when the requested `asset_id` does not exist in the active asset list? The system returns a "unknown asset" error distinct from the "not requestable" error.
- What happens when `target_energy_kwh` is provided as zero or negative? The system rejects the request with a zero-energy error.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: The system MUST resolve charging request targets for energy-storage assets (EV charger, battery) using only each asset's own current state and configuration — without the request controller maintaining per-type knowledge.
- **FR-002**: The system MUST reject user requests targeting assets that do not support SoC-based energy requests (heater, PV inverter, base load) with a clear error response.
- **FR-003**: The system MUST accept `target_energy_kwh` as a direct override, bypassing SoC-based computation entirely when provided.
- **FR-004**: The system MUST return an "unknown asset" error when the requested `asset_id` does not match any asset in the active configuration.
- **FR-005**: The system MUST continue to support `POST /requests` with identical request/response behavior for all previously supported asset types (EV charger and battery) after this change.
- **FR-006**: The system MUST fall back to each asset's configured default charge rate when `desired_power_kw` is not specified in the request.
- **FR-007**: The request controller module MUST NOT import or reference the profile configuration structure directly.

### Key Entities

- **AssetEntry**: An active asset in the VEN simulator. Has an identity (`id`), a type-specific state (including configuration and current readings), and the ability to declare whether it supports energy requests and how to compute targets.
- **UserRequest**: A developer- or operator-initiated instruction to charge a specific asset to a target state. Contains the target asset ID, optional SoC target, optional energy quantity, and optional desired charging rate.
- **RequestTarget**: The resolved pair of (required energy in kWh, desired power in kW) derived from a user request and the current asset state. This is the output of the target resolution step.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: All existing user request scenarios for EV charger and battery assets pass without modification to test expectations — 0 regressions in the existing BDD test suite.
- **SC-002**: A request targeting a non-storage asset (PV, heater, base load) returns a 400-level error in 100% of cases.
- **SC-003**: The request controller module contains 0 references to specific asset type names ("ev", "battery") or profile accessor methods for named asset configs.
- **SC-004**: Adding a new energy-storage asset type requires changes to 0 files outside that asset's own module to enable request support.

## Assumptions

- The currently active asset list is accessible at the point of request handling (already available in the application state).
- Asset current state (including SoC) is always up-to-date in memory at request time — no additional reads are needed.
- The `soc_pct` key in each storage asset's `state_values()` map is the canonical source for current SoC at request time.
- The minimum energy threshold below which a request is considered "zero energy" (and rejected) remains 1e-6 kWh.
- Profile configuration remains the source of truth for per-asset defaults (capacity, max charge rate, SoC target); it is accessed via the asset's own state, not via the request controller.
