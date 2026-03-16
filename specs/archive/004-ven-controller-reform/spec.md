# Feature Specification: VEN Controller Reform

**Feature Branch**: `004-ven-controller-reform`
**Created**: 2026-03-15
**Status**: Draft
**Input**: Refactor the VEN controller layer — remove reactor, simplify dispatcher, reform trace system, reorganise controller module, dual-mode VTN reporting

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Single Authoritative Control Path (Priority: P1)

As a VEN operator, the device responds to OpenADR events through a single, predictable control path — the planner — so that setpoints are always consistent with the current plan and the system never silently produces conflicting control decisions.

**Why this priority**: The reactor created a parallel control path that conflicted with the planner without any notification. Removing it is the foundational correctness fix that all other stories depend on.

**Independent Test**: Can be tested by verifying that VTN event signals result in planner-driven setpoints only — no separate reactor setpoints are applied — and all UC-01–UC-12 BDD scenarios continue to pass.

**Acceptance Scenarios**:

1. **Given** the reactor module has been removed, **When** a VTN event with an import capacity limit arrives, **Then** the planner produces a revised plan and the dispatcher applies its setpoints — no FSM state transitions occur.
2. **Given** the reactor module has been removed, **When** the UC-01 through UC-12 controller BDD scenarios are run, **Then** all scenarios pass without any reactor-related step definitions.
3. **Given** the `reactor/` directory has been deleted, **When** the VEN application builds, **Then** the build succeeds with no references to `reactor` in `main.rs`, `state.rs`, or route handlers.

---

### User Story 2 - Transparent Controller Observability (Priority: P2)

As a VEN operator or developer, I can inspect both a structured event log of all controller decisions and a time-series history of each asset's power and state, so that I can audit what the controller did and why.

**Why this priority**: Replaces the old `GET /trace` endpoint with a richer, split observability model — a typed event log (what happened) and an asset history buffer (time series values). This is needed for correct monitoring and debugging in production.

**Independent Test**: Can be tested by calling `GET /trace/events` to see controller events and `GET /trace/history?asset=ev` to see per-asset historical data rows after a short period of VEN operation.

**Acceptance Scenarios**:

1. **Given** the VEN is running with active tariff rates and an energy packet, **When** `GET /trace/events` is called, **Then** the response contains typed controller event entries including `OpenAdrArrived`, `RateChange`, `PlanCycle`, and `PacketTransition` entries with timestamps.
2. **Given** the VEN has been running for at least 5 seconds, **When** `GET /trace/history?asset=ev&limit=10` is called, **Then** the response contains up to 10 rows, each with `power_kw`, `soc_pct`, and `cost_rate_eur_h` fields.
3. **Given** the old `GET /trace` endpoint has been removed, **When** `GET /trace` is called, **Then** the response is 404.

---

### User Story 3 - Correct Packet Energy Accounting (Priority: P3)

As a VEN operator, the energy attributed to each active packet accurately reflects what the simulator actually delivered this tick — regardless of plan changes or setpoint gaps — so that packet accounting is authoritative and ledger data is trustworthy.

**Why this priority**: The monitor taking over energy accounting from the dispatcher places it after the sim tick where actual power values are known. This eliminates accounting errors from planned-vs-actual divergence.

**Independent Test**: Can be tested by running a packet through its lifecycle and verifying that `GET /ledger` shows energy that matches cumulative sim power output.

**Acceptance Scenarios**:

1. **Given** an active FIRM packet for the EV asset, **When** the sim tick runs, **Then** the monitor attributes energy equal to `|power_kw| × elapsed_time` to the active packet.
2. **Given** `GET /ledger` is called after several sim ticks with an active packet, **Then** the ledger energy for that asset matches the cumulative energy reported by `GET /sim`.
3. **Given** no active packet covers a tick interval, **When** the monitor records the tick, **Then** no energy is attributed to any packet.

---

### User Story 4 - Dual-Mode VTN Reporting (Priority: P4)

As a VEN operator, measurement reports are sent on a timer and status reports are sent immediately on significant controller events (plan cycles, packet transitions) — so that the VTN receives both regular telemetry and prompt status updates without coupling timing to business logic.

**Why this priority**: Splits reporting into two orthogonal modes: timer-driven telemetry and event-driven status. The old reporter mixed these concerns and had a `reactor_mode` parameter that no longer applies.

