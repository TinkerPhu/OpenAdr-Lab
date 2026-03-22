# Phase A: Asset config/state split + step()/capability() + per-asset history buffer

## Context

The VEN planner needs to simulate asset state forward in time (lookahead, plan building,
timeline) without side effects. Currently `update()` mutates internal state — it cannot
be called for projection. The history buffer lives in a central `ControllerTrace`, requiring
deep clones on every HTTP request and reporter tick.

Phase A addresses both, following the interface spec exactly:
- Split each asset struct into **config** (physics params, immutable) and **state** (SoC,
  temperature, etc., changes each tick)
- Add `step(config, &state, setpoint, dt) → (new_state, power_kw)` — pure function
- Add `capability(config, &state) → AssetCapability` — dynamic, SoC-dependent
- Move `AssetHistoryBuffer` from central `ControllerTrace` into each `AssetEntry`

---

## The config / state split

Each asset struct is split into two. Fields that never change at runtime → config.
Fields that change every tick → state.

### Battery

**Config** (`Battery` struct — keeps existing name):
```rust
pub struct Battery {
    pub capacity_kwh:          f64,
    pub max_charge_kw:         f64,
    pub max_discharge_kw:      f64,
    pub round_trip_efficiency: f64,
    pub min_soc:               f64,
}
```

**State** (`BatteryState` — new struct in `assets/mod.rs`):
```rust
pub struct BatteryState {
    pub soc_pct:          f64,   // [0.0, 1.0]
    pub actual_power_kw:  f64,   // positive = charging
}
```

### EvCharger

**Config** (keeps existing name):
```rust
pub struct EvCharger {
    pub max_charge_kw:    f64,
    pub max_discharge_kw: f64,
    pub battery_kwh:      f64,
    pub soc_target:       f64,
    pub default_charge_kw: f64,
}
```

**State** (`EvState`):
```rust
pub struct EvState {
    pub soc_pct:          f64,
    pub plugged:          bool,
    pub actual_power_kw:  f64,
}
```

### Heater

**Config** (keeps existing name):
```rust
pub struct Heater {
    pub max_kw:               f64,
    pub temp_min_c:           f64,
    pub temp_max_c:           f64,
    pub thermal_mass_kwh_per_c: f64,   // hardcoded 2.0 today → config field
}
```

**State** (`HeaterState`):
```rust
pub struct HeaterState {
    pub temperature_c:    f64,
    pub actual_power_kw:  f64,
    pub ambient_temp_c:   f64,   // set by sim before each tick (from UserOverrides)
}
```

`ambient_temp_c` moves into state so `step()` has no hidden env parameter.

### PvInverter

**Config** (keeps existing name):
```rust
pub struct PvInverter {
    pub rated_kw:          f64,
    pub export_limit_kw:   Option<f64>,
}
```

**State** (`PvState`):
```rust
pub struct PvState {
    pub actual_power_kw:  f64,   // always ≤ 0 (export)
    pub irradiance:       f64,   // 0.0–1.0; set by sim before each tick
}
```

`irradiance` moves into state so `step()` needs no time/env parameter.

### BaseLoad

**Config** (keeps existing name):
```rust
pub struct BaseLoad {
    pub baseline_kw: f64,
}
```

**State** (`BaseLoadState`):
```rust
pub struct BaseLoadState {
    pub actual_power_kw: f64,   // always = baseline_kw
}
```

---

## New enums in `assets/mod.rs`

### AssetConfig — replaces current AssetState for config dispatch

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "asset_type", rename_all = "snake_case")]
pub enum AssetConfig {
    Battery(Battery),
    Ev(EvCharger),
    Heater(Heater),
    Pv(PvInverter),
    BaseLoad(BaseLoad),
}
```

Keeps the same `#[serde(tag = "asset_type")]` as the current `AssetState` enum, so
the `from_profile()` construction logic changes minimally.

### AssetState — new, holds only mutable state

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "asset_type", rename_all = "snake_case")]
pub enum AssetState {
    Battery(BatteryState),
    Ev(EvState),
    Heater(HeaterState),
    Pv(PvState),
    BaseLoad(BaseLoadState),
}

impl AssetState {
    pub fn actual_power_kw(&self) -> f64 { /* match arms */ }
}
```

---

## Methods on AssetConfig

All existing dispatch methods on the current `AssetState` move to `AssetConfig`,
with signatures updated to take `state: &AssetState` where they need dynamic state.

```rust
impl AssetConfig {
    // ── Core new interface ─────────────────────────────────────────────────
    pub fn step(
        &self, state: &AssetState, setpoint_kw: f64, dt: Duration,
    ) -> (AssetState, f64) {
        match (self, state) {
            (AssetConfig::Battery(cfg), AssetState::Battery(s)) =>
                { let (ns, p) = cfg.step(s, setpoint_kw, dt); (AssetState::Battery(ns), p) },
            (AssetConfig::Ev(cfg), AssetState::Ev(s)) => ...,
            // etc. — panic on mismatch (programming error, never happens at runtime)
            _ => unreachable!("config/state type mismatch"),
        }
    }

