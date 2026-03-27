# Asset Simulation Reference

Updated: 2026-03-27

This document describes every simulated asset in the VEN simulator — physics model, profile
parameters, inject overrides, and external influences.

---

## Dispatcher → Asset Call Chain

This section traces exactly how a setpoint flows from the planner through to an asset's physics.

### Step 1 — `dispatcher::build_setpoints()` (`controller/dispatcher.rs`)

Called every sim tick from `loops::spawn_sim_tick()`. Signature:

```rust
pub fn build_setpoints(
    plan: &Plan,
    assets: &[AssetEntry],
    asset_configs: &[AssetConfig],
    capacity: &OadrCapacityState,
    heater_setpoint_c: Option<f64>,
    now: DateTime<Utc>,
) -> HashMap<String, f64>
```

Returns a `HashMap<asset_id, power_kw>`. Algorithm:

1. Start with each asset's `AssetConfig::default_setpoint()` (idle/hold values).
2. Find the FIRM plan slot covering `now` — if found, overwrite the relevant asset entries with
   their slot allocations.
3. If no FIRM slot, try the FLEXIBLE slot covering `now`.
4. If `heater_setpoint_c` inject is active and the plan has no heater allocation, compute an
   ON/OFF setpoint based on current temperature vs. the target.
5. Enforce `export_limit_kw` on the `pv` key if `OadrCapacityState` has one.

`SimInjectState` fields are **not consulted here**. The dispatcher knows nothing about injects
other than `heater_setpoint_c` which is passed explicitly.

### Step 2 — Behaviour A injects (`loops::spawn_sim_tick`)

Before calling `tick()`, the sim loop applies one-shot Behaviour A injects from `SimInjectState`:

- `battery_soc`, `ev_soc`, `heater_temp_c` — applied via `sim.find_asset_mut()` + `cfg.reset()`,
  then `state.clear_inject_field()` clears each field immediately so it fires only once.

### Step 3 — `SimState::tick()` (`simulator/mod.rs`)

Called immediately after setpoints are built. Signature:

```rust
pub fn tick(
    &mut self,
    dt_s: f64,
    setpoints: HashMap<String, f64>,
    now: DateTime<Utc>,
    pv_irradiance_override: Option<f64>,
    pv_alpha: f64,
    ambient_temp_c_override: Option<f64>,
    heater_temp_min_override: Option<f64>,
    heater_temp_max_override: Option<f64>,
    base_load_kw_override: Option<f64>,
    ev_plugged_override: Option<bool>,
    ev_soc_target_override: Option<f64>,
)
```

Inside `tick()`, for each `(AssetConfig, AssetEntry)` pair:

1. **PV irradiance** — compute `irradiance` via Behaviour B EMA smoothing (see PV section), then
   set `pv.irradiance = irradiance`.
2. **Behaviour C env/state injections** — applied per asset type:
   - `Heater` → `h.ambient_temp_c = ambient_temp_c_override.unwrap_or(10.0)`;
     `h.temp_min_c = heater_temp_min_override.unwrap_or(h.temp_min_c_profile)`;
     `h.temp_max_c = heater_temp_max_override.unwrap_or(h.temp_max_c_profile)`
   - `BaseLoad` → `bl.baseline_kw = base_load_kw_override.unwrap_or(bl.baseline_kw_profile)`
   - `Ev` → `s.plugged = ev_plugged_override.unwrap_or(true)` (snaps back to plugged on release);
     `ev.soc_target = ev_soc_target_override.unwrap_or(ev.soc_target_profile)`
3. **Setpoint lookup** — picks the asset's value from the map, falling back to
   `default_setpoint()` if absent.
4. **Physics step** — calls `AssetConfig::step()`:
   ```rust
   let (new_state, actual_kw) = cfg.step(&entry.state, sp, dt);
   ```

### Step 4 — `AssetConfig::step()` (`assets/mod.rs`)

Enum dispatch to the concrete physics type. Trait signature:

```rust
fn step(&self, state: &AssetState, setpoint_kw: f64, dt: Duration) -> (AssetState, f64);
```

Returns `(new_state, actual_kw)`. `actual_kw` may differ from `setpoint_kw` because physics
constraints are applied inside `step()` (SoC ceilings, thermostat hard-stops, etc.).

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

**Control path**: VTN events → Planner (periodic 20 s, produces a Plan with FIRM/FLEXIBLE slots)
→ Dispatcher (per-tick, reads current plan slot → per-asset setpoints) → Simulator tick.
The dispatcher is stateless; all scheduling intelligence lives in the planner.

