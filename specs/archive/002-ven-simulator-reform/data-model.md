# Data Model: VEN Simulator Reform

**Branch**: `002-ven-simulator-reform`
**Date**: 2026-03-15

## Overview

This document describes the data model changes introduced by the generic asset refactor. All types live in `VEN/src/simulator/` unless otherwise noted.

---

## Core Runtime Types

### SimState (`simulator/mod.rs`)

The top-level simulator runtime state, persisted to `sim_state.json`.

| Field | Type | Description |
|---|---|---|
| `assets` | `Vec<AssetEntry>` | Ordered list of all configured assets |
| `grid` | `GridMeter` | Derived grid boundary totals |
| `last_tick` | `Option<DateTime<Utc>>` | Timestamp of last physics tick |

**Replaces**: named fields `ev: Option<EvCharger>`, `heater: Option<Heater>`, `pv: Option<PvInverter>`, `battery: Option<Battery>`, `base_load_w: f64`, `energy: EnergyCounter`, `net_power_w`, `import_w`, `export_w`, `voltage_v`.

---

### AssetEntry (`simulator/mod.rs`)

One asset in the simulator — physics state + metadata.

| Field | Type | Description |
|---|---|---|
| `id` | `String` | Unique identifier (matches profile YAML `id` field) |
| `state` | `AssetState` | Physics state; enum dispatches per-type behavior |
| `setpoint` | `f64` | Last commanded value from dispatcher (kW) |
| `energy` | `EnergyCounter` | Cumulative import/export kWh for this asset |

---

### AssetState (`simulator/assets/mod.rs`)

Discriminated union over all supported asset types.

```
AssetState::Ev(EvCharger)
AssetState::Heater(Heater)
AssetState::Pv(PvInverter)
AssetState::Battery(Battery)
AssetState::BaseLoad(BaseLoad)
```

**Methods** (enum delegation to inner type):
- `update(dt_s: f64, setpoint: f64, env: &TickEnvironment) -> f64` — physics tick, returns actual power_kw
- `predict(setpoint: f64, horizon_s: f64, env: &TickEnvironment) -> Vec<(DateTime<Utc>, f64)>` — forward projection (stub in speckit 1)
- `state_values() -> HashMap<String, f64>` — asset-specific state key-value map
- `default_setpoint() -> f64` — natural operating point when no plan allocation is active
- `capabilities() -> AssetCapabilities` — planning interface descriptor
- `control_schema() -> Vec<ControlDescriptor>` — UI control descriptors
- `reset(values: HashMap<String, f64>)` — writes initial state directly (e.g. SoC)
- `update_config(values: HashMap<String, f64>)` — updates config fields in place

---

### GridMeter (`simulator/mod.rs`)

Derived grid totals — computed after each tick, not ticked as an asset.

| Field | Type | Description |
|---|---|---|
| `net_power_w` | `f64` | Sum of all asset power_w outputs (positive = import) |
| `import_w` | `f64` | max(0, net_power_w) |
| `export_w` | `f64` | max(0, -net_power_w) |
| `import_kwh` | `f64` | Cumulative grid import |
| `export_kwh` | `f64` | Cumulative grid export |
| `voltage_v` | `f64` | 230V ± 2V random variance (unchanged) |

---

### EnergyCounter (`simulator/energy.rs`)

Cumulative energy tracker — used both per-asset and by GridMeter (no changes to existing type).

| Field | Type | Description |
|---|---|---|
| `import_kwh` | `f64` | Cumulative energy imported |
| `export_kwh` | `f64` | Cumulative energy exported |

---

## Asset Configuration Types

### AssetConfig (`profile.rs`)

Deserialized from YAML using `#[serde(tag = "type", rename_all = "snake_case")]`.

```
AssetConfig::Ev(EvConfig)
AssetConfig::Heater(HeaterConfig)
AssetConfig::Pv(PvConfig)
AssetConfig::Battery(BatteryConfig)
AssetConfig::BaseLoad(BaseLoadConfig)
```

**Replaces**: `DeviceConfig` struct with named optional fields.

---

### EvConfig (`simulator/assets/ev.rs`)

