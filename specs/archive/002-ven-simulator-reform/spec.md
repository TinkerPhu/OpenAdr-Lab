# Feature Specification: VEN Simulator Reform

**Feature Branch**: `002-ven-simulator-reform`
**Created**: 2026-03-15
**Status**: Draft
**Input**: Refactor the VEN simulator and profile configuration to use a generic, extensible asset model. Pure backend refactor — zero behavior change, zero API change, zero UI change. All existing BDD simulator scenarios must pass before and after.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Existing Simulator Behavior Preserved (Priority: P1)

A developer running the full BDD test suite after the refactor sees all existing simulator and controller scenarios pass without any test changes. The simulator still reads VEN profile YAML files, ticks physics correctly, and exposes device states and energy data via the same API contract as before.

**Why this priority**: This is the primary acceptance gate. If any existing behavior regresses, the refactor has introduced a defect. All downstream work (controller reform, timeline UI) depends on a stable, passing test suite.

**Independent Test**: Run the full BDD test suite against the refactored codebase. All 123 scenarios must pass with 0 failures and 0 step changes.

**Acceptance Scenarios**:

1. **Given** the VEN service is started with a profile YAML that previously used named device fields, **When** the BDD test suite is executed, **Then** all existing simulator and controller scenarios pass without modification.
2. **Given** the simulator is running with assets loaded from profile YAML, **When** `GET /sim` is called, **Then** the response contains device state and energy data consistent with the physics tick results.
3. **Given** a BDD scenario exercises `POST /sim/override` to force an asset setpoint, **When** the scenario runs after the refactor, **Then** the override is applied correctly and the scenario passes.

---

### User Story 2 - Generic Asset State API (Priority: P2)

A developer inspecting the live simulator via `GET /sim` sees the new generic `assets` map format, where each asset is identified by its configured `id` and carries its `power_kw` plus any asset-specific state values (e.g. `soc_pct`, `temp_c`, `irradiance`). No named per-device fields exist in the response.

**Why this priority**: The generic snapshot format is the API contract that the controller reform (speckit 2) and timeline UI (speckit 3) build on. It must exist and be correct before those features can proceed.

**Independent Test**: Call `GET /sim` on a running VEN instance and verify the response structure contains `assets: { "<id>": { "power_kw": <f64>, "values": { ... } } }` for every configured asset, plus grid-level fields (`net_power_w`, `import_w`, `export_w`, `import_kwh`, `export_kwh`).

**Acceptance Scenarios**:

1. **Given** a VEN profile with five asset types (EV, heater, PV, battery, base_load), **When** `GET /sim` is called, **Then** the response contains an `assets` map with five entries, each keyed by asset `id` and containing `power_kw` and type-specific state values.
2. **Given** a running simulator, **When** `GET /sim` is called, **Then** the response contains no named per-device fields at the top level (no `ev`, `heater`, `pv`, `battery`, `base_load` root keys).

---

### User Story 3 - Control Schema Discovery (Priority: P3)

A developer (or future UI) queries `GET /sim/schema` to discover what runtime controls each configured asset exposes. The response lists each asset's controllable parameters with label, type (slider/switch/number input), bounds, and unit — sufficient to render a control panel dynamically without hardcoding per-device knowledge.

**Why this priority**: This endpoint is new and required for the timeline UI work (speckit 3). It delivers independent value as a machine-readable capability descriptor for the simulator.

**Independent Test**: Call `GET /sim/schema` on a running VEN instance and verify the response contains a map from asset `id` to a list of control descriptors. Each descriptor must contain at minimum: key, label, kind, and unit.

**Acceptance Scenarios**:

1. **Given** a VEN with configured assets, **When** `GET /sim/schema` is called, **Then** each asset `id` maps to a non-empty list of control descriptors.
2. **Given** a control descriptor for a continuous parameter (e.g. charge rate), **When** the schema is inspected, **Then** the descriptor carries `min`, `max`, and `unit` fields.

---

### User Story 4 - Asset Reset and Config Endpoints (Priority: P3)

An operator or test script can reset an asset's initial state (e.g. set EV state-of-charge to 80%) or update a config parameter (e.g. change battery capacity) via dedicated endpoints, replacing the previous stub fields in the override body.