---

## 1. Battery

**Source**: `VEN/src/simulator/assets/battery.rs`

### Physics

- Bidirectional: positive setpoint = charge (import), negative = discharge (export).
- Setpoint is clamped to `[−max_discharge_kw, max_charge_kw]`.
- Hard stops: charging halts at `soc = 1.0`; discharging halts at `soc = min_soc`.
- Efficiency applied on charge only (discharge is lossless):
  ```
  Δsoc = (actual_kw × dt_hours × round_trip_efficiency) / capacity_kwh
  ```

### Profile Parameters (YAML)

| Parameter | Default | Unit | Description |
|---|---|---|---|
| `capacity_kwh` | 10.0 | kWh | Energy capacity |
| `max_charge_kw` | 5.0 | kW | Max charge rate |
| `max_discharge_kw` | 5.0 | kW | Max discharge rate |
| `initial_soc` | 0.5 | [0,1] | Starting state of charge |
| `round_trip_efficiency` | 0.92 | [0,1] | Charge efficiency (discharge = 1.0) |
| `min_soc` | 0.10 | [0,1] | Discharge floor |

### Inject Overrides (`POST /sim/inject`)

| Field | Behaviour | Effect |
|---|---|---|
| `battery_soc` | A — one-shot | Jump SoC to value; cleared after next tick; charge/discharge continues from there |

No Behaviour C fields. Battery scheduling is fully planner-driven.

### External Influences

| Source | Influence |
|---|---|
| Dispatcher | Setpoint (kW) from active plan slot |
| Planner (battery arbitrage) | Charge when tariff cheap, discharge when expensive |

### Output State

- `soc_pct` — current state of charge [0, 1]

### Capability (for planner)

- `max_import_kw = 0.0` if `soc_pct ≥ 1.0`, else `max_charge_kw`
- `max_export_kw = 0.0` if `soc_pct ≤ min_soc`, else `−max_discharge_kw`

---

## 2. EV Charger

**Source**: `VEN/src/simulator/assets/ev.rs`

### Physics

