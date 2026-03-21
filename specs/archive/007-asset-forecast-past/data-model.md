# Data Model: Asset Interface forecast() and history() — RF-01

## New Types

All three types live in: `VEN/src/common/mod.rs`

---

### `Interpolation` (enum)

| Variant | Meaning | Typical use |
|---|---|---|
| `Linear` | Values vary continuously; interpolate between adjacent samples | Power, temperature, SOC, irradiance |
| `Step` | Value holds constant (LOCF) from one sample until the next | Tariff, CO₂ intensity, EV on/off, base-load |

Used in two roles:
1. Governs how the mandatory boundary-point value is computed (FR-010)
2. Informs consumers how to read values between samples

---

### `Quantity` (enum)

Declares the physical or financial meaning of the `f64` values.

| Variant | Description | Typical `Unit` | Default `Interpolation` |
|---|---|---|---|
| `Power` | Instantaneous power at site/asset boundary | `Kilowatt` | `Linear` |
| `Energy` | Cumulative energy over an interval | `KilowattHour` | `Linear` |
| `StateOfCharge` | Battery/EV charge level | `Percent` | `Linear` |
| `Temperature` | Thermal state (room, ambient, device) | `Celsius` | `Linear` |
| `Irradiance` | Solar irradiance at PV surface | `WattsPerSquareMeter` | `Linear` |
| `Tariff` | Import or export price per energy unit | `EuroPerKilowattHour` | `Step` |
| `Co2Intensity` | CO₂ emission factor per energy unit | `GramsPerKilowattHour` | `Step` |

"Default interpolation" is a documentation convention — `interpolation` is always set explicitly on each `TimeSeries` instance.

---

### `Unit` (enum)

Declares the measurement scale of the `f64` values.

| Variant | Symbol | Quantity |
|---|---|---|
| `Kilowatt` | kW | Power |
| `KilowattHour` | kWh | Energy |
| `Percent` | % (0–100) | StateOfCharge |
| `Celsius` | °C | Temperature |
| `WattsPerSquareMeter` | W/m² | Irradiance |
| `EuroPerKilowattHour` | €/kWh | Tariff |
| `GramsPerKilowattHour` | gCO₂/kWh | Co2Intensity |

---

### `TimeSeries` (struct)

| Field | Type | Description |
|---|---|---|
| `samples` | `Vec<(DateTime<Utc>, f64)>` | Time-ordered samples. Empty for zero-duration timespan. |
| `quantity` | `Quantity` | What is being measured. |
| `unit` | `Unit` | Scale of the `f64` values. |
| `interpolation` | `Interpolation` | How to read values between samples and how to compute the boundary point. |

**Invariants:**
- Samples are strictly ascending in timestamp.
- For non-empty series from `forecast(timespan)`: last sample timestamp == `now + timespan`.
- For non-empty series from `history(timespan)`: first sample timestamp == `now − timespan`.
- Sign convention for power: positive = import from grid, negative = export.

**Upgrade path**: RF-05 introduces `TimeSeries<T>` alongside `TimeSeries`. A future speckit introduces `MultiTimeSeries` (multiple quantities per timestamp — e.g., power + SOC + temperature in one series), which builds on `TimeSeries`. No redesign of `TimeSeries` itself is required.

---

## Modified Types

### `AssetState` (enum — `VEN/src/simulator/assets/mod.rs`)

New dispatch methods replacing `predict()`:

| Method | Signature | Replaces |
|---|---|---|
| `forecast` | `(&self, timespan: Duration) -> TimeSeries` | `predict(setpoint, horizon_s, env)` |
| `history` | `(&self, timespan: Duration, history: &AssetHistoryBuffer) -> TimeSeries` | *(new)* |

`predict()` is removed. `forecast()` does not take a `setpoint` parameter — each asset uses its current internal setpoint.

---

### `run_planner()` (function — `VEN/src/controller/planner.rs`)

New parameter:

| Parameter | Type | Description |
|---|---|---|
| `asset_forecasts` | `&HashMap<String, TimeSeries>` | Pre-computed forecasts keyed by asset_id. Planner reads `"pv"` to replace the removed `pv_forecast()`. |

---

## Per-Asset Forecast Declarations

| Asset | `quantity` | `unit` | `interpolation` | Notes |
|---|---|---|---|---|
| PV | `Power` | `Kilowatt` | `Linear` | Negative (export). Sinusoidal irradiation model. |
| Battery | `Power` | `Kilowatt` | `Linear` | Sign follows setpoint. Power → 0 at SoC limit. |
| EV | `Power` | `Kilowatt` | `Step` | Zero if no session. Constant at setpoint if charging. |
| Heater | `Power` | `Kilowatt` | `Linear` | Thermal decay toward setpoint temperature. |
| BaseLoad | `Power` | `Kilowatt` | `Step` | Constant `baseline_kw`. |

---

## `history()` Data Extraction

`history(timespan, history: &AssetHistoryBuffer)` for all asset types:

1. Compute `start = now − timespan`.
2. Slice buffer to `[start, now]` using `AssetHistoryBuffer::to_timeline(Some((start, now)))`.
3. Extract `power_kw` column; drop NaN rows.
4. Prepend boundary point at `start` using the asset's `Interpolation` mode.
5. Return `TimeSeries { samples, quantity: Power, unit: Kilowatt, interpolation }`.

---

## Source Layout

```text
VEN/src/
  common/
    mod.rs          ← Interpolation, Quantity, Unit, TimeSeries  (NEW MODULE)
  simulator/
    assets/
      mod.rs        ← AssetState: forecast() + history() dispatch; predict() removed
      pv.rs         ← forecast(timespan) → TimeSeries
      battery.rs    ← forecast(timespan) → TimeSeries
      ev.rs         ← forecast(timespan) → TimeSeries
      heater.rs     ← forecast(timespan) → TimeSeries
      base_load.rs  ← forecast(timespan) → TimeSeries
  controller/
    planner.rs      ← pv_forecast() removed; asset_forecasts param added
  main.rs           ← compute forecast map; wire history() into timeline handler
```
