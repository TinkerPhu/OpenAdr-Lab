# Feature Specification: Asset Interface — forecast() and history() Methods

**Feature Branch**: `007-asset-forecast-past`
**Created**: 2026-03-20
**Status**: Draft
**Backlog ref**: RF-01 (`docs/BACKLOG.md`)

---

## Background

The HEMS system manages a set of energy assets (PV, battery, EV charger, heater, base load). Currently:

- Every asset exposes a `predict()` method, but all implementations are **stubs** that return only the current snapshot at the moment of the call — no forward projection.
- The **planner** contains its own `pv_forecast()` function (a sinusoidal model) that duplicates physics already present in the PV asset itself. This means the planner and the PV asset can silently diverge.
- Historical data is held in the controller's ring buffer, not by the asset itself. Assets have no `history()` capability.
- When a measured sensor (real hardware, not a simulation) replaces a simulated asset in the future, all controller code that calls asset-specific formulas would need to be audited and changed — because it currently bypasses the asset's own knowledge.

This feature defines the full `forecast(timespan)` and `history(timespan)` contracts for each asset type, removing planner-side duplicates and establishing each asset as the single authoritative source for its own past and future.

---

## Design Decisions

### Parameter type — Duration (relative offset from now)

Both `forecast(timespan)` and `history(timespan)` take a **duration** (positive time offset), not an absolute timestamp.

| Aspect | Detail |
|--------|--------|
| **Pros** | Natural for the most common use case: "how much power in the next 4 hours?" or "the last 30 minutes". Caller does not need to supply or compute wall-clock endpoints. Consistent on every call because it always anchors at "now". |
| **Cons** | Cannot express an arbitrary window like "from 10:00 to 11:00". Makes deterministic testing slightly harder since the start time is implicitly `now` — mitigated by injectable clocks (see FR-ASSET-04 in `docs/REQUIREMENTS.md`). |

An absolute-endpoint variant (`forecast(until: DateTime)`) was considered. It was rejected because the planner always plans "from now for N slots × slot_duration" — a pure duration computation — and because aligning to OpenADR event boundaries is the job of the caller (or RF-05), not the asset.

### No `interval` parameter — raw samples only

An `interval` parameter was considered to let callers request a specific sampling grid (e.g., "every 5 minutes"). It was **rejected** because:

- Each asset independently implementing resampling logic duplicates work across 5 asset types with no shared semantics.
- Defining interval rounding rules at this layer pre-solves part of RF-05 (`TimeSeries<T>`), which is the correct home for resampling.
- RF-05 will introduce `TimeSeries<T>.resample(interval)` — at that point the return type of `forecast()` and `history()` becomes `TimeSeries<f64>` and resampling is applied once, uniformly, at the call site.

**Decision**: `forecast(timespan)` and `history(timespan)` return **raw samples** at the asset's natural resolution (e.g., physics tick rate for forecast; ring buffer row rate for past). The caller is responsible for alignment until RF-05 is in place.

> **Note for RF-05**: When `TimeSeries<T>` is introduced, `forecast()` and `history()` return types should be updated to `TimeSeries<f64>`. The rounding rule for `resample(interval)` is: forecast series start at `ceil(now, interval)` (next grid boundary); past series end at `floor(now, interval)` (most recent complete interval). Example: `now = 12:22`, `interval = 5 min` → forecast starts at `12:25`, past ends at `12:20`. This ensures series from different assets align on the same grid without further adjustment.

---

## User Scenarios & Testing

### User Story 1 — Planner Uses Per-Asset Forecasts (Priority: P1)

The HEMS planner requests a forward-looking power profile from each asset before building a plan. Each asset returns a time series covering the planning timespan at its natural resolution. The planner uses this to estimate per-slot load and generation without containing any asset-specific formulas itself.

**Why this priority**: This is the core correctness requirement. The planner's slot-cost calculations are only accurate if they use the asset's own forecast. The existing `pv_forecast()` duplication in the planner is the primary bug this feature fixes.

