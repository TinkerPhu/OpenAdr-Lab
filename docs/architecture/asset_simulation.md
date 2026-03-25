# Asset Simulation Reference

Generated: 2026-03-24

This document describes every simulated asset in the VEN simulator — physics model, profile
parameters, API overrides, and external influences.

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
    now: DateTime<Utc>,
) -> HashMap<String, f64>
```

Returns a `HashMap<asset_id, power_kw>`. Algorithm:

1. Start with each asset's `AssetConfig::default_setpoint()` (idle/hold values).
2. Find the FIRM plan slot covering `now` — if found, overwrite the relevant asset entries with their slot allocations.
3. If no FIRM slot, try the FLEXIBLE slot covering `now`.
4. Enforce VTN `export_limit_kw` on the `pv` key if `OadrCapacityState` has one.

**`UserOverrides` are not consulted here.** The dispatcher knows nothing about overrides.

### Step 2 — `SimState::tick()` (`simulator/mod.rs`)

Called immediately after `build_setpoints()` with the resulting map:

```rust
sim_guard.tick(dt_s, sp_map, now, &overrides);
```

Inside `tick()`, for each `(AssetConfig, AssetEntry)` pair:

1. **Override mutations** — env params and device specs from `UserOverrides` are injected into the config *before* stepping:
   - `AssetConfig::Pv` → `pv.irradiance = overrides.pv_irradiance.unwrap_or(auto_model)`
   - `AssetConfig::Heater` → `h.ambient_temp_c = overrides.ambient_temp_c.unwrap_or(10.0)`, plus optional `max_kw`, `temp_min_c`, `temp_max_c`
   - `AssetConfig::Ev` → optional `max_charge_kw`, `soc_target`, `plugged` state
   - `AssetConfig::Pv` → optional `rated_kw`
   - `AssetConfig::BaseLoad` → optional `baseline_kw` (from `base_load_w / 1000`)

2. **Setpoint lookup** — picks the asset's value from the map, falling back to `default_setpoint()` if not present:
   ```rust
   let sp = setpoints.get(&entry.id).copied()
       .unwrap_or_else(|| cfg.default_setpoint(&entry.state));
   ```

3. **Physics step** — calls `AssetConfig::step()`:
   ```rust
   let (new_state, actual_kw) = cfg.step(&entry.state, sp, dt);
   ```

### Step 3 — `AssetConfig::step()` (`assets/mod.rs`)

Enum dispatch — routes to the concrete physics type:

```rust
match self {
    Self::Battery(cfg)  => cfg.step(state, setpoint_kw, dt),
    Self::Ev(cfg)       => cfg.step(state, setpoint_kw, dt),
    Self::Heater(cfg)   => cfg.step(state, setpoint_kw, dt),
    Self::Pv(cfg)       => cfg.step(state, setpoint_kw, dt),
    Self::BaseLoad(cfg) => cfg.step(state, setpoint_kw, dt),
}
```

Each concrete type implements the `Asset` trait's `step()` method. The trait signature:

```rust
fn step(&self, state: &AssetState, setpoint_kw: f64, dt: Duration) -> (AssetState, f64);
```

Returns `(new_state, actual_kw)`. `actual_kw` may differ from `setpoint_kw` because physics
constraints are applied inside `step()` (SoC ceilings, thermostat hard-overrides, etc.).

### API Overrides: What Reaches the Dispatcher vs. the Physics

`POST /sim/override` stores a `UserOverrides` struct. Fields are consumed **only inside
`SimState::tick()`**, not by `build_setpoints()`. The dispatcher sees none of them.

| Override field | Where consumed | Effect |
|---|---|---|
| `pv_irradiance` | `tick()` — sets `pv.irradiance` before `step()` | Changes PV output |
| `pv_rated_kw` | `tick()` — sets `pv.rated_kw` before `step()` | Changes PV peak capacity |
| `ambient_temp_c` | `tick()` — sets `h.ambient_temp_c` before `step()` | Changes heater loss rate |
| `heater_max_kw` | `tick()` — sets `h.max_kw` before `step()` | Changes heater ceiling |
| `heater_temp_min_c` | `tick()` — sets `h.temp_min_c` before `step()` | Changes thermostat lower bound |
| `heater_temp_max_c` | `tick()` — sets `h.temp_max_c` before `step()` | Changes thermostat upper bound |
| `ev_max_charge_kw` | `tick()` — sets `ev.max_charge_kw` before `step()` | Changes EV charge ceiling |
| `ev_soc_target` | `tick()` — sets `ev.soc_target` before `step()` | Changes EV target (used by planner) |
| `ev_plugged` | `tick()` — sets `ev_state.plugged` before `step()` | Disconnects EV (zeroes power) |
| `base_load_w` | `tick()` — sets `bl.baseline_kw = w/1000` before `step()` | Changes fixed consumption |
| `ev_desired_kw` | **Not consumed** — dead field in `UserOverrides` | No effect |

> **Note on `battery_force_kw`**: This key appears in `Battery::control_schema()` (exposed via
> `GET /sim/schema`) and in the frontend `types.ts`, but it is **not a field in `UserOverrides`**
> and is not consumed anywhere in Rust. It is an unimplemented control — sending it via
> `POST /sim/override` is silently ignored.

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

---

## Planned: Override Redesign — Physical Plant State Injection

> **Status**: Design only — not yet implemented. This chapter captures the intended behaviour
> for a future implementation task.

### Problem with current overrides

`POST /sim/override` currently stores a `UserOverrides` struct whose fields are re-applied to
the asset *config* on every sim tick. This is wrong for two reasons:

1. **Config fields are device specifications** (`max_charge_kw`, `rated_kw`, thermostat bounds).
   They describe what the hardware *can* do, not what is currently happening. Mutating them
   from the API blurs the line between "this is a 7.4 kW charger" and "right now the EV is
   half-full", and breaks planner assumptions about device capabilities.

2. **The planner and dispatcher never see the injected condition as real state.** Because
   overrides are applied after `build_setpoints()`, the planner still plans against stale state.
   Injecting the SoC or temperature into the *physics state* is the only way to make the planner
   and dispatcher reason from the correct starting point.

### Correct model: Physical Plant State

The simulation has three distinct layers:

| Layer | Examples | Who owns it |
|---|---|---|
| **Device config** (static) | `max_charge_kw`, `capacity_kwh`, `max_kw`, `temp_min/max_c` | Profile YAML; never changed at runtime |
| **Physical plant state** (dynamic) | `soc`, `temp_c`, `plugged` | Physics engine; evolves every tick |
| **Environmental inputs** (exogenous) | `irradiance`, `ambient_temp_c` | External world; injected, then evolves naturally |

In control theory, plant state + environmental inputs together form the **observable state** that
the controller (planner + dispatcher) reasons from. Overrides should inject into these two layers
only — never into device config.

### Two override behaviours

All overrides share one property: the state jumps immediately to the injected value. What
differs is what happens when the override is **released** (cleared by the user):

| Behaviour | Description | Used for |
|---|---|---|
| **Jump + free evolution** | State jumps once; physics drives it from there. No override field is held between ticks. | SoC, room temperature, plug state |
| **Frozen + exponential return** | State is held at the injected value each tick while override is active. When released, the state blends back to its model value using exponential smoothing. | PV irradiance, base load |
| **Jump + hold** | State jumps immediately and stays at the injected value indefinitely (no autonomous model to return to). | Ambient temperature, EV departure time |

### Exponential smoothing for gradual return

When a frozen override is released, the resulting state follows:

```
s(n+1) = s(n) * (1 - α) + model(n+1) * α
```

where `s` is the current simulation state, `model` is the value the autonomous model would
produce at that tick, and `α` is the smoothing factor (0 < α < 1). This is the standard
**exponential moving average** (also called a first-order IIR low-pass filter). Low α gives a
slow cloud-dispersal feel; high α snaps back quickly. α should be profile-configurable per asset.

### Per-asset override fields (redesigned)

#### Battery — energy state

| Field | Behaviour | Target | Effect |
|---|---|---|---|
| `battery_soc` | Jump + free evolution | `BatteryState.soc_pct` | Jumps SoC; charge/discharge physics continues from there. |

#### EV Charger — energy state + availability state

| Field | Behaviour | Target | Effect |
|---|---|---|---|
| `ev_soc` | Jump + free evolution | `EvState.soc_pct` | Jumps SoC; charge physics continues from there. |
| `ev_plugged` | Jump + hold | `EvState.plugged` | Jumps plug state; zeroes power and prevents planner from scheduling EV energy while false. |
| `ev_departure_min` | Jump + hold | Planning input | Sets minutes until EV must leave with `soc_target`. Raises planner urgency for charging. Stays until overridden or cleared. Currently only reachable via `POST /requests`; a direct inject is useful for demonstrations. |

#### Heater — thermal state + environmental inputs

| Field | Behaviour | Target | Effect |
|---|---|---|---|
| `heater_temp_c` | Jump + free evolution | `HeaterState.temperature_c` | Jumps room temperature; thermal loss and thermostat take over. |
| `ambient_temp_c` | Jump + hold | Environment (`h.ambient_temp_c`) | Jumps ambient temperature; heater loss rate changes immediately. Stays until overridden — no autonomous ambient model yet. |
| `heater_setpoint_c` | Jump + hold | Planning preference | Sets a runtime comfort target between `temp_min` and `temp_max`. Simulates occupancy changes (home/away/eco) without editing profile YAML. The thermostat hard limits still apply. |

#### PV Inverter — environmental input

| Field | Behaviour | Target | Effect |
|---|---|---|---|
| `pv_irradiance` | Frozen + exponential return | Environment (`pv.irradiance`) | Holds irradiance at the injected value while active. When released, blends back to the sinusoidal model via exponential smoothing with factor α (`pv_irradiance_alpha`, default ~0.05 per tick at 1 s = ~5 min recovery). Models cloud cover that passes gradually. |

#### Base Load — power input

| Field | Behaviour | Target | Effect |
|---|---|---|---|
| `base_load_kw` | Frozen + snap return | `BaseLoad.baseline_kw` | Holds base load at the injected value while active. When released, snaps back to the profile default immediately. Models discrete appliance events (kettle, tumble dryer, EV charger) that switch on/off abruptly. No gradual return — step changes are natural for loads. |

#### Grid — capacity inputs

| Field | Behaviour | Target | Effect |
|---|---|---|---|
| `grid_import_limit_kw` | Jump + hold | `OadrCapacityState.import_limit_kw` | Injects an import capacity limit without needing a live VTN event. Useful for testing the dispatcher's capacity enforcement path. Stays until overridden or cleared; a real VTN event takes precedence if one arrives. |
| `grid_export_limit_kw` | Jump + hold | `OadrCapacityState.export_limit_kw` | Same, for export limit. |

### What the planner sees after injection

Because plant state feeds directly into `AssetConfig::capability()`, which the planner calls
during `run_planner()`, the planner immediately reasons from the injected state on the next
replan cycle. No special planner changes are needed — the state change propagates automatically:

```
Inject battery soc=0.1
    → BatteryState.soc_pct = 0.1
    → Battery::capability_inner() returns max_export_kw = 0.0 (below min_soc)
    → Planner sees battery can only charge, not discharge
    → Plan is revised: discharge slots removed, charge slots added
