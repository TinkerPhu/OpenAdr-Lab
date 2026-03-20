# Research: Asset Interface forecast() and history() — RF-01

## Current Codebase Findings

### pv_forecast() call site

**Location**: `VEN/src/controller/planner.rs:563`

```rust
fn pv_forecast(profile: &Profile, ts: DateTime<Utc>) -> f64 { ... }
```

Called at line 138 inside `build_grid()`:
```rust
let pv_kw = pv_forecast(profile, start);  // `start` = slot start timestamp
```

`build_grid()` is called from `run_planner()`, which does NOT currently receive asset state — it only sees `Profile`.

**Impact**: To eliminate `pv_forecast()`, the planner must receive pre-computed forecast series rather than computing PV power itself. The simplest approach: compute all asset forecasts before calling `run_planner()` and pass them in as a map.

---

### predict() stubs — current signatures

`AssetState::predict(setpoint, horizon_s, env)` dispatches to per-asset implementations. All stubs return a single `(Utc::now(), power)` point. The `horizon_s` parameter exists but is unused in every implementation.

The rename `predict → forecast` plus the new `timespan: Duration` parameter replaces this cleanly. The `horizon_s` stub parameter can be dropped since `timespan` carries the same intent with a proper type.

---

### History buffer ownership

`AssetHistoryBuffer` lives in `ControllerTrace.asset_history: HashMap<String, AssetHistoryBuffer>`, which is part of `AppState`. Rows are pushed from `main.rs:449` via `state.push_asset_row(asset_id, now, row)` after each simulator tick.

Assets have **no direct access** to their own history buffer.

**Decision: pass history as parameter to `history()`**

Rather than moving buffer ownership into each `AssetEntry` (which would require restructuring `AppState`, `SimState`, and persistence), `history()` receives the buffer as a parameter:

```rust
history(timespan: Duration, history: &AssetHistoryBuffer) -> AssetSeries
```

The caller (planner, timeline endpoint, reporter) already has access to the buffer via `ControllerTrace`. This satisfies the spec's requirement that the asset *defines* what `history()` returns, without restructuring ownership.

**Rationale**: Lean Architecture (Constitution IV) — moving buffer ownership into `AssetEntry` would require changes to `SimState` persistence, `AppState`, and all callers. Passing a reference achieves the same interface contract with zero structural changes.

---

### History buffer content

`AssetHistoryBuffer` stores sparse columns keyed by string name. The `power_kw` column is written for every asset on every tick via `state_values()`. The `history()` implementation extracts the `power_kw` column for the relevant asset.

Buffer capacity: 3600 rows (~1 hour at 1 Hz tick rate).

---

### Planner access to assets

`run_planner()` signature:
```rust
pub fn run_planner(
    rates: &[TariffSnapshot],
    packets: &[EnergyPacket],
    capacity: &OadrCapacityState,
    profile: &Profile,
    now: DateTime<Utc>,
    trigger: PlanTrigger,
) -> Plan
```

No asset state is passed. The fix: add `asset_forecasts: &HashMap<String, AssetSeries>` parameter. The planner then looks up `"pv"` from this map instead of calling the internal `pv_forecast()` function. Other asset types are future consumers of the same map.

---

### Forecast natural resolution per asset

| Asset | Natural forecast resolution | Interpolation |
|---|---|---|
| PV | Physics step — 1 point per minute is sufficient (irradiation changes slowly) | Linear |
| Battery | Physics step — SOC trajectory at setpoint; 1 point per minute | Linear |
| Heater | Physics step — thermal decay toward setpoint; 1 point per minute | Linear |
| EV | Constant at current setpoint; one point now + boundary | Step |
| BaseLoad | Constant at baseline; one point now + boundary | Step |

---

## Decisions

### D-01: QuantitySeries return type in VEN/src/common/

**Decision**: New module `VEN/src/common/mod.rs` with four types:
- `enum Interpolation { Linear, Step }`
- `enum Quantity { Power, Energy, StateOfCharge, Temperature, Irradiance, Tariff, Co2Intensity }`
- `enum Unit { Kilowatt, KilowattHour, Percent, Celsius, WattsPerSquareMeter, EuroPerKilowattHour, GramsPerKilowattHour }`
- `struct QuantitySeries { samples, quantity, unit, interpolation }`

**Rationale**: `QuantitySeries` is a universal, self-describing time series. Placing it in `common/` (not in `simulator/assets/`) draws the module boundary that RF-05 will build out. A future speckit will introduce `MultiQuantitySeries`. Enum unit is type-safe and avoids string typos. Compatible with future `TimeSeries<T>` from RF-05.

**Alternative considered**: Keeping it in `simulator/assets/mod.rs` alongside `AssetState`. Rejected — `QuantitySeries` has no dependency on asset simulation; it belongs in the shared module. Moving it later would be churn.

---

### D-02: Rename predict() → forecast(), drop horizon_s stub

**Decision**: `AssetState::predict(setpoint, horizon_s, env)` → `AssetState::forecast(timespan: Duration)`. The `horizon_s: f64` stub parameter and `env: &TickEnvironment` are dropped because:
- `timespan: Duration` is typed and expressive; `horizon_s: f64` was a stub placeholder.
- `env` (ambient temperature, etc.) is not used for forward projection — the asset reads its own internal state.

**Alternative considered**: Keep `predict()` and add `forecast()` alongside. Rejected — the stub `predict()` is only called in tests and serves no useful function in its current form. Removing it is cleaner.

---

### D-03: Mandatory boundary point via interpolation

**Decision**: Every non-empty `AssetSeries` includes a point at exactly `now + timespan` (forecast) or `now − timespan` (past). For assets with Step interpolation, this is simply the last known value repeated. For Linear assets, this is a linearly interpolated value if the last sample does not land exactly on the boundary.

**Rationale**: Spec FR-010. Guarantees a definite endpoint for the planner's horizon and for RF-05's resampler.

---

### D-04: Planner receives pre-computed forecast map

**Decision**: `run_planner()` gains a new parameter `asset_forecasts: &HashMap<String, AssetSeries>`. The call site in `main.rs` computes all forecasts from `SimState.assets` before the planner runs.

**Alternative considered**: Pass `&SimState` directly into the planner. Rejected — the planner is a pure function on plan-relevant data. Injecting the full `SimState` would couple it to simulation state unnecessarily.

---

### D-05: Forecast sampling frequency — 1 sample per minute

**Decision**: For continuously varying assets (PV, battery, heater), `forecast()` generates one sample per minute across the requested timespan. This is fine-grained enough for the planner's 5-minute slots (the planner will find the nearest sample; RF-05 will later resample properly).

**Rationale**: 1 sample/minute × 8 hour horizon = 480 samples — negligible allocation. Finer resolution gives RF-05 more accuracy when it resamples.

---

## Test Infrastructure

BDD tests run via `python -m behave` inside Docker (`tests/docker-compose.test.yml`). New feature files go in `tests/features/`. The `--build` flag is always required when VEN Rust source changes.

Cargo unit tests (`cargo test`) supplement BDD for isolated asset physics verification.
