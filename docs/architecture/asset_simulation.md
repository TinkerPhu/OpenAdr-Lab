# Asset Simulation Reference

Generated: 2026-03-24

This document describes every simulated asset in the VEN simulator — physics model, profile
parameters, API overrides, and external influences.

---

## Asset Overview

| Asset | Type | Simulated | Controllable |
|---|---|---|---|
| Battery | Storage (bidirectional) | ✅ | ✅ dispatcher setpoint |
| EV Charger | Consumption (+ optional V2G) | ✅ | ✅ dispatcher setpoint |
| Heater | Thermal consumption | ✅ | ✅ dispatcher setpoint |
| PV Inverter | Production | ✅ | ❌ non-curtailable |
| Base Load | Fixed consumption | ✅ | ❌ fixed |
| Grid | Virtual (derived) | ✅ (derived) | ❌ VTN-driven limits |
| HeatPump | Consumption | ❌ entity only | — |
| WashingMachine | Consumption | ❌ entity only | — |
| CookingStove | Consumption | ❌ entity only | — |
| SiteResidual | Virtual | ❌ entity only | — |
| GenericConsumer | Consumption | ❌ entity only | — |
| GenericProducer | Production | ❌ entity only | — |

**Sign convention**: positive power = import from grid, negative = export to grid.

> **Note on control path**: There is no reactor. The old reactor FSM was removed in Phase 24
> (Simulator Reform). The sole control path is: VTN events → Planner (periodic, produces a Plan
> with FIRM/FLEXIBLE slots) → Dispatcher (per-tick, reads current plan slot → per-asset setpoints)
> → Simulator tick. The dispatcher is stateless and intentionally dumb; all scheduling intelligence
> lives in the planner.

---

## 1. Battery

**Source**: `VEN/src/simulator/assets/battery.rs`

### Physics

- Bidirectional: positive setpoint = charge (import), negative = discharge (export).
- Setpoint is clamped to `[−max_discharge_kw, max_charge_kw]`.
- Hard stops: charging halts at `soc = 1.0`; discharging halts at `soc = min_soc`.
- Efficiency applied on charge only (discharge is lossless):
  ```
  Δsoc = (actual_kw × dt_hours × efficiency) / capacity_kwh
  ```

### Profile Parameters (YAML)

| Parameter | Default | Unit | Description |
|---|---|---|---|
| `capacity_kwh` | 10.0 | kWh | Energy capacity |
| `max_charge_kw` | 5.0 | kW | Max charge rate |
| `max_discharge_kw` | 5.0 | kW | Max discharge rate |
| `initial_soc` | 0.5 | [0,1] | Starting state of charge |
| `round_trip_efficiency` | 0.92 | [0,1] | Charge efficiency (discharge = 1.0) |
| `min_soc` | 0.1 | [0,1] | Discharge floor |

### API Overrides (`POST /sim/override`)

| Field | Effect |
|---|---|
| `battery_force_kw` | Bypass dispatcher — force exact power (clamped to [−max_discharge_kw, max_charge_kw]) |

### External Influences

| Source | Influence |
|---|---|
| Dispatcher | Setpoint (kW) from active OpenADR events or planner |
| Planner | May schedule charge/discharge in energy packets |

### Output State

- `soc` — current state of charge [0, 1]
- `capacity_kwh`, `max_charge_kw`, `max_discharge_kw`, `min_soc` — config echo

### Capability (for planner)

- `max_import_kw = 0.0` if `soc ≥ 1.0`, else `max_charge_kw`
- `max_export_kw = 0.0` if `soc ≤ min_soc`, else `max_discharge_kw`

---

## 2. EV Charger

**Source**: `VEN/src/simulator/assets/ev.rs`

### Physics

- Unidirectional by default (V2G enabled only when `max_discharge_kw > 0`).
- If not plugged: power = 0.0 regardless of setpoint.
- Setpoint clamped to `[−max_discharge_kw, max_charge_kw]`.
- SoC upper bound: stops charging at `soc = 1.0`.
- SoC lower bound: `min_soc = 0.0` (hardcoded; no profile override).
- No efficiency loss on charge (unlike Battery).
- Default power when no dispatcher signal: `default_charge_kw`.

### Hardcoded Constants

| Constant | Value | Notes |
|---|---|---|
| `min_soc` | 0.0 | EV never blocked from discharging by SoC floor |

### Profile Parameters (YAML)

| Parameter | Default | Unit | Description |
|---|---|---|---|
| `max_charge_kw` | 7.4 | kW | Max AC charge rate |
| `max_discharge_kw` | 0.0 | kW | V2G discharge rate (0 = disabled) |
| `initial_soc` | 0.5 | [0,1] | Starting SoC |
| `battery_kwh` | 60.0 | kWh | EV battery capacity |
| `soc_target` | 0.8 | [0,1] | User's desired departure SoC |
| `default_charge_kw` | 0.0 | kW | Idle charge rate when no event active |