**Independent Test**: Implement `forecast()` on PV only and verify that the planner calls it and produces a PV-aware plan, while removing `pv_forecast()` from the planner source. The remaining assets can return flat profiles temporarily.

**Acceptance Scenarios**:

1. **Given** a PV asset with rated power 5 kW and current time at noon, **When** the planner requests a 4-hour forecast, **Then** the returned series covers 4 hours starting from now, with values following the asset's irradiation model (highest near noon, declining toward evening), and the planner no longer contains a separate `pv_forecast()` function.

2. **Given** a battery asset with current state-of-charge at 80%, **When** the planner requests a 2-hour forecast with a charge setpoint, **Then** the returned series shows power draw consistent with the charge rate and capacity remaining, clamped at rated power.

3. **Given** a base-load asset, **When** the planner requests any forward forecast, **Then** the returned series has constant values matching the baseline power.

4. **Given** an EV charger with no session in progress, **When** the planner requests a forecast, **Then** the returned series reflects zero active power.

5. **Given** a heater asset at current temperature 19 °C with setpoint 21 °C, **When** the planner requests a 1-hour forecast, **Then** the returned series shows declining power draw as the room approaches setpoint, consistent with the thermal model.

---

### User Story 2 — UI Timeline Uses Asset History (Priority: P2)

The VEN UI timeline chart displays historical power data for each asset. With `history()` on the asset interface, the controller can ask any asset for its recent history without knowing how the asset stores data internally.

**Why this priority**: Needed for timeline accuracy and for the asset abstraction to be complete. Secondary to the planner fix but required before measured assets can replace simulated ones.

**Independent Test**: Implement `history()` on PV and battery, verify that the timeline endpoint returns data from those methods, and confirm that simulated vs. measured assets produce the same API response structure.

**Acceptance Scenarios**:

1. **Given** a PV asset that has been running for 30 minutes, **When** the UI requests 30 minutes of history via `history()`, **Then** the returned series covers the last 30 minutes with power values consistent with the simulation log.

2. **Given** any asset with less history stored than the requested timespan (e.g., just started), **When** `history()` is called, **Then** only the available samples are returned (no error, no padding with zeros).

3. **Given** a timespan longer than the history buffer depth, **When** `history()` is called, **Then** the result covers only the available buffer (partial result, no error).

---

### User Story 3 — Simulated and Measured Assets Are Interchangeable (Priority: P3)

When a real sensor replaces a simulated asset, the planner, dispatcher, and reporter must not require changes. Only the asset's own implementation changes.

**Why this priority**: This is the architectural goal that justifies the refactoring. Without it, adding real hardware always requires controller-level changes. Lower priority because no measured assets are planned near-term, but the interface must be designed correctly now.

**Independent Test**: Implement a minimal stub `MeasuredPv` that satisfies the same interface as `SimulatedPv`. Swap it into the asset list and verify the full BDD suite passes without modifying planner, dispatcher, or reporter source.

**Acceptance Scenarios**:

1. **Given** a simulated PV asset replaced by a measured PV asset (same interface), **When** the planner requests a forecast, **Then** the plan is produced without modification to the planner or dispatcher source.

2. **Given** a measured asset that has no physics model for forward projection, **When** `forecast()` is called, **Then** the asset returns a flat profile at current power (valid fallback), and the planner accepts it without error.

---

### Edge Cases

- What if `timespan` is zero for `forecast()`? → Return an empty series (valid, not an error). No endpoint is added because there is no range to bound.
- What if `timespan` is zero for `history()`? → Return an empty series.
- What if the asset has no history yet (just started) but `timespan > 0`? → `history()` returns only the mandatory start endpoint (one sample at `now − timespan`) with a value interpolated from available data, or zero if no data exists at all.
- What if `timespan` is negative? → Treat as a caller error; document that timespan must be positive.
- What if the PV asset's rated power is zero (no PV configured)? → `forecast()` returns all zeros including the mandatory endpoint.
- What if the battery is at full SoC and a charge setpoint is requested? → Forecast shows zero or near-zero power (charge not possible); the mandatory endpoint at `now + timespan` also reflects zero/near-zero.
- What if no natural sample falls exactly on the endpoint timestamp? → The endpoint value is **interpolated** from surrounding samples using the series' declared interpolation mode: `Step` holds the last known value; `Linear` interpolates between the two nearest samples.

