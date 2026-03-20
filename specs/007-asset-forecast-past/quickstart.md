# Quickstart: Implementing RF-01

## What changes and where

| File | Change |
|---|---|
| `VEN/src/common/mod.rs` | **NEW FILE**: `Interpolation`, `Quantity`, `Unit`, `QuantitySeries` — add `mod common;` to `main.rs` |
| `VEN/src/simulator/assets/mod.rs` | Add `forecast()` + `past()` dispatch on `AssetState`; remove `predict()` |
| `VEN/src/simulator/assets/pv.rs` | Replace `predict()` with `forecast(timespan)` — full sinusoidal model |
| `VEN/src/simulator/assets/battery.rs` | Replace `predict()` with `forecast(timespan)` — SOC trajectory |
| `VEN/src/simulator/assets/ev.rs` | Replace `predict()` with `forecast(timespan)` — flat/zero |
| `VEN/src/simulator/assets/heater.rs` | Replace `predict()` with `forecast(timespan)` — thermal decay |
| `VEN/src/simulator/assets/base_load.rs` | Replace `predict()` with `forecast(timespan)` — constant |
| `VEN/src/controller/planner.rs` | Remove `pv_forecast()`; add `asset_forecasts` param to `run_planner()` + `build_grid()` |
| `VEN/src/main.rs` | Compute forecast map from `SimState.assets` before each `run_planner()` call |
| `tests/features/asset_forecast.feature` | New BDD scenarios for forecast() per asset type |
| `tests/features/asset_history.feature` | New BDD scenarios for past() |

---

## Step-by-step

### 1. Create `VEN/src/common/mod.rs` and add `mod common;` to `main.rs`

```rust
pub enum Interpolation { Linear, Step }
pub enum Quantity { Power, Energy, StateOfCharge, Temperature,
                   Irradiance, Tariff, Co2Intensity }
pub enum Unit { Kilowatt, KilowattHour, Percent, Celsius,
                WattsPerSquareMeter, EuroPerKilowattHour, GramsPerKilowattHour }
pub struct QuantitySeries {
    pub samples:       Vec<(chrono::DateTime<chrono::Utc>, f64)>,
    pub quantity:      Quantity,
    pub unit:          Unit,
    pub interpolation: Interpolation,
}
```

Then add `forecast()` and `past()` dispatch arms to `impl AssetState` in `simulator/assets/mod.rs` (remove `predict()`).

### 2. Implement `forecast(timespan)` on each asset

Start with PV — it is the only one the planner currently uses. The other four can follow.

PV implementation sketch:
```
generate samples from now to now+timespan, one per minute
  power = rated_kw × irradiance_at(ts)  // same formula as pv_forecast()
  (sign: negative — generation is export)
append boundary point at exactly now+timespan
return QuantitySeries { samples, interpolation: Linear }
```

### 3. Remove `pv_forecast()` from planner.rs

Add `asset_forecasts: &HashMap<String, QuantitySeries>` to `run_planner()` and `build_grid()`. In `build_grid()`, replace:
```rust
let pv_kw = pv_forecast(profile, start);
```
with:
```rust
let pv_kw = asset_forecasts.get("pv")
    .map(|s| nearest_value(s, start))
    .unwrap_or(0.0);
```
Add `nearest_value()` helper to `planner.rs` (private function, ~10 lines).

### 4. Wire forecast map in `main.rs`

Before each `run_planner()` call, compute:
```rust
let asset_forecasts: HashMap<String, QuantitySeries> = sim_state.assets
    .iter()
    .map(|e| (e.id.clone(), e.state.forecast(planning_horizon)))
    .collect();
```

### 5. Implement `past(timespan, history)` on `AssetState`

Each variant delegates to its struct; the struct implementation slices the buffer and extracts `power_kw`, prepending the boundary point. All variants share the same extraction logic — put it in a helper in `mod.rs`.

### 6. Write BDD feature files first (Constitution II)

Write `tests/features/asset_forecast.feature` and `tests/features/asset_history.feature` BEFORE touching implementation code. Run them and confirm they fail. Then implement.

---

## Running tests

```bash
# Cargo unit tests (fast, no Docker)
cargo test -p ven --lib

# Full BDD suite (Pi4 via SSH)
ssh Pi4-Server "cd /srv/docker/openadr_lab && \
  docker compose -f tests/docker-compose.test.yml run --build --rm test-runner \
  features/asset_forecast.feature features/asset_history.feature"
```

Always pass `--build` — VEN Rust source is baked into the test-runner image at build time.
