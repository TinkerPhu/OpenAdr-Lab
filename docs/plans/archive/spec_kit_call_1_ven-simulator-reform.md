# Speckit Call 1 — VEN Simulator Reform

**Speckit feature name**: `ven-simulator-reform`
**Depends on**: nothing (first in sequence)
**Must be complete before**: spec_kit_call_2 (controller reform)

## How to invoke

```
/speckit.specify <paste the Feature Description section below>
```

When prompted for a feature path / name, use: `ven-simulator-reform`

---

## Feature Description

Refactor the VEN simulator and profile configuration to use a generic, extensible asset model. This is a **pure Rust backend refactor** — zero behavior change, zero API change, zero UI change. All existing BDD simulator scenarios must pass before and after.

### Context

The simulator currently hardcodes every supported device type as named fields at every layer: `DeviceConfig` in `profile.rs`, `SimState` in `simulator/mod.rs`, four actor types in a single `simulator/actors.rs`, `Setpoints` struct in the reactor, and `UserOverrides` in `state.rs`. Adding a new asset type requires touching all layers simultaneously. Battery and base_load are missing from the reactor trace because the trace struct was never updated.

This refactor establishes the generic foundation that the controller reform (call 2) and timeline/UI work (call 3) depend on.

### Scope

#### 1. Simulator modularisation — split actors into per-type files

Split `VEN/src/simulator/actors.rs` into a module:

```
simulator/
  mod.rs           — SimState (Vec<AssetEntry> + GridMeter), tick(), to_sim_snapshot()
  assets/
    mod.rs         — AssetState enum + dispatch
    ev.rs          — EvCharger, EvConfig
    heater.rs      — Heater, HeaterConfig
    pv.rs          — PvInverter, PvConfig
    battery.rs     — Battery, BatteryConfig
    base_load.rs   — BaseLoad, BaseLoadConfig
  energy.rs        — EnergyCounter (shared utility)
  persist.rs       — save/load SimState (unchanged, move only)
  power_model.rs   — net power computation (unchanged, move only)
```

`SimState` changes from hardcoded named device fields to:
```rust
struct SimState {
    assets: Vec<AssetEntry>,
    grid:   GridMeter,
}

struct AssetEntry {
    id:       String,
    state:    AssetState,     // enum dispatch to per-type physics
    setpoint: f64,            // last commanded value from dispatcher
    energy:   EnergyCounter,  // cumulative kWh for this asset
}

enum AssetState {
    Ev(EvCharger),
    Heater(Heater),
    Pv(PvInverter),
    Battery(Battery),
    BaseLoad(BaseLoad),
}
```

`GridMeter` is separate from `Vec<AssetEntry>` — it is derived post-tick (sum of asset outputs) and must not be ticked as an asset. It has its own fields: `net_power_w`, `import_w`, `export_w`, `import_kwh`, `export_kwh`.

Sign convention throughout: **positive = import/consumption, negative = export/generation**.

#### 2. AssetState interface — methods on the enum

Each variant must implement (via enum delegation to the inner type):
- `update(dt_s: f64, env: &TickEnvironment) -> f64` — one physics tick, returns actual power_kw
- `predict(setpoint: f64, horizon_s: f64, env: &TickEnvironment) -> Vec<(DateTime<Utc>, f64)>` — forward projection using own physics model
- `state_values() -> HashMap<String, f64>` — asset-specific state (e.g. `soc_pct`, `temp_c`, `irradiance`)
- `default_setpoint() -> f64` — natural operating point when no plan allocation is active
- `capabilities() -> AssetCapabilities` — planning interface (see below)
- `control_schema() -> Vec<ControlDescriptor>` — UI control descriptors (see below)

`TickEnvironment` is a generic map passed to all assets:
```rust
type TickEnvironment = HashMap<String, f64>;
// e.g. {"hour_of_day": 13.5, "ambient_temp_c": 8.0}
```

Each asset reads what it needs and ignores the rest.

#### 3. Asset capability interface

Each asset exposes capabilities to the planner:
```rust
struct AssetCapabilities {
    asset_id:      String,
    max_import_kw: f64,
    max_export_kw: f64,       // 0.0 for unidirectional assets
    is_flexible:   bool,      // true = planner may create power allocations
    energy_state:  Option<EnergyState>,
    availability:  Option<TimeWindow>,
}

struct EnergyState {
    current_kwh: f64,
    min_kwh:     f64,
    max_kwh:     f64,
}
```

Flexible assets: EV (`true`), Battery (`true`), Heater (`true`).
Non-flexible: PV (`false`), BaseLoad (`false`).

Non-flexible assets are included in planner net power accounting via their `predict()` output but never receive power allocations.

PV `is_flexible: false` means the planner does not schedule power allocations. The `export_limit_kw` setpoint is a grid compliance constraint enforced by the dispatcher directly — it is not a planning decision.

Heater `predict()` must incorporate the thermal model so the planner receives realistic forecasts. If the planner allocates zero heater power during a cold period, the thermostat will override — the planner must account for this.

#### 4. Idle setpoints — asset-owned defaults