**Mandatory endpoint rule**: every non-empty series returned by `forecast()` or `history()` MUST contain a sample at exactly the boundary of the requested timespan:
- `forecast(timespan)`: last sample at `now + timespan`
- `history(timespan)`: first sample at `now − timespan`

This guarantees at least one sample for any positive timespan and gives downstream consumers (planner, chart, RF-05 resampler) a definite anchor to close the interval.

---

## Requirements

### Functional Requirements

- **FR-001**: Each asset type (PV, battery, EV charger, heater, base load) MUST implement a `forecast(timespan)` method that returns a `QuantitySeries` — a list of time-stamped samples from the current moment through `timespan` at the asset's natural resolution, with declared `Quantity`, `Unit`, and `Interpolation` fields.

- **FR-002**: Each asset type MUST implement a `history(timespan)` method that returns a `QuantitySeries` — a list of time-stamped samples covering the last `timespan` duration at the ring buffer's stored resolution, with declared `Quantity`, `Unit`, and `Interpolation` fields.

- **FR-003**: `forecast()` MUST use the same physics model that the asset uses for simulation — there must be no separate copy of asset-specific formulas anywhere in the controller or planner.

- **FR-004**: The planner's standalone `pv_forecast()` function MUST be removed; all forecast calls MUST be routed through the asset's `forecast()` method.

- **FR-005**: The `history()` method MUST return raw samples from the ring buffer sliced to the requested `timespan`. If fewer samples are available than the timespan covers, the method MUST return only the available portion (partial result is valid).

- **FR-006**: The controller MUST NOT need to distinguish between simulated and measured asset implementations when calling `forecast()` or `history()`.

- **FR-007**: The `forecast()` series MUST start at or after the current moment and MUST NOT contain past-timestamped entries.

- **FR-008**: The `history()` series MUST NOT contain future-timestamped entries.

- **FR-010**: Every non-empty `QuantitySeries` returned by `forecast()` MUST include a sample at exactly `now + timespan` as the last entry. Every non-empty `QuantitySeries` returned by `history()` MUST include a sample at exactly `now − timespan` as the first entry. If no natural sample falls on that timestamp, the value is computed by interpolating from surrounding samples according to the declared `Interpolation` mode (`Step` = hold last value; `Linear` = weighted interpolation between neighbours).

- **FR-011**: Each asset MUST populate all three metadata fields of the returned `QuantitySeries`: `quantity` (what is measured), `unit` (scale of the f64 values), and `interpolation` (how values between samples are read). For `forecast()` and `history()` returning power: `quantity = Power`, `unit = Kilowatt`, interpolation as appropriate per asset physics.

- **FR-012 (deferred to RF-05)**: Resampling of `QuantitySeries` to a caller-specified interval, including grid-aligned rounding, is out of scope for this feature. It will be addressed when `TimeSeries<T>` is introduced in RF-05. A future speckit will introduce `MultiQuantitySeries` (multiple quantities per timestamp) building on `QuantitySeries`.

### Key Entities

- **Asset**: A physical or simulated energy device (PV, battery, EV, heater, base load). Each asset owns its own forward model (for `forecast`) and its own history (for `history`). Assets are interchangeable through a common interface.