    pub fn capability(&self, state: &AssetState) -> AssetCapability { /* dispatch */ }

    // ── Existing methods retained ──────────────────────────────────────────
    pub fn default_setpoint(&self, state: &AssetState) -> f64 { ... }
    pub fn state_values(&self, state: &AssetState) -> HashMap<String, f64> { ... }
    pub fn capabilities(&self, asset_id: &str, state: &AssetState) -> AssetCapabilities { ... }
    pub fn control_schema(&self) -> Vec<ControlDescriptor> { ... }
    pub fn reset(&self, state: &mut AssetState, values: HashMap<String, f64>) { ... }
    pub fn update_config(&mut self, values: HashMap<String, f64>) { ... }
    pub fn resolve_request_target(&self, state: &AssetState, ...) -> Option<(f64, f64)> { ... }
    pub fn default_comfort_rates(&self) -> Vec<ComfortRate> { ... }
    pub fn default_completion_policy(&self) -> CompletionPolicy { ... }
    pub fn default_post_deadline_comfort_bid(&self) -> Option<f64> { ... }
    pub fn forecast(&self, state: &AssetState, timespan: Duration) -> TimeSeries { ... }
    pub fn history(&self, timespan: Duration, buffer: &AssetHistoryBuffer) -> TimeSeries { ... }
}
```

`forecast()` and `history()` stay on `AssetConfig` (with `state` param added) until Phase D
replaces them with `simulate_forward()` and per-asset buffer access respectively. They are
kept as-is for now so all existing routes continue working.

---

## step() implementation per asset

### Battery::step

```rust
impl Battery {
    pub fn step(
        &self, state: &BatteryState, setpoint_kw: f64, dt: Duration,
    ) -> (BatteryState, f64) {
        let dt_h = dt.num_milliseconds() as f64 / 3_600_000.0;
        let clamped = setpoint_kw
            .max(-self.max_discharge_kw)
            .min(self.max_charge_kw);
        // SoC ceiling / floor
        let (clamped, actual) = enforce_soc_bounds(self, state, clamped, dt_h);
        let energy_kwh = actual * dt_h * if actual > 0.0 { self.round_trip_efficiency } else { 1.0 };
        let new_soc = (state.soc_pct + energy_kwh / self.capacity_kwh).clamp(0.0, 1.0);
        (BatteryState { soc_pct: new_soc, actual_power_kw: actual }, actual)
    }
}
```

Physics identical to current `Battery::update()` — just refactored to take state in,
return new state out.

### EvCharger::step, Heater::step, PvInverter::step, BaseLoad::step

Same pattern: take `&self` (config) + `&EvState` / `&HeaterState` / etc., run physics,
return new state + actual power. Physics logic moves verbatim from current `update()`
implementations.

---

## capability() per asset

### Battery::capability

```rust
pub fn capability(&self, state: &BatteryState) -> AssetCapability {
    AssetCapability {
        power_min_kw: if state.soc_pct <= self.min_soc { 0.0 }
                      else { -self.max_discharge_kw },
        power_max_kw: if state.soc_pct >= 1.0 { 0.0 }
                      else { self.max_charge_kw },
    }
}
```

### EvCharger::capability

```rust
pub fn capability(&self, state: &EvState) -> AssetCapability {
    if !state.plugged {
        return AssetCapability { power_min_kw: 0.0, power_max_kw: 0.0 };
    }
    AssetCapability {
        power_min_kw: if state.soc_pct <= 0.0 { 0.0 } else { -self.max_discharge_kw },
        power_max_kw: if state.soc_pct >= 1.0 { 0.0 } else { self.max_charge_kw },
    }
}
```

### Heater::capability

```rust
pub fn capability(&self, state: &HeaterState) -> AssetCapability {
    AssetCapability {
        power_min_kw: 0.0,
        power_max_kw: if state.temperature_c >= self.temp_max_c { 0.0 } else { self.max_kw },
    }
}
```

### PvInverter::capability, BaseLoad::capability

Both return `[actual_power_kw, actual_power_kw]` — fixed, uncontrollable.

---

## simulator/mod.rs — AssetEntry

```rust
pub struct AssetEntry {
    pub id:            String,
    pub config:        AssetConfig,       // was `state: AssetState` — config only
    pub state:         AssetState,        // NEW — mutable state only
    pub setpoint_kw:   f64,              // was `setpoint: f64`
    pub last_power_kw: f64,
    pub energy:        EnergyCounter,
    pub history:       AssetHistoryBuffer, // moved from ControllerTrace (Checkpoint 2)
}
```

### SimState::tick() changes

**Before each step** — set env fields from UserOverrides into the state enum:
```rust
// Set irradiance/ambient into state before dispatching step()
match &mut entry.state {
    AssetState::Pv(s)     => s.irradiance     = compute_irradiance(now, &overrides),
    AssetState::Heater(s) => s.ambient_temp_c = overrides.ambient_temp_c.unwrap_or(10.0),
    _ => {}
}
// Apply UserOverride config mutations
match &mut entry.config {
    AssetConfig::Ev(cfg)     => { if let Some(v) = overrides.ev_max_charge_kw { cfg.max_charge_kw = v; } }
    AssetConfig::Heater(cfg) => { if let Some(v) = overrides.heater_max_kw { cfg.max_kw = v; } }
    // etc.
}
// Dispatch physics
let (new_state, actual_kw) = entry.config.step(&entry.state, setpoint, dt);
entry.state    = new_state;
entry.last_power_kw = actual_kw;
```

The old `entry.state.update(dt_s, setpoint, env)` call and all the inline `match &mut
entry.state { AssetState::Ev(ev) => ev.max_charge_kw = ... }` blocks are replaced by
the above pattern.

---

## Persistence note

`sim_state.json` currently serializes `AssetEntry.state: AssetState` (old, config+state).
After the split it serializes both `config: AssetConfig` and `state: AssetState` (new, separate).
**Format changes — delete any existing sim_state.json on Pi4-Server before first deployment.**
The VEN starts fresh with profile defaults; no data is lost (buffers are ephemeral, SoC
recovers from first tick).

---

## Checkpoint 2: Move AssetHistoryBuffer to AssetEntry

Unchanged from previous plan revision. Summarised:

- Add `pub history: AssetHistoryBuffer` to `AssetEntry` (initialized with capacity 3600)
- Add `SimState::push_asset_history_row()` and `SimState::asset_history()` methods
- Remove `asset_history: HashMap<String, AssetHistoryBuffer>` from `ControllerTrace`
- Remove `push_asset_row()` from `state.rs`
- Write history rows into `sim.lock()` in `loops.rs` (instead of via `state.push_asset_row()`)
- Routes read from `ctx.sim.lock().await.asset_history(&id)` (routes already have `ctx.sim`)
- Reporter receives `&SimState` instead of `&HashMap<String, AssetHistoryBuffer>`
- `build_asset_timeline()` receives `&SimState` instead of `&HashMap`

---

## Files changed summary

| File | Checkpoint | Change |
|---|---|---|
| `assets/mod.rs` | 1 | Add `BatteryState`, `EvState`, `HeaterState`, `PvState`, `BaseLoadState`; add `AssetCapability`; replace `AssetState` enum with `AssetConfig` (config dispatch) + new `AssetState` (state only); update all dispatch method signatures to take `state: &AssetState` |
| `assets/battery.rs` | 1 | Remove state fields; add `step()`, `capability()` |
| `assets/ev.rs` | 1 | Remove state fields; add `step()`, `capability()` |
| `assets/heater.rs` | 1 | Remove state fields; add `step()`, `capability()` |
| `assets/pv.rs` | 1 | Remove state fields; add `step()`, `capability()` |
| `assets/base_load.rs` | 1 | Remove state fields; add `step()`, `capability()` |
| `simulator/mod.rs` | 1+2 | `AssetEntry`: rename `state` → `config: AssetConfig`, add `state: AssetState`, rename `setpoint` → `setpoint_kw`, add `history: AssetHistoryBuffer` (C2); update `tick()` override logic and physics dispatch; add `push_asset_history_row()`, `asset_history()` (C2) |
| `controller/trace.rs` | 2 | Remove `asset_history` field + its methods from `ControllerTrace` |
| `state.rs` | 2 | Delete `push_asset_row()` method |
| `loops.rs` | 2 | Write history rows into sim lock; remove `state.push_asset_row()` calls |
| `controller/reporter.rs` | 2 | Accept `&SimState` instead of `&HashMap<String, AssetHistoryBuffer>` |
| `routes/assets.rs` | 2 | Read history from `ctx.sim.lock()` |
| `routes/trace.rs` | 2 | Read history from `ctx.sim.lock()` |
| `controller/timeline.rs` | 2 | Receive `&SimState` instead of `&HashMap` |

---

## One prompt or two?

**One prompt.** Both checkpoints are sequential steps in the same session:
1. Checkpoint 1 — compile + BDD green
2. Checkpoint 2 — compile + BDD green
3. Single commit

The plan is fully specified. No further design decisions required during implementation.