Each asset defines `default_setpoint() -> f64`. The dispatcher uses this when no plan allocation covers the current tick:
```rust
setpoints.entry(asset.id.clone()).or_insert_with(|| asset.state.default_setpoint());
```

Typical defaults: EV → 0.0 (don't charge unless planned), Heater → thermostat setpoint, PV → no limit, Battery → 0.0, BaseLoad → profile `baseline_kw`.

#### 5. SimSnapshot — generic maps

Replace named typed snapshot structs (`EvSnapshot`, `HeaterSnapshot`, etc.) with:
```rust
struct SimSnapshot {
    ts:           DateTime<Utc>,
    net_power_w:  f64,
    import_w:     f64,
    export_w:     f64,
    import_kwh:   f64,
    export_kwh:   f64,
    assets: HashMap<String, AssetSnapshot>,
}

struct AssetSnapshot {
    power_kw: f64,
    values:   HashMap<String, f64>,  // state_values() output
}
```

#### 6. Dynamic UI control schema

Each asset defines its controllable parameters:
```rust
struct ControlDescriptor {
    key:   String,           // parameter name, used as POST /sim/override body key
    label: String,
    kind:  ControlKind,      // Slider | Switch | NumberInput
    min:   Option<f64>,
    max:   Option<f64>,
    unit:  String,
}
```

Exposed via `GET /sim/schema` — returns `HashMap<asset_id, Vec<ControlDescriptor>>`. This endpoint is new but the UI change is out of scope for this speckit.

#### 7. Asset history buffer data structure

Add `AssetHistoryBuffer` to `controller/trace.rs` (the data structure only — writing to it is wired up in speckit 2):

```rust
struct AssetHistoryBuffer {
    timestamps: VecDeque<DateTime<Utc>>,
    columns:    HashMap<String, VecDeque<f64>>,  // one deque per value key
    capacity:   usize,
}
```

Columnar layout: one `HashMap` at buffer level, each column is a contiguous `VecDeque<f64>`. Missing values represented as `f64::NAN`. Row-oriented output via `to_timeline(window) -> Vec<AssetTimelinePoint>`.

```rust
struct AssetTimelinePoint {
    ts:     DateTime<Utc>,
    values: HashMap<String, f64>,
}
```

#### 8. Profile YAML generalisation

Change `ven-N.yaml` from named device fields to a typed list:

```yaml
assets:
  - type: ev
    id: ev
    max_charge_kw: 11.0
    max_discharge_kw: 0.0
    battery_kwh: 60.0
    initial_soc: 0.8
    default_charge_kw: 0.0

  - type: heater
    id: heater
    max_kw: 2.0
    temp_min_c: 18.0
    temp_max_c: 22.0
    default_setpoint_c: 20.0

  - type: pv
    id: pv
    peak_kw: 5.0

  - type: battery
    id: battery
    max_charge_kw: 5.0
    max_discharge_kw: 5.0
    capacity_kwh: 10.0
    initial_soc: 0.5

  - type: base_load
    id: base_load
    baseline_kw: 0.4
```

Rust:
```rust
#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum AssetConfig {
    Ev(EvConfig),
    Heater(HeaterConfig),
    Pv(PvConfig),
    Battery(BatteryConfig),
    BaseLoad(BaseLoadConfig),
}
```

Migrate all four existing profile YAML files: `ven-1.yaml`, `ven-2.yaml`, `ven-3.yaml`, `test.yaml`. Replace `DeviceConfig` in `profile.rs` with `Vec<AssetConfig>`.

### Acceptance criteria

1. All existing BDD simulator and controller scenarios pass unchanged.
2. `GET /sim` returns the new generic `assets: HashMap<String, AssetSnapshot>` format.
3. `GET /sim/schema` returns control descriptors for all configured assets.
4. No named per-device fields remain in `SimState`, `SimSnapshot`, `DeviceConfig`, or `AssetConfig`.
5. Adding a hypothetical new asset type requires: one new file in `simulator/assets/`, one new variant in `AssetState` and `AssetConfig` — nothing else.

#### 9. Simulation initialisation endpoints (stub replacement)

Remove the three stub fields from `UserOverrides` and replace with proper endpoints:

| Stub removed | Replacement endpoint |
|---|---|
| `ev_initial_soc: Option<f64>` | `POST /sim/reset/ev` with `{ "soc": 0.8 }` |
| `battery_initial_soc: Option<f64>` | `POST /sim/reset/battery` with `{ "soc": 0.5 }` |
| `battery_capacity_kwh: Option<f64>` | `PUT /sim/config/battery` with `{ "capacity_kwh": 20.0 }` |

Both endpoints dispatch to a method on `AssetState`:
- `reset(values: HashMap<String, f64>)` — writes initial state (e.g. SoC) directly into the asset physics state
- `update_config(values: HashMap<String, f64>)` — updates the asset's config struct in place (e.g. capacity)

Persist `sim_state.json` after each call.

### Files NOT in scope

- `reactor/` — touched in speckit 2
- `controller/dispatcher.rs`, `controller/monitor.rs` — touched in speckit 2
- `UserOverrides` force fields — removed in speckit 2
- Any UI component or API timeline endpoint — speckit 3