**Why this priority**: The stub fields in `UserOverrides` were a workaround. The proper endpoints remove that workaround and give tests a clean, explicit way to set initial conditions.

**Independent Test**: POST to `POST /sim/reset/ev` with `{"soc": 0.8}`, then call `GET /sim` and verify the EV reports the updated state of charge. Repeat for battery reset and battery config update.

**Acceptance Scenarios**:

1. **Given** a running simulator with an EV at default SoC, **When** `POST /sim/reset/ev` is called with `{"soc": 0.8}`, **Then** `GET /sim` returns the EV with `soc_pct` near 80 and the new state persists across the next tick.
2. **Given** a running simulator with a battery at default capacity, **When** `PUT /sim/config/battery` is called with `{"capacity_kwh": 20.0}`, **Then** `GET /sim` reflects the updated capacity and the battery operates within the new bounds.
3. **Given** `POST /sim/reset/battery` is called with `{"soc": 0.2}`, **Then** the battery SoC is updated and persisted to disk.

---

### User Story 5 - Profile YAML Migration (Priority: P1)

All four profile YAML files (`ven-1.yaml`, `ven-2.yaml`, `ven-3.yaml`, `test.yaml`) are migrated from named device fields to the typed asset list format. VEN instances start successfully with the migrated profiles and exhibit identical physics behavior.

**Why this priority**: Without migrated profiles, no VEN instance can start after the refactor. This is a prerequisite for all other stories.

**Independent Test**: Start all three VEN containers using migrated profile files. All BDD scenarios that exercise per-VEN behavior (different asset mixes) must pass.

**Acceptance Scenarios**:

1. **Given** a profile YAML with named device fields, **When** the profile is migrated to the typed asset list format, **Then** the VEN service starts without error and the simulator ticks correctly.
2. **Given** `test.yaml` is migrated, **When** the BDD test suite runs, **Then** the planner and controller scenarios that depend on specific initial SoC values in `test.yaml` continue to produce the expected FIRM/FLEXIBLE split.

---

### Edge Cases

- What happens when a profile YAML contains an unknown asset type? The service must reject startup with a clear error message identifying the unknown type.
- What happens when `POST /sim/reset/<type>` targets an asset type not present in the loaded profile? The endpoint must return a clear error (asset not found) rather than silently succeeding.
- What happens when the persisted `sim_state.json` was written in the old named-field format? The service must either migrate it on load or detect the incompatibility and reinitialize from the profile defaults.
- What happens when two assets in the same profile have the same `id`? Startup must fail with a duplicate-id error.
- What happens when `GET /sim/schema` is called while no assets are configured? The response must return an empty map (not an error).

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: The system MUST load simulator assets from a profile YAML using a typed list format where each entry declares its `type` and `id` alongside type-specific configuration fields.
- **FR-002**: The system MUST represent all simulator asset states in a single generic collection keyed by asset `id`, with no hardcoded named fields for individual device types.
- **FR-003**: Each asset type MUST implement a physics tick that returns the asset's actual power output for a given time step and environment.
- **FR-004**: Each asset type MUST implement a forward prediction that returns expected power output over a configurable horizon given a setpoint and environment.
- **FR-005**: Each asset type MUST expose its current state as a key-value map (e.g. `soc_pct`, `temp_c`, `irradiance`) that is included in the simulator snapshot.
- **FR-006**: Each asset type MUST declare its planning capabilities: maximum import/export power, whether it is flexible (accepts power allocations), and optionally its energy storage state and availability window.
- **FR-007**: Each asset type MUST declare a default setpoint used when no plan allocation is active.
- **FR-008**: Each asset type MUST expose a list of control descriptors defining its runtime-controllable parameters (label, kind, bounds, unit).
- **FR-009**: The `GET /sim` endpoint MUST return a generic snapshot with an `assets` map and grid-level totals (`net_power_w`, `import_w`, `export_w`, `import_kwh`, `export_kwh`). No named per-device fields may appear at the top level.
- **FR-010**: The `GET /sim/schema` endpoint MUST return a map from asset `id` to list of control descriptors for all configured assets.
- **FR-011**: The system MUST provide `POST /sim/reset/<type>` to reinitialize an asset's runtime state (e.g. SoC) and persist the result to disk.
- **FR-012**: The system MUST provide `PUT /sim/config/<type>` to update an asset's configuration parameters in place and persist the result to disk.
- **FR-013**: The stub fields `ev_initial_soc`, `battery_initial_soc`, and `battery_capacity_kwh` MUST be removed from the override body. These capabilities are replaced by FR-011 and FR-012.
- **FR-014**: The system MUST add a columnar `AssetHistoryBuffer` data structure to the codebase. This structure stores timestamped asset value series with configurable capacity and supports retrieval as a row-oriented timeline. (Wiring to live data is deferred to speckit 2.)
- **FR-015**: All four existing profile YAML files MUST be migrated to the typed asset list format. No named device fields may remain in any profile file.
- **FR-016**: The `GridMeter` MUST be derived from the sum of asset power outputs after each tick, not ticked as an asset itself.
- **FR-017**: All existing BDD simulator and controller scenarios MUST pass without any test modifications after the refactor.
- **FR-018**: Adding a new asset type MUST require only: one new file in the assets module, one new variant in the asset state enum, and one new variant in the asset config enum — no other files.