**Independent Test**: Can be tested independently by verifying that a plan cycle event triggers a status report send, while a timer expiry triggers a measurement report send, in two separate test scenarios.

**Acceptance Scenarios**:

1. **Given** the VEN is connected to a VTN and a plan cycle runs, **When** a `PlanCycle` controller event is emitted, **Then** a TELEMETRY_STATUS report is sent to the VTN within the same controller processing cycle.
2. **Given** the measurement report timer has elapsed, **When** the reporter check runs, **Then** a TELEMETRY_USAGE/READING report built from asset history data is sent to the VTN.
3. **Given** the reporter has been relocated to the controller module, **When** the VEN builds, **Then** no `reactor_mode` parameter exists in any report builder function.

---

### User Story 5 - Tariff Nomenclature Alignment (Priority: P5)

As a developer working on the VEN codebase, Rust source uses `TariffSnapshot` / `GET /tariffs` consistently instead of `RateSnapshot` / `GET /rates` — matching the project's established nomenclature where tariff = price per kWh.

**Why this priority**: Eliminates the naming inconsistency introduced before the nomenclature convention was established. Unblocks the UI rename planned for speckit 3.

**Independent Test**: Can be tested by building the VEN and confirming `GET /tariffs` returns the same data that `GET /rates` previously returned, with no `RateSnapshot` type remaining in Rust source.

**Acceptance Scenarios**:

1. **Given** the rename has been applied, **When** `GET /tariffs` is called, **Then** the response is identical in structure and content to the previous `GET /rates` response.
2. **Given** the rename has been applied, **When** the Rust source is searched for `RateSnapshot`, `PlannedRates`, or `PastRates`, **Then** no results are found.
3. **Given** `GET /rates` has been removed, **When** `GET /rates` is called, **Then** the response is 404.

---

### Edge Cases

- What happens when the dispatcher receives a plan with no slot covering the current time? — Each asset falls back to its own default setpoint; the device is not left in an uncontrolled state.
- What happens when a new VTN event arrives while a dispatch cycle is in progress? — The planner replans immediately; the dispatcher applies new setpoints next tick; no ramp state is preserved.
- What happens when `AssetHistoryBuffer` is full and a new row is written? — Oldest row is overwritten (ring buffer semantics); no error is raised.
- What happens when an event-driven status report fails to send (VTN unreachable)? — The failure is logged; the send is not retried inline; the next timer-driven report will carry updated state.
- What happens when `POST /sim/override` receives an unknown key? — Unknown keys are silently ignored; only recognised control schema keys take effect.
- What happens when an ExportCapLimit constraint is active and the plan also allocates PV export? — The hard capacity constraint from the dispatcher overrides the plan allocation for the PV asset setpoint.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: The system MUST remove the `reactor/` module entirely — no FSM states, arbitration logic, or reactor tick interval.
- **FR-002**: The system MUST remove all force-override fields (`ev_force_kw`, `heater_force_kw`, `battery_force_kw`, `pv_force_export_limit_kw`) from `UserOverrides`.
- **FR-003**: The `POST /sim/override` endpoint MUST accept a generic map keyed by control schema keys; environmental overrides (e.g. `ambient_temp_c`, `irradiance_override`) MUST continue to work.
- **FR-004**: The system MUST expose `GET /trace/events` returning a list of typed `ControllerEvent` entries from a ring-buffer event log.
- **FR-005**: The system MUST expose `GET /trace/history?asset=<id>&limit=N` returning time-series rows from the asset history buffer for the named asset.
- **FR-006**: Controller event entries MUST be written by the OpenADR interface (on event arrival/expiry and rate/capacity changes), the planner (on each plan cycle), and the dispatcher/user request handler (on packet/request status transitions).
- **FR-007**: The monitor MUST write one row per asset per tick to the asset history buffer containing: measured power, all state values (e.g. `soc_pct`, `temp_c`), cost rate, and CO2 rate; the grid asset row MUST additionally include import/export prices and import/export limits.
- **FR-008**: The monitor MUST handle packet energy accounting on every tick, attributing measured energy to the active packet for each asset.
- **FR-009**: The dispatcher MUST scan all FIRM and FLEXIBLE plan slots for the current time and fill gaps with each asset's default setpoint.
- **FR-010**: The dispatcher MUST enforce an active export capacity limit constraint directly on the PV asset setpoint, regardless of plan content.
- **FR-011**: The reporter MUST be relocated to the controller module with two distinct builder functions: one for timer-driven measurement reports (TELEMETRY_USAGE/READING) and one for event-driven status reports (TELEMETRY_STATUS); the `reactor_mode` parameter MUST be removed.
- **FR-012**: Event-driven status reports MUST be sent from the controller orchestrator on `PlanCycle` and `PacketTransition` events — not from the tick loop.
- **FR-013**: The controller module MUST declare sub-modules in three logical groups: VTN protocol adapter (`openadr_interface`, `reporter`), control logic (`planner`, `dispatcher`, `user_request`), and observability (`trace`, `monitor`, `timeline`).
- **FR-014**: A `timeline` stub module MUST be created with a `build_asset_timeline` public function signature for use in speckit 3.
- **FR-015**: The tariff entity file and type names MUST be renamed: `rate_snapshot.rs` → `tariff_snapshot.rs`, `RateSnapshot` → `TariffSnapshot`, `PlannedRates` → `PlannedTariffs`, `PastRates` → `PastTariffs`.
- **FR-016**: The `GET /rates` route MUST be renamed to `GET /tariffs`; all callers MUST be updated.
- **FR-017**: All UC-01 through UC-12 BDD scenarios MUST continue to pass after the refactor.
- **FR-018**: All BDD scenarios testing FSM states, reactor arbitration, or force-override fields MUST be deleted (not skipped) from the test suite.
- **FR-019**: BDD scenarios referencing `GET /trace` MUST be rewritten to use `GET /trace/events` or `GET /trace/history?asset=<id>` as appropriate.
- **FR-020**: BDD scenarios for `POST /sim/override` MUST be updated to the new generic key format.