- Unidirectional by default (V2G enabled only when `max_discharge_kw > 0`).
- If not plugged: power = 0.0 regardless of setpoint.
- Setpoint clamped to `[−max_discharge_kw, max_charge_kw]`.
- Charging halts when `soc_pct ≥ soc_target` (user's preferred ceiling, default 0.8).
- Discharging halts when `soc_pct ≤ min_soc` (0.0 by default — V2G floor unset unless configured).
- No efficiency loss (unlike Battery).
- Default power when no dispatcher signal: `default_charge_kw`.

### Profile Parameters (YAML)

| Parameter | Default | Unit | Description |
|---|---|---|---|
| `max_charge_kw` | 7.4 | kW | Max AC charge rate |
| `max_discharge_kw` | 0.0 | kW | V2G discharge rate (0 = disabled) |
| `initial_soc` | 0.5 | [0,1] | Starting SoC |
| `battery_kwh` | 60.0 | kWh | EV battery capacity |
| `soc_target` | 0.8 | [0,1] | User's desired charge ceiling |
| `default_charge_kw` | 0.0 | kW | Idle charge rate when no plan active |

### Inject Overrides (`POST /sim/inject`)

| Field | Behaviour | Effect |
|---|---|---|
| `ev_soc` | A — one-shot | Jump SoC to value; cleared after next tick |
| `ev_plugged` | C — frozen + snap | Hold plugged/unplugged state while active; snaps back to `true` on release |
| `ev_soc_target` | C — frozen + snap | Override BMS charge ceiling; snaps to `soc_target_profile` on release |
| `ev_departure_min` | C — frozen | Minutes until EV must depart; replaces active EV packet tier deadline in planner |

### External Influences

| Source | Influence |
|---|---|
| Dispatcher | Setpoint (kW) from active plan slot |
| `ev_plugged` inject | Disconnects EV — zeroes power, capability drops to 0 |
| `ev_soc_target` inject | Lowers BMS ceiling; planner also uses it to size energy packets |

### Output State

- `soc_pct` — current SoC [0, 1]
- `plugged` — current plug state (bool)
- `actual_power_kw` — last tick power

### Capability (for planner)

- If unplugged: `max_import_kw = 0.0, max_export_kw = 0.0`
- If plugged:
  - `max_import_kw = 0.0` if `soc_pct ≥ 1.0`, else `max_charge_kw`
  - `max_export_kw = 0.0` if `soc_pct ≤ min_soc`, else `−max_discharge_kw`

> Note: capability uses `soc_pct ≥ 1.0` as the ceiling, but physics halts at `soc_target`.
> The planner schedules charging until the battery is full; the physics enforces the user's ceiling.

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
- Thermostat hard overrides (priority over dispatcher setpoint):
  - `temp_c ≥ temp_max_c` → force off (0.0 kW)
  - `temp_c ≤ temp_min_c` → force on at `min_power_kw`
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

### Inject Overrides (`POST /sim/inject`)

| Field | Behaviour | Effect |
|---|---|---|
| `heater_temp_c` | A — one-shot | Jump room temperature; cleared after next tick; thermal model continues from there |
| `heater_setpoint_c` | C — frozen | Comfort target passed to dispatcher: ON if `temp_c < target`, OFF otherwise; no snap-back model |
| `heater_temp_min_c` | C — frozen + snap | Override thermostat lower bound; snaps to `temp_min_c_profile` on release |
| `heater_temp_max_c` | C — frozen + snap | Override thermostat upper bound; snaps to `temp_max_c_profile` on release |
| `ambient_temp_c` | C — frozen | Override outdoor temperature (default 10.0 °C); no snap-back model |

### External Influences

| Source | Influence |
|---|---|
| Dispatcher | Setpoint (kW) overridden by thermostat hard limits |
| `ambient_temp_c` inject | Changes loss rate: lower ambient → more loss → faster cool-down |

### Output State

- `temperature_c` — current room temperature
- `actual_power_kw` — last tick power

### Capability (for planner)

- `max_import_kw = 0.0` if `temp_c ≥ temp_max_c` (forced off)
- `max_import_kw = min_power_kw` if `temp_c ≤ temp_min_c` (forced on)
- Otherwise: `max_import_kw = max_kw`
- `max_export_kw = 0.0` always

---

## 4. PV Inverter

**Source**: `VEN/src/simulator/assets/pv.rs`

### Physics

- Non-curtailable: ignores dispatcher setpoint.
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

### Behaviour B — EMA smoothing (`simulator/mod.rs`)

`pv_irradiance` uses Behaviour B (frozen + exponential blend-back). The simulator tracks a
`PvSmoothingState { current_irradiance, override_was_active }`:

- **While override active**: `irradiance = override_value` each tick.
- **On release** (`pv_irradiance = null`): EMA blend-back activates:
  ```
  current = current * (1 − α) + natural_model * α
  ```
  Converges when `|current − natural| < 0.005`. Only activates when `override_was_active` was
  true — prevents ramp-up lag at VEN startup.
- **Normal operation** (no override ever set): uses `natural_model` directly.

### Profile Parameters (YAML)

| Parameter | Default | Unit | Description |
|---|---|---|---|
| `rated_kw` | 5.0 | kW | Peak rated output |

### Inject Overrides (`POST /sim/inject`)

| Field | Behaviour | Effect |
|---|---|---|
| `pv_irradiance` | B — frozen + EMA return | Freeze irradiance [0–1]; EMA blend-back to sinusoidal model on release |
| `pv_irradiance_alpha` | Parameter | EMA blend-back speed (default 0.1); higher = faster snap-back |

### External Influences

| Source | Influence |
|---|---|
| System clock (hour-of-day) | Automatic irradiance unless overridden |
| `export_limit_kw` (OadrCapacityState) | Curtails PV output if VTN sets a limit |

### Output State

- `actual_power_kw` — current output (≤ 0, export)

### Capability (for planner)

- Fixed asset: `max_export_kw = max_import_kw = actual_power_kw`

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

### Inject Overrides (`POST /sim/inject`)

| Field | Behaviour | Effect |
|---|---|---|
| `base_load_kw` | C — frozen + snap | Override baseline power (kW); snaps to `baseline_kw_profile` on release |

### External Influences

None. Output is entirely determined by profile / active inject.

### Capability (for planner)

- Fixed asset: `max_export_kw = max_import_kw = actual_power_kw`

---

## 6. Grid (Virtual Asset)

**Source**: `VEN/src/simulator/assets/grid.rs` / `VEN/src/entities/capacity.rs`

### Physics

- Read-only virtual asset — derived from the sum of all other asset powers each tick.
- Voltage is randomly sampled in [228, 232] V (cosmetic realism).

### External Influences

| Source | Influence |
|---|---|
| All other assets | `net_power_kw = Σ(all asset powers)` |
| VTN `IMPORT_CAPACITY_LIMIT` event | Sets `import_limit_kw` in `OadrCapacityState` |
| VTN `EXPORT_CAPACITY_LIMIT` event | Sets `export_limit_kw` in `OadrCapacityState` |
| `grid_import_limit_kw` inject | Overrides import limit when no VTN event is active |
| `grid_export_limit_kw` inject | Overrides export limit when no VTN event is active |

**Grid limit priority**: VTN event always wins. Inject only applies when
`capacity_snap.import_limit_event_id.is_none()`.

### Default Limits

- `import_limit_kw = f64::MAX` (unlimited) at startup
- `export_limit_kw = −f64::MAX` (unlimited) at startup

### Output State

- `net_power_kw` — grid power (positive = import, negative = export)
- `import_kwh`, `export_kwh` — cumulative energy counters

---

## Complete Inject Reference (`POST /sim/inject`)

| Field | Type | Behaviour | Asset | Evolution when released |
|---|---|---|---|---|
| `battery_soc` | f64 [0,1] | A | Battery | Physics-driven from injected value |
| `ev_soc` | f64 [0,1] | A | EV | Physics-driven from injected value |
| `heater_temp_c` | f64 | A | Heater | Thermal model from injected value |
| `pv_irradiance` | f64 [0,1] | B | PV | EMA blend-back to sinusoidal model |
| `pv_irradiance_alpha` | f64 | — | PV | EMA coefficient (default 0.1) |
| `ev_plugged` | bool | C | EV | Snaps to `true` (plugged) |
| `ev_departure_min` | f64 | C | EV | No snap-back — stays until cleared |
| `ev_soc_target` | f64 [0,1] | C | EV | Snaps to `soc_target_profile` |
| `heater_setpoint_c` | f64 | C | Heater | No snap-back — stays until cleared |
| `heater_temp_min_c` | f64 | C | Heater | Snaps to `temp_min_c_profile` |
| `heater_temp_max_c` | f64 | C | Heater | Snaps to `temp_max_c_profile` |
| `ambient_temp_c` | f64 | C | Heater | No snap-back — stays until cleared |
| `base_load_kw` | f64 | C | Base Load | Snaps to `baseline_kw_profile` |
| `grid_import_limit_kw` | f64 | C | Grid | No snap-back; VTN event takes precedence |
| `grid_export_limit_kw` | f64 | C | Grid | No snap-back; VTN event takes precedence |

Sending a field as **absent** = no change. Sending **`null`** = release override.
See `docs/architecture/asset_simulation_override_redesign.md` → deleted; full inject API
reference is in `VEN/src/routes/sim.rs` and `docs/architecture/` (now removed — see git history
or `asset_simulation_override_redesign.md` at `fa70a3b~1`).

> **Inject API quick reference**: `GET /sim/inject` — read state. `POST /sim/inject` — partial
> merge. `POST /sim/inject/reset` — release all.

---

## Profile Files

| File | Assets | Use |
|---|---|---|
| `VEN/profiles/test.yaml` | EV + Heater + PV + Battery + Base Load | BDD integration tests |
| `VEN/profiles/ven-1.yaml` | EV + PV + Battery + Base Load | VEN-1 instance (residential prosumer) |
| `VEN/profiles/ven-2.yaml` | Heater + PV + Base Load | VEN-2 instance (commercial building) |
| `VEN/profiles/ven-3.yaml` | EV + Heater + PV + Base Load | VEN-3 instance (full mix) |
| `VEN/profiles/policy_test.yaml` | Same as test.yaml + `flexibility_policy` reserve | Policy BDD tests |

---

## Assets Without Simulation

These asset types are defined in `VEN/src/entities/asset.rs` for entity model / reporting
purposes but have no physics simulation in the `assets/` module:

| Asset Type | Entity Enum Variant |
|---|---|
| `HeatPump` | `AssetKind::HeatPump` |
| `WashingMachine` | `AssetKind::WashingMachine` |
| `CookingStove` | `AssetKind::CookingStove` |
| `SiteResidual` | `AssetKind::SiteResidual` |
| `GenericConsumer` | `AssetKind::GenericConsumer` |
| `GenericProducer` | `AssetKind::GenericProducer` |