- **QuantitySeries**: The return type of both `forecast()` and `history()`. Lives in `VEN/src/common/`. Contains:
  - `samples` — a time-ordered list of `(timestamp, f64)` pairs
  - `quantity` — a `Quantity` variant declaring what is being measured
  - `unit` — a `Unit` variant declaring the scale of the `f64` values
  - `interpolation` — a declared `Interpolation` mode that describes how values between samples should be read and how the mandatory boundary point is computed

  `QuantitySeries` is the direct precursor to a future `MultiQuantitySeries` (multiple quantities per timestamp, introduced in a later speckit) and to `TimeSeries<T>` in RF-05. The structure is intentionally compatible with both.

- **Quantity** (enum): Declares what physical or financial quantity the series represents.
  - `Power` — instantaneous power at the site or asset boundary
  - `Energy` — cumulative energy over an interval
  - `StateOfCharge` — battery or EV charge level as a fraction
  - `Temperature` — thermal state (room, ambient, or device)
  - `Irradiance` — solar irradiance at the PV surface
  - `Tariff` — import or export price per unit of energy
  - `Co2Intensity` — CO₂ emission factor per unit of energy

- **Unit** (enum): Declares the measurement unit of the `f64` values.
  - `Kilowatt` — kW
  - `KilowattHour` — kWh
  - `Percent` — 0–100 (used for state-of-charge)
  - `Celsius` — °C
  - `WattsPerSquareMeter` — W/m²
  - `EuroPerKilowattHour` — €/kWh
  - `GramsPerKilowattHour` — gCO₂/kWh

- **Interpolation** (enum with two variants):
  - `Linear` — values vary continuously between samples; use weighted interpolation to read any point between two known samples. Appropriate for power, temperature, state-of-charge.
  - `Step` — value holds constant from one sample until the next (last-observation-carried-forward). Appropriate for discrete states, on/off loads, tariffs, and constant base-load.

- **Timespan**: A positive duration. Used as the sole parameter for both `forecast()` (how far ahead) and `history()` (how far back).

- **Boundary Point**: The mandatory endpoint sample that every non-empty `QuantitySeries` must contain — `now + timespan` for `forecast()`, `now − timespan` for `history()`. Its value is interpolated from surrounding samples using the declared `Interpolation` mode.

---

## Success Criteria

### Measurable Outcomes

- **SC-001**: After implementation, zero asset-specific formulas remain in the planner or dispatcher modules — all such logic lives exclusively within each asset's own code.

- **SC-002**: The planner produces a valid cost-optimal plan for all 5 asset types (PV, battery, EV, heater, base load) using only their `forecast()` outputs — verified by the existing BDD test suite passing without modification to test scenarios.

- **SC-003**: The timeline chart in the VEN UI correctly displays per-asset history sourced via `history()` — verified by an automated UI test.

- **SC-004**: A simulated asset can be replaced by a minimal measured-asset stub (same interface, different implementation) without modifying planner, dispatcher, or reporter source files — verified by swapping in the stub and running the full BDD suite.

- **SC-005**: All 5 asset `forecast()` methods pass unit tests covering: zero-length timespan, night-time PV (zero output), battery at full/empty SoC boundary, EV with no active session.

- **SC-006**: `history()` correctly handles partial history (buffer not yet full) — returns available data without error — verified by unit test.

---

## Dependencies and Assumptions

**Depends on:**
- RF-05 (`TimeSeries<T>`) is NOT required first. This feature uses the existing ring buffer and raw sample lists directly. When RF-05 lands, the return types of `forecast()` and `history()` are updated to `TimeSeries<f64>` — no interface redesign needed, only a type upgrade.

**Assumptions:**
- History ring buffer capacity (~1 hour at 1 Hz) is sufficient for the `history()` use cases targeted here. Longer historical windows are out of scope.
- Irradiation is the primary simulated quantity for PV (per FR-SIM-03 in `docs/REQUIREMENTS.md`); `P_pv` is derived as `irradiance × rated_kw`. The `forecast()` method for PV produces `P_pv` at each step, not raw irradiance.
- Battery `forecast()` assumes the setpoint passed to the planner remains constant for the entire timespan. Dynamic setpoint scheduling within a forecast is out of scope.
