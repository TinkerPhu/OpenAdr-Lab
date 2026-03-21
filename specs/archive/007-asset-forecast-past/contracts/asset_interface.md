# Contract: Asset Interface — forecast() and history()

Internal Rust function contracts. These define the behavioral guarantees each asset implementation must satisfy.

---

## Types

Lives in `VEN/src/common/mod.rs`.

```
enum Interpolation {
    Linear,   // interpolate between adjacent samples
    Step,     // hold last value (LOCF) until next sample
}

enum Quantity {
    Power, Energy, StateOfCharge, Temperature,
    Irradiance, Tariff, Co2Intensity,
}

enum Unit {
    Kilowatt, KilowattHour, Percent, Celsius,
    WattsPerSquareMeter, EuroPerKilowattHour, GramsPerKilowattHour,
}

struct TimeSeries {
    samples:       Vec<(DateTime<Utc>, f64)>,  // ascending timestamps
    quantity:      Quantity,
    unit:          Unit,
    interpolation: Interpolation,
}
```

---

## AssetState::forecast(timespan: Duration) -> TimeSeries

**Pre-conditions:**
- `timespan > Duration::zero()`

**Post-conditions:**
- `samples` timestamps are strictly ascending.
- All timestamps fall in `[now, now + timespan]`.
- If `timespan > Duration::zero()`: last sample timestamp == `now + timespan` (boundary point, FR-010).
- If `timespan == Duration::zero()`: `samples` is empty.
- `quantity = Power`, `unit = Kilowatt` for all current asset implementations.
- Power sign convention: positive = import from grid, negative = export/generation.
- `quantity`, `unit`, and `interpolation` are set consistently per asset (see table below).

**Per-asset guarantees:**
| Asset | At night (PV) | At SoC limit (battery) | No session (EV) | Interpolation |
|---|---|---|---|---|
| PV | Returns zero series | N/A | N/A | Linear |
| Battery | N/A | Power → 0 at limit timestamp | N/A | Linear |
| EV | N/A | N/A | Returns zero series | Step |
| Heater | N/A | N/A | N/A | Linear |
| BaseLoad | N/A | N/A | N/A | Step |

---

## AssetState::history(timespan: Duration, history: &AssetHistoryBuffer) -> TimeSeries

**Pre-conditions:**
- `timespan > Duration::zero()`

**Post-conditions:**
- `samples` timestamps are strictly ascending.
- All timestamps fall in `[now − timespan, now]`.
- If `timespan > Duration::zero()` AND buffer is non-empty: first sample timestamp == `now − timespan` (boundary point, FR-010).
- If `timespan > Duration::zero()` AND buffer is empty: `samples` is empty (no error).
- No future-timestamped entries.
- `power_kw` column extracted from ring buffer; NaN rows are dropped.
- `interpolation` matches the asset's `forecast()` interpolation mode (same asset, same physics).

---

## run_planner() — updated signature

```
pub fn run_planner(
    rates:            &[TariffSnapshot],
    packets:          &[EnergyPacket],
    capacity:         &OadrCapacityState,
    profile:          &Profile,
    now:              DateTime<Utc>,
    trigger:          PlanTrigger,
    asset_forecasts:  &HashMap<String, TimeSeries>,   // NEW
) -> Plan
```

**Behavioral guarantee:** If `asset_forecasts` contains `"pv"`, the planner uses it for `pv_forecast_kw` per slot. If absent or empty, behavior falls back to zero PV (same as if no PV is configured). `pv_forecast()` standalone function is removed from `planner.rs`.

---

## Lookup helper (internal, not a public contract)

For the planner to read a forecast value at a specific slot timestamp (until RF-05 provides `resample()`):

```
fn nearest_value(series: &TimeSeries, ts: DateTime<Utc>) -> f64
```

- Finds the sample with the closest timestamp to `ts`.
- If `series.samples` is empty: returns `0.0`.
- For Step series: returns the value of the last sample at or before `ts`; falls back to first sample if `ts` precedes all samples.
- For Linear series: returns nearest-neighbour (simple, correct enough without RF-05).