### Key Entities

- **ControllerEvent**: A typed log entry recording a significant controller decision or state change; includes timestamp and event-specific fields (event name, signal type, slot counts, packet/request IDs, status transitions).
- **ControllerEventLog**: A ring buffer of `ControllerEvent` entries with a fixed capacity; exposed via `GET /trace/events`.
- **AssetHistoryBuffer**: A columnar ring buffer storing time-series rows per asset (power, state values, cost rate, CO2 rate); exposed via `GET /trace/history`.
- **TariffSnapshot**: Renamed from `RateSnapshot`; holds import/export prices per kWh for a time interval; returned by `GET /tariffs`.
- **UserOverrides**: Simplified struct — force-override fields removed; remaining fields are environmental/device-spec inputs (e.g. `ambient_temp_c`, `irradiance_override`).

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: All 12 UC-01–UC-12 controller BDD scenarios pass after the refactor with zero failures.
- **SC-002**: Zero reactor BDD scenarios and zero force-override scenarios remain in the test suite (verifiable by searching for deleted step patterns).
- **SC-003**: `GET /trace/events` returns a non-empty list of typed controller events after 10 seconds of VEN operation.
- **SC-004**: `GET /trace/history?asset=ev&limit=5` returns exactly 5 rows each containing measured power and asset state values after 5 sim ticks.
- **SC-005**: `GET /tariffs` returns the same data structure as the old `GET /rates`; zero occurrences of `RateSnapshot`, `PlannedRates`, or `PastRates` remain in Rust source files.
- **SC-006**: The VEN application builds and all cargo tests pass with zero references to the `reactor` module.
- **SC-007**: A `PlanCycle` event triggers a TELEMETRY_STATUS report send within the same controller processing cycle (verifiable via integration test or log output).
- **SC-008**: `GET /ledger` energy totals remain consistent with `GET /sim` cumulative energy after the monitor takes over packet energy accounting.

## Assumptions

- The `AssetHistoryBuffer` data structure already exists in `controller/trace.rs` from speckit 1 and only needs wiring — no new data structure design is required here.
- `SimSnapshot.assets: HashMap<String, AssetSnapshot>` is available from speckit 1 and provides measured power and state values per asset.
- The UI is not changed in this speckit; the `GET /tariffs` rename is backend-only; UI rename (`useRates` → `useTariffs`) is deferred to speckit 3.
- `GET /timeline/*` endpoints are out of scope for this speckit (deferred to speckit 3); only the `timeline.rs` stub module is created here.
- The existing UC-01–UC-12 scenarios exercise the planner/dispatcher/request flow, which is structurally unchanged — only the reactor parallel path is removed.
- A CO2 factor (`co2_g_kwh`) is accessible to the monitor (per-VEN profile or system default) for computing `co2_rate_g_h`.