### API Overrides (`POST /sim/override`)

| Field | Effect |
|---|---|
| `ev_desired_kw` | Override idle charge rate |
| `ev_plugged` | Toggle plugged/unplugged state |
| `ev_max_charge_kw` | Override max charge rate |
| `ev_soc_target` | Override departure SoC target |

### External Influences

| Source | Influence |
|---|---|
| Dispatcher | Setpoint (kW) from active OpenADR events or planner |
| User override `ev_plugged` | Disconnects EV, zeroing all power |

### Output State

- `soc` — current SoC [0, 1]
- `plugged` — 1.0 / 0.0
- `max_charge_kw`, `soc_target`, `battery_kwh` — config echo

### Capability (for planner)

- If unplugged: `max_import_kw = 0.0, max_export_kw = 0.0`
- If plugged:
  - `max_import_kw = 0.0` if `soc ≥ 1.0`, else `max_charge_kw`
  - `max_export_kw = 0.0` if `soc ≤ 0.0`, else `max_discharge_kw`

---

## 3. Heater

**Source**: `VEN/src/simulator/assets/heater.rs`

### Physics

- Unidirectional (consumption only): power ≥ 0.0.
- Thermal model per tick:
  ```
  loss_kw     = (temp_c − ambient_temp_c) × 0.1
  net_heating = (actual_kw − loss_kw) / thermal_mass_kwh_per_c
  ΔT          = net_heating × dt_hours
  ```
- Thermostat hard overrides (priority over setpoint):
  - `temp_c ≥ temp_max_c` → force off (0.0 kW)
  - `temp_c ≤ temp_min_c` → force on at minimum (0.0 kW minimum)
  - Otherwise → clamp setpoint to `[0.0, max_kw]`

### Hardcoded Constants

| Constant | Value | Unit | Description |
|---|---|---|---|
| `thermal_mass_kwh_per_c` | 2.0 | kWh/°C | Thermal inertia of the space |
| Ambient loss rate | 0.1 | kW/°C | Heat loss per degree above ambient |

### Profile Parameters (YAML)

| Parameter | Default | Unit | Description |
|---|---|---|---|
| `max_kw` | 5.0 | kW | Maximum heating power |
| `temp_initial_c` | 20.0 | °C | Starting room temperature |
| `temp_min_c` | 18.0 | °C | Thermostat lower bound |
| `temp_max_c` | 23.0 | °C | Thermostat upper bound |

### API Overrides (`POST /sim/override`)

| Field | Effect |
|---|---|
| `ambient_temp_c` | Override ambient temperature (default 10.0 °C) |
| `heater_max_kw` | Override max heating power |
| `heater_temp_min_c` | Override thermostat lower bound |
| `heater_temp_max_c` | Override thermostat upper bound |

### External Influences

| Source | Influence |
|---|---|
| Dispatcher | Setpoint (kW) — overridden by thermostat hard limits |
| `ambient_temp_c` override | Changes loss rate; lower ambient → more loss → faster cool-down |

### Output State

- `temp_c` — current room temperature
- `max_kw`, `temp_min_c`, `temp_max_c` — config echo

### Capability (for planner)

- `max_import_kw = 0.0` if `temp_c ≥ temp_max_c` (forced off)
- `max_import_kw = min_power_kw` if `temp_c ≤ temp_min_c` (forced on)
- Otherwise: `max_import_kw = max_kw`
- `max_export_kw = 0.0` always

---

## 4. PV Inverter

**Source**: `VEN/src/simulator/assets/pv.rs`

### Physics

- Non-curtailable: ignores dispatcher setpoint in current phase.
- Power output:
  ```
  power_kw = −(rated_kw × irradiance)   [negative = export]
  ```
- Irradiance auto-model (when not overridden):
  ```
  irradiance = max(0, sin(π × (hour − 6) / 12))   for hour ∈ [6, 18]
  irradiance = 0                                    outside that window
  ```
  Peak irradiance = 1.0 at 12:00.

### Hardcoded Constants

| Constant | Description |
|---|---|
| Solar window | 06:00–18:00 |
| Irradiance model | Sinusoidal, peaks at solar noon |
| `export_limit_kw` | Currently unused (always `None`) |

### Profile Parameters (YAML)

| Parameter | Default | Unit | Description |
|---|---|---|---|
| `rated_kw` | 5.0 | kW | Peak rated output |

### API Overrides (`POST /sim/override`)

| Field | Effect |
|---|---|
| `pv_irradiance` | Override irradiance directly [0.0–1.0]; disables auto model for the tick |
| `pv_rated_kw` | Override rated capacity |

### External Influences

| Source | Influence |
|---|---|
| System clock (hour-of-day) | Automatic irradiance unless overridden |

### Output State

- `irradiance` — current irradiance [0, 1]
- `rated_kw` — rated capacity

### Capability (for planner)

- `is_fixed() = true` (non-curtailable)
- `max_export_kw = max_import_kw = actual_power_kw` (fixed point)