```

Similarly for EV (unplugged → planner drops all EV charge packets), heater (cold room → planner
schedules early heating), PV (cloud → planner reduces expected export, may add battery discharge).

### API shape (proposed)

A single endpoint replaces the current `POST /sim/override`. All fields are optional; only
provided fields are applied. Setting a field activates the override; sending `null` for a field
releases it (triggering exponential return or snap-back as appropriate).

**`POST /sim/inject`**

```json
{
  "battery_soc": 0.1,
  "ev_soc": 0.4,
  "ev_plugged": false,
  "ev_departure_min": 120,
  "heater_temp_c": 16.5,
  "heater_setpoint_c": 19.0,
  "ambient_temp_c": 2.0,
  "pv_irradiance": 0.0,
  "pv_irradiance_alpha": 0.05,
  "base_load_kw": 3.5,
  "grid_import_limit_kw": 5.0,
  "grid_export_limit_kw": 3.0
}
```

To release a specific override (e.g. stop freezing PV irradiance):

```json
{ "pv_irradiance": null }
```

The existing `POST /sim/override` and its `UserOverrides` struct (which mutates config per-tick)
should be removed. Config-level fields (`heater_max_kw`, `ev_max_charge_kw`, etc.) are dropped —
device specs belong in the profile YAML, not the runtime API.

### Summary

| Field | Behaviour | Evolution when released |
|---|---|---|
| `battery_soc` | Jump + free evolution | Charge/discharge physics from injected value |
| `ev_soc` | Jump + free evolution | Charge physics from injected value |
| `ev_plugged` | Jump + hold | Stays; no autonomous plug model |
| `ev_departure_min` | Jump + hold | Stays until cleared or overridden |
| `heater_temp_c` | Jump + free evolution | Thermal loss + thermostat from injected value |
| `heater_setpoint_c` | Jump + hold | Stays; no autonomous occupancy model |
| `ambient_temp_c` | Jump + hold | Stays; no autonomous ambient model |
| `pv_irradiance` | Frozen + exponential return | Blends back to sinusoidal model at rate α per tick |
| `pv_irradiance_alpha` | Parameter for above | — |
| `base_load_kw` | Frozen + snap return | Snaps back to profile default immediately |
| `grid_import_limit_kw` | Jump + hold | Stays; real VTN event takes precedence |
| `grid_export_limit_kw` | Jump + hold | Stays; real VTN event takes precedence |