### Key Entities

- **AssetEntry**: A single asset in the simulator — carries its `id`, current physics state, last commanded setpoint, and cumulative energy counter.
- **AssetState**: A discriminated union over all supported asset types; dispatches physics tick, prediction, state export, capabilities, default setpoint, and control schema to the per-type implementation.
- **AssetConfig**: A discriminated union over all supported asset configuration types; deserialized from profile YAML using a `type` discriminator field.
- **AssetSnapshot**: The read-only view of a single asset at a point in time — power in kW and a key-value map of type-specific state values.
- **SimSnapshot**: The full simulator snapshot at a point in time — timestamp, grid totals, and an `assets` map from id to `AssetSnapshot`.
- **AssetCapabilities**: Planning interface descriptor for a single asset — power limits, flexibility flag, optional energy storage state, optional availability window.
- **ControlDescriptor**: A single controllable parameter descriptor — key, human-readable label, input kind (slider/switch/number), optional bounds, and unit string.
- **AssetHistoryBuffer**: A columnar time-series buffer for a single asset — one deque of timestamps and one deque per value key, with configurable capacity.
- **GridMeter**: Derived grid boundary values — net power, import, export (instantaneous and cumulative kWh).
- **TickEnvironment**: A key-value map of ambient values passed to all assets during a physics tick (e.g. hour of day, ambient temperature).

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: All 123 existing BDD scenarios (801 steps) pass with zero failures and zero test file modifications after the refactor is complete.
- **SC-002**: `GET /sim` returns a response where all asset data is contained within the `assets` map — verified by the absence of any top-level named device keys.
- **SC-003**: `GET /sim/schema` returns a non-empty control descriptor list for every flexible asset configured in the profile.
- **SC-004**: `POST /sim/reset/ev` and `POST /sim/reset/battery` each produce a verifiable state change observable in the next `GET /sim` response.
- **SC-005**: All four profile YAML files load successfully in the migrated format and produce identical simulator physics behavior to the pre-refactor baseline (verified by BDD test suite).
- **SC-006**: A code reviewer can add a hypothetical sixth asset type by touching exactly two files (new asset module + enum variant registration) — confirmed by inspection of the resulting diff.
- **SC-007**: The three stub override fields (`ev_initial_soc`, `battery_initial_soc`, `battery_capacity_kwh`) are absent from the override body schema after the refactor.

## Assumptions

- The `reactor/` module, `controller/dispatcher.rs`, and `controller/monitor.rs` are **not modified** in this feature. All changes are confined to the simulator and profile layers.
- `UserOverrides` force fields (e.g. `battery_force_kw`) that exist for BDD test support are **not removed** in this feature — only the three initialization stubs are removed.
- The BDD test suite already has full coverage of simulator physics and controller behavior; no new BDD scenarios are needed to validate this refactor.
- The `AssetHistoryBuffer` data structure is added but not wired to any data source in this feature — writing to the buffer is part of speckit 2.
- The `GET /sim/schema` endpoint is new but no UI change is made to consume it — that is speckit 3 scope.
- The sign convention (positive = import/consumption, negative = export/generation) is unchanged from the current implementation.
- Profile YAML files for `ven-1`, `ven-2`, `ven-3` do not all contain every asset type — the generic model must support sparse asset configurations (e.g., ven-1 may have no battery).