---

## 5. Base Load

**Source**: `VEN/src/simulator/assets/base_load.rs`

### Physics

- Fixed consumption: always returns `baseline_kw` regardless of any input.
- Non-flexible; setpoint is ignored.

### Profile Parameters (YAML)

| Parameter | Default | Unit | Description |
|---|---|---|---|
| `baseline_kw` | 0.5 | kW | Fixed background consumption |

Legacy format: `devices.base_load_w` (watts, auto-converted to kW).

### API Overrides (`POST /sim/override`)

| Field | Effect |
|---|---|
| `base_load_w` | Override baseline load (in watts) |

### External Influences

None. Output is entirely determined by profile/override.

### Capability (for planner)

- `is_fixed() = true`
- `max_export_kw = max_import_kw = actual_power_kw`

---

## 6. Grid (Virtual Asset)

**Source**: `VEN/src/simulator/assets/grid.rs`

### Physics

- Read-only virtual asset — derived from the sum of all other asset powers.
- Updated each tick by the simulator loop after all other assets step:
  ```
  net_power_kw   = Σ(all asset powers)
  import_limit_kw = from active VTN CAPACITY_RESERVATION events (≥ 0)
  export_limit_kw = from active VTN CAPACITY_RESERVATION events (≤ 0)
  ```
- Voltage is randomly sampled in [228, 232] V (cosmetic realism).

### Profile Parameters

None — Grid is not configurable via YAML.

### API Overrides

None directly.

### External Influences

| Source | Influence |
|---|---|
| VTN OpenADR events (`IMPORT_CAPACITY_RESERVATION`, `EXPORT_CAPACITY_RESERVATION`) | Sets import/export limits |
| All other assets | Net power is always derived |

### Default Limits

- `import_limit_kw = f64::MAX` (unlimited) at startup
- `export_limit_kw = −f64::MAX` (unlimited) at startup

### Output State

- `net_power_w` — grid power in watts
- `voltage_v` — sampled voltage [228, 232] V
- `import_kwh`, `export_kwh` — cumulative energy counters
- `import_limit_kw`, `export_limit_kw` — active VTN limits

### Capability (for planner)

- `max_import_kw = import_limit_kw`
- `max_export_kw = export_limit_kw`

### Known Limitation

`simulate_forward()` cannot compute net multi-asset sum because the `Asset` trait only receives
its own state and setpoint. Full predictive multi-asset simulation would require a
`SiteSimulator` abstraction.

---

## Assets Without Simulation

These asset types are defined in `VEN/src/entities/asset.rs` for entity model / reporting
purposes but have no physics simulation in the `assets/` module:

| Asset Type | Entity Enum Variant | Notes |
|---|---|---|
| `HeatPump` | `AssetKind::HeatPump` | No simulator module |
| `WashingMachine` | `AssetKind::WashingMachine` | No simulator module |
| `CookingStove` | `AssetKind::CookingStove` | No simulator module |
| `SiteResidual` | `AssetKind::SiteResidual` | Implicit in base load; no dedicated sim |
| `GenericConsumer` | `AssetKind::GenericConsumer` | Placeholder |
| `GenericProducer` | `AssetKind::GenericProducer` | Placeholder |

---

## Complete Override Reference (`POST /sim/override`)

| Field | Type | Asset | Effect |
|---|---|---|---|
| `pv_irradiance` | f64 [0,1] | PV | Override irradiance; bypasses auto solar model |
| `pv_rated_kw` | f64 | PV | Override rated capacity |
| `ambient_temp_c` | f64 | Heater | Override ambient temperature (default 10.0 °C) |
| `heater_max_kw` | f64 | Heater | Override max heating power |
| `heater_temp_min_c` | f64 | Heater | Override thermostat lower bound |
| `heater_temp_max_c` | f64 | Heater | Override thermostat upper bound |
| `ev_desired_kw` | f64 | EV | Override idle charge rate |
| `ev_plugged` | bool | EV | Toggle plugged/unplugged state |
| `ev_max_charge_kw` | f64 | EV | Override max charge rate |
| `ev_soc_target` | f64 [0,1] | EV | Override departure SoC target |
| `base_load_w` | f64 | Base Load | Override baseline load (watts) |
| `battery_force_kw` | f64 | Battery | Force exact battery power (bypass dispatcher) |

---

## Profile Files

| File | Assets | Use |
|---|---|---|
| `VEN/profiles/test.yaml` | All assets | BDD integration tests |
| `VEN/profiles/ven-1.yaml` | EV + PV + Battery + Base Load | VEN-1 instance |
| `VEN/profiles/ven-2.yaml` | Heater + PV + Base Load | VEN-2 instance |
| `VEN/profiles/ven-3.yaml` | EV + Heater + PV + Base Load | VEN-3 instance |
| `VEN/profiles/policy_test.yaml` | Same as test.yaml + flexibility_policy reserve | Policy BDD tests |