| Field | Type | Default | Description |
|---|---|---|---|
| `id` | `String` | — | Asset identifier |
| `max_charge_kw` | `f64` | 7.4 | Maximum charge rate |
| `max_discharge_kw` | `f64` | 0.0 | V2G discharge rate (0 = unidirectional) |
| `battery_kwh` | `f64` | 60.0 | Usable battery capacity |
| `initial_soc` | `f64` | 0.5 | Starting state of charge [0..1] |
| `soc_target` | `f64` | 0.8 | Desired SoC when fully charged |
| `default_charge_kw` | `f64` | 0.0 | Default setpoint (0 = don't charge unless planned) |

`state_values()` keys: `"soc_pct"`, `"plugged"`, `"current_kw"`

---

### HeaterConfig (`simulator/assets/heater.rs`)

| Field | Type | Default | Description |
|---|---|---|---|
| `id` | `String` | — | Asset identifier |
| `max_kw` | `f64` | 5.0 | Maximum heating power |
| `temp_initial_c` | `f64` | 20.0 | Initial room temperature |
| `temp_min_c` | `f64` | 18.0 | Thermostat lower bound |
| `temp_max_c` | `f64` | 23.0 | Thermostat upper bound |
| `default_setpoint_c` | `f64` | 20.0 | Default temperature target (maps to kW via physics) |

`state_values()` keys: `"temp_c"`, `"current_kw"`

---

### PvConfig (`simulator/assets/pv.rs`)

| Field | Type | Default | Description |
|---|---|---|---|
| `id` | `String` | — | Asset identifier |
| `peak_kw` | `f64` | 5.0 | Rated peak output |

`state_values()` keys: `"irradiance"`, `"current_kw"`

`is_flexible: false` — PV is non-flexible; `export_limit_kw` is a dispatcher constraint, not a planner allocation.

---

### BatteryConfig (`simulator/assets/battery.rs`)

| Field | Type | Default | Description |
|---|---|---|---|
| `id` | `String` | — | Asset identifier |
| `max_charge_kw` | `f64` | 5.0 | Maximum charge rate |
| `max_discharge_kw` | `f64` | 5.0 | Maximum discharge rate |
| `capacity_kwh` | `f64` | 10.0 | Usable capacity |
| `initial_soc` | `f64` | 0.5 | Starting state of charge [0..1] |
| `round_trip_efficiency` | `f64` | 0.92 | Charging efficiency factor |
| `min_soc` | `f64` | 0.1 | Minimum allowed SoC |

`state_values()` keys: `"soc_pct"`, `"current_kw"`

---

### BaseLoadConfig (`simulator/assets/base_load.rs`)

| Field | Type | Default | Description |
|---|---|---|---|
| `id` | `String` | — | Asset identifier |
| `baseline_kw` | `f64` | 0.5 | Fixed baseline consumption |

`state_values()` keys: `"current_kw"`

`is_flexible: false`

---

## Planning Interface Types

### AssetCapabilities (`simulator/assets/mod.rs`)

Returned by `AssetState::capabilities()`.

| Field | Type | Description |
|---|---|---|
| `asset_id` | `String` | Asset identifier |
| `max_import_kw` | `f64` | Maximum import power |
| `max_export_kw` | `f64` | Maximum export power (0 for unidirectional) |
| `is_flexible` | `bool` | True if planner may create power allocations |
| `energy_state` | `Option<EnergyState>` | Storage state for flexible assets |
| `availability` | `Option<TimeWindow>` | When asset is available for scheduling |

### EnergyState

| Field | Type | Description |
|---|---|---|
| `current_kwh` | `f64` | Current stored energy |
| `min_kwh` | `f64` | Minimum allowed stored energy |
| `max_kwh` | `f64` | Maximum capacity |

### TimeWindow

| Field | Type | Description |
|---|---|---|
| `start` | `DateTime<Utc>` | Window start |
| `end` | `DateTime<Utc>` | Window end |

---

## Control Schema Types

### ControlDescriptor (`simulator/assets/mod.rs`)

Returned by `AssetState::control_schema()`.

| Field | Type | Description |
|---|---|---|
| `key` | `String` | POST /sim/override body key |
| `label` | `String` | Human-readable parameter name |
| `kind` | `ControlKind` | Input type: Slider | Switch | NumberInput |
| `min` | `Option<f64>` | Minimum value (for Slider/NumberInput) |
| `max` | `Option<f64>` | Maximum value |
| `unit` | `String` | Display unit (e.g. "kW", "°C", "%", "") |

### ControlKind

```
ControlKind::Slider
ControlKind::Switch
ControlKind::NumberInput
```

---

## Snapshot Types (API Response)

### SimSnapshot (`simulator/mod.rs`)

Response body for `GET /sim`.

| Field | Type | Description |
|---|---|---|
| `ts` | `DateTime<Utc>` | Snapshot timestamp |
| `net_power_w` | `f64` | Grid net power (positive = import) |
| `import_w` | `f64` | Grid import power |
| `export_w` | `f64` | Grid export power |
| `import_kwh` | `f64` | Cumulative grid import energy |
| `export_kwh` | `f64` | Cumulative grid export energy |
| `assets` | `HashMap<String, AssetSnapshot>` | Per-asset state keyed by asset id |

**Replaces**: `SimSnapshot` with named fields `ev: Option<EvSnapshot>`, `heater: Option<HeaterSnapshot>`, `pv: Option<PvSnapshot>`, `battery: Option<BatterySnapshot>`, `base_load_w`, etc.

### AssetSnapshot (`simulator/mod.rs`)

One entry in `SimSnapshot.assets`.

| Field | Type | Description |
|---|---|---|
| `power_kw` | `f64` | Current asset power output |
| `values` | `HashMap<String, f64>` | Asset-specific state (from `state_values()`) |

---

## History Buffer Types

### AssetHistoryBuffer (`controller/trace.rs`)

Data structure only — not wired to live data in this speckit.

| Field | Type | Description |
|---|---|---|
| `timestamps` | `VecDeque<DateTime<Utc>>` | Ordered timestamps |
| `columns` | `HashMap<String, VecDeque<f64>>` | One deque per value key; `f64::NAN` for missing |
| `capacity` | `usize` | Maximum entries before oldest are evicted |

**Methods**:
- `push(ts: DateTime<Utc>, values: HashMap<String, f64>)` — append a row; evict oldest if at capacity; insert NAN for missing columns
- `to_timeline(window: Option<(DateTime<Utc>, DateTime<Utc>)>) -> Vec<AssetTimelinePoint>` — row-oriented output

### AssetTimelinePoint

| Field | Type | Description |
|---|---|---|
| `ts` | `DateTime<Utc>` | Row timestamp |
| `values` | `HashMap<String, f64>` | Value map for this row |

---

## Environment Type

### TickEnvironment (`simulator/assets/mod.rs`)

```rust
type TickEnvironment = HashMap<String, f64>;
```

Standard keys populated by the main tick loop:

| Key | Description |
|---|---|
| `"hour_of_day"` | Fractional hour [0.0..24.0) |
| `"ambient_temp_c"` | Ambient temperature; defaults to 8.0 if not overridden |

Each asset reads what it needs and ignores unknown keys.

---

## Profile Loading Changes

### Profile struct (`profile.rs`)

`devices: DeviceConfig` → `assets: Vec<AssetConfig>`

All other profile fields (`reactor`, `simulator`, `planner`, `packets`) are unchanged.

### YAML format migration summary

| Old field | New YAML |
|---|---|
| `devices.ev: { ... }` | `assets: [{ type: ev, id: ev, ... }]` |
| `devices.heater: { ... }` | `assets: [{ type: heater, id: heater, ... }]` |
| `devices.pv: { ... }` | `assets: [{ type: pv, id: pv, ... }]` |
| `devices.battery: { ... }` | `assets: [{ type: battery, id: battery, ... }]` |
| `devices.base_load_w: 500` | `assets: [{ type: base_load, id: base_load, baseline_kw: 0.5 }]` |

Assets not present in the old profile simply have no entry in the new list.

---

## Removed Types

These types are removed as part of the refactor:

- `DeviceConfig` struct (replaced by `Vec<AssetConfig>`)
- `EvSnapshot`, `HeaterSnapshot`, `PvSnapshot`, `BatterySnapshot` (replaced by `AssetSnapshot`)
- Named device fields on `SimState` (`ev`, `heater`, `pv`, `battery`, `base_load_w`)
- `UserOverrides` fields: `ev_initial_soc`, `battery_initial_soc`, `battery_capacity_kwh`
