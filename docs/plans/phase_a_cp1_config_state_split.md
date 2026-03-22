# Phase A – Checkpoint 1: Config/state split · step() · capability() · Asset trait

## Context

The VEN planner needs to simulate asset state forward in time without side effects.
Currently `update()` mutates internal state — it cannot be called for projection.

Checkpoint 1 addresses the full config/state split and introduces the pure physics
interface (`step()`, `capability()`). It does **not** wire per-asset history writes
(that is Checkpoint 2).

---

## Rename: `profile::AssetConfig` → `AssetProfile`

`profile.rs` has an `AssetConfig` enum used for YAML loading. The new
`assets::AssetConfig` enum (runtime physics dispatch) would collide with it.
Rename the profile type to `AssetProfile` everywhere before introducing the new one.

Files affected: `profile.rs` and all `use crate::profile::AssetConfig` imports
(currently only `simulator/mod.rs`).

---

## Per-asset config/state splits

Each asset struct becomes two. The config struct keeps the existing name.
The state struct is new.

### Battery

**Config** (`Battery` — keeps name, loses state fields):
```rust
pub struct Battery {
    pub capacity_kwh:          f64,
    pub max_charge_kw:         f64,
    pub max_discharge_kw:      f64,
    pub round_trip_efficiency: f64,
    pub min_soc:               f64,
}
```

**State** (`BatteryState`):
```rust
pub struct BatteryState {
    pub soc_pct:         f64,   // [0.0, 1.0]
    pub actual_power_kw: f64,   // positive = charging (import)
}
```

### EvCharger

**Config** (`EvCharger` — keeps name, loses `soc`, `plugged`, `current_kw`;
gains `min_soc`):
```rust
pub struct EvCharger {
    pub max_charge_kw:     f64,
    pub max_discharge_kw:  f64,
    pub battery_kwh:       f64,
    pub soc_target:        f64,
    pub default_charge_kw: f64,
    pub min_soc:           f64,   // V2G floor; 0.0 if not specified in profile
}
```

**State** (`EvState`):
```rust
pub struct EvState {
    pub soc_pct:         f64,   // [0.0, 1.0]; was `soc` in old struct
    pub plugged:         bool,
    pub actual_power_kw: f64,   // positive = charging; was `current_kw`
}
```

### Heater

**Config** (`Heater` — keeps name, loses `temp_c`; adds `thermal_mass_kwh_per_c`,
`ambient_temp_c`, `min_power_kw`):
```rust
pub struct Heater {
    pub max_kw:                f64,
    pub min_power_kw:          f64,   // forced-on floor at temp_min_c (0.0 if none)
    pub temp_min_c:            f64,
    pub temp_max_c:            f64,
    pub thermal_mass_kwh_per_c: f64,  // hardcoded 2.0 today → explicit config field
    pub ambient_temp_c:        f64,   // set each tick by sim from UserOverrides; not from YAML
}
```

`ambient_temp_c` is NOT in `HeaterState` (spec contract). It belongs in config
because `step()` reads it via `self.ambient_temp_c`. The sim tick loop sets it from
`UserOverrides.ambient_temp_c` each tick before calling `step()`.

**State** (`HeaterState` — per spec):
```rust
pub struct HeaterState {
    pub temperature_c:   f64,
    pub actual_power_kw: f64,   // always ≥ 0
}
```

### PvInverter

**Config** (`PvInverter` — keeps name; adds `irradiance`):
```rust
pub struct PvInverter {
    pub rated_kw:        f64,
    pub export_limit_kw: Option<f64>,   // ≤ 0; None = no curtailment limit
    pub irradiance:      f64,           // [0.0, 1.0]; set each tick by sim; not from YAML
}
```

`irradiance` is NOT in `PvState` (spec contract). It belongs in config because
`step()` reads it via `self.irradiance`. The sim tick loop sets it (from
`UserOverrides.pv_irradiance` or time-based model) before calling `step()`.

**State** (`PvState` — per spec):
```rust
pub struct PvState {
    pub actual_power_kw: f64,   // always ≤ 0 (export)
}
```

### BaseLoad

**Config** (`BaseLoad` — keeps name, loses `actual_power_kw`):
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

## New types in `assets/mod.rs`

### AssetCapability

```rust
/// Point-in-time feasible power range. Follows the sign convention:
///   negative = export/generation, positive = import/consumption.
///
/// max_export_kw ≤ 0  (floor;  magnitude of maximum export)
/// max_import_kw ≥ 0  (ceiling; magnitude of maximum import)
///
/// For non-curtailable assets (PV, BaseLoad):
///   max_export_kw == max_import_kw == actual_power_kw  (point range, is_fixed() = true)
#[derive(Debug, Clone, Copy)]
pub struct AssetCapability {
    pub max_export_kw: f64,
    pub max_import_kw: f64,
}

impl AssetCapability {
    /// True if the asset has no controllable headroom (point-range).
    pub fn is_fixed(&self) -> bool {
        (self.max_import_kw - self.max_export_kw).abs() < 1e-6
    }
}
```

### `AssetConfig` enum — runtime config dispatch

Replaces the old `AssetState` enum for config-side dispatch.
Uses the same `#[serde(tag = "asset_type")]` so profile construction changes minimally.

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

Note: no `Grid` variant — Grid state is set by the controller from VTN capacity events,
not driven by an `AssetConfig`.

### New `AssetState` enum — mutable state only

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "asset_type", rename_all = "snake_case")]
pub enum AssetState {
    Battery(BatteryState),
    Ev(EvState),
    Heater(HeaterState),
    Pv(PvState),
    BaseLoad(BaseLoadState),
    /// Virtual asset: derived from the sum of all other assets + VTN capacity limits.
    /// Set by the controller; never produced by AssetConfig::step().
    Grid(GridState),
}

impl AssetState {
    pub fn actual_power_kw(&self) -> f64 { /* match arms — Grid returns net_power_kw */ }
}
```

`GridState` (per spec §1.3):
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GridState {
    pub net_power_kw:    f64,   // signed: positive = import
    pub import_limit_kw: f64,   // ≥ 0
    pub export_limit_kw: f64,   // ≤ 0
}
```

---

## `Asset` trait (Phase A subset)

Defined in `assets/mod.rs`. Full trait (with `id()`, `current_state()`, `history()`)
is added in Phase B/C when trait objects (`&dyn Asset`) are used by the planner.

```rust
pub trait Asset: Send + Sync {
    /// Pure physics step. Returns (new_state, actual_power_kw).
    /// actual_power_kw may differ from setpoint_kw (e.g. SoC ceiling clamps charge rate).
    /// Sign convention: positive = import/charge, negative = export/discharge.
    fn step(
        &self,
        state:       &AssetState,
        setpoint_kw: f64,
        dt:          Duration,
    ) -> (AssetState, f64);

    /// Point-in-time feasible power range given current state.
    fn capability(&self, state: &AssetState) -> AssetCapability;

    /// Project state forward over an explicit setpoint schedule (default impl).
    /// `setpoints` is a list of (slot_start, setpoint_kw) pairs in ascending time order.
    fn simulate_forward(
        &self,
        initial:   &AssetState,
        setpoints: &[(DateTime<Utc>, f64)],
    ) -> Trajectory {
        let mut state = initial.clone();
        let mut points = Vec::new();
        for window in setpoints.windows(2) {
            let (ts, sp) = window[0];
            let dt = window[1].0 - ts;
            let (next, actual_kw) = self.step(&state, sp, dt);
            points.push(TrajectoryPoint { ts, power_kw: actual_kw, state: state.clone() });
            state = next;
        }
        if let Some(&(ts, sp)) = setpoints.last() {
            let (_, actual_kw) = self.step(&state, sp, Duration::seconds(0));
            points.push(TrajectoryPoint { ts, power_kw: actual_kw, state });
        }
        Trajectory { points }
    }
}
```

Each concrete type implements `Asset`. `AssetConfig` also implements `Asset` by
dispatching to the inner concrete type.

```rust
impl Asset for Battery {
    fn step(&self, state: &AssetState, setpoint_kw: f64, dt: Duration) -> (AssetState, f64) {
        let AssetState::Battery(s) = state else { unreachable!("Battery/state mismatch") };
        let (ns, p) = self.step(s, setpoint_kw, dt);  // calls Battery::step(&BatteryState)
        (AssetState::Battery(ns), p)
    }
    fn capability(&self, state: &AssetState) -> AssetCapability {
        let AssetState::Battery(s) = state else { unreachable!() };
        self.capability(s)
    }
}
// Same pattern for EvCharger, Heater, PvInverter, BaseLoad.

impl Asset for AssetConfig {
    fn step(&self, state: &AssetState, setpoint_kw: f64, dt: Duration) -> (AssetState, f64) {
        match self {
            Self::Battery(cfg) => cfg.step(state, setpoint_kw, dt),
            Self::Ev(cfg)      => cfg.step(state, setpoint_kw, dt),
            Self::Heater(cfg)  => cfg.step(state, setpoint_kw, dt),
            Self::Pv(cfg)      => cfg.step(state, setpoint_kw, dt),
            Self::BaseLoad(cfg)=> cfg.step(state, setpoint_kw, dt),
        }
    }
    fn capability(&self, state: &AssetState) -> AssetCapability {
        match self {
            Self::Battery(cfg) => cfg.capability(state),
            Self::Ev(cfg)      => cfg.capability(state),
            Self::Heater(cfg)  => cfg.capability(state),
            Self::Pv(cfg)      => cfg.capability(state),
            Self::BaseLoad(cfg)=> cfg.capability(state),
        }
    }
}
```

---

## Methods on `AssetConfig`

All dispatch methods that previously lived on the old `AssetState` move to `AssetConfig`,
with signatures updated to take `state: &AssetState` where needed.

```rust
impl AssetConfig {
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

    /// Construct the initial AssetState from this config (for profile initialisation).
    pub fn initial_state(&self) -> AssetState { ... }
}
```

`forecast()` and `history()` retain their current signatures until Phase D replaces them
with `simulate_forward()` and per-asset buffer access respectively.

---

## `step()` per asset

### `Battery::step`

```rust
impl Battery {
    pub fn step(
        &self, state: &BatteryState, setpoint_kw: f64, dt: Duration,
    ) -> (BatteryState, f64) {
        let dt_h = dt.num_milliseconds() as f64 / 3_600_000.0;
        let clamped = setpoint_kw
            .max(-self.max_discharge_kw)
            .min(self.max_charge_kw);
        let (clamped, actual) = enforce_soc_bounds(self, state, clamped, dt_h);
        let energy_kwh = actual * dt_h
            * if actual > 0.0 { self.round_trip_efficiency } else { 1.0 };
        let new_soc = (state.soc_pct + energy_kwh / self.capacity_kwh).clamp(0.0, 1.0);
        (BatteryState { soc_pct: new_soc, actual_power_kw: actual }, actual)
    }
}
```

Physics identical to current `Battery::update()` — refactored to take state in,
return new state out.

### `EvCharger::step`

```rust
impl EvCharger {
    pub fn step(
        &self, state: &EvState, setpoint_kw: f64, dt: Duration,
    ) -> (EvState, f64) {
        if !state.plugged {
            return (EvState { actual_power_kw: 0.0, ..state.clone() }, 0.0);
        }
        let kw = setpoint_kw.clamp(-self.max_discharge_kw, self.max_charge_kw);
        let kw = if kw > 0.0 && state.soc_pct >= 1.0 { 0.0 }
                 else if kw < 0.0 && state.soc_pct <= self.min_soc { 0.0 }
                 else { kw };
        let dt_h = dt.num_milliseconds() as f64 / 3_600_000.0;
        let new_soc = (state.soc_pct + (kw * dt_h) / self.battery_kwh).clamp(0.0, 1.0);
        (EvState { soc_pct: new_soc, plugged: state.plugged, actual_power_kw: kw }, kw)
    }
}
```

### `Heater::step`

```rust
impl Heater {
    pub fn step(
        &self, state: &HeaterState, setpoint_kw: f64, dt: Duration,
    ) -> (HeaterState, f64) {
        let dt_h = dt.num_milliseconds() as f64 / 3_600_000.0;
        let clamped = setpoint_kw.clamp(0.0, self.max_kw);
        // Thermostat overrides; reads ambient_temp_c from self (set each tick by sim)
        let actual = if state.temperature_c >= self.temp_max_c { 0.0 }
                     else if state.temperature_c <= self.temp_min_c { clamped.max(self.min_power_kw) }
                     else { clamped };
        // Thermal model: loss = 0.1 kW/°C
        let loss_kw = (state.temperature_c - self.ambient_temp_c) * 0.1;
        let delta_c = (actual - loss_kw) / self.thermal_mass_kwh_per_c * dt_h;
        let new_temp = state.temperature_c + delta_c;
        (HeaterState { temperature_c: new_temp, actual_power_kw: actual }, actual)
    }
}
```

`self.ambient_temp_c` is read here (not from state). The sim tick loop sets it each tick
(see SimState::tick() section below).

### `PvInverter::step`

```rust
impl PvInverter {
    pub fn step(
        &self, _state: &PvState, _setpoint_kw: f64, _dt: Duration,
    ) -> (PvState, f64) {
        // Non-curtailable: output is determined by irradiance set in config each tick.
        // setpoint is ignored in Phase A.
        let raw_kw = -(self.rated_kw * self.irradiance);  // negative = export
        let actual_kw = self.export_limit_kw
            .map(|lim| raw_kw.max(lim))  // lim ≤ 0; max() clamps to less export
            .unwrap_or(raw_kw);
        (PvState { actual_power_kw: actual_kw }, actual_kw)
    }
}
```

`self.irradiance` is read here (not from state). The sim tick loop sets it each tick
(see SimState::tick() section below).

### `BaseLoad::step`

```rust
impl BaseLoad {
    pub fn step(
        &self, _state: &BaseLoadState, _setpoint_kw: f64, _dt: Duration,
    ) -> (BaseLoadState, f64) {
        let actual_kw = self.baseline_kw;
        (BaseLoadState { actual_power_kw: actual_kw }, actual_kw)
    }
}
```

---

## `capability()` per asset

### `Battery::capability`

```rust
pub fn capability(&self, state: &BatteryState) -> AssetCapability {
    AssetCapability {
        max_export_kw: if state.soc_pct <= self.min_soc { 0.0 } else { -self.max_discharge_kw },
        max_import_kw: if state.soc_pct >= 1.0          { 0.0 } else {  self.max_charge_kw },
    }
}
```

### `EvCharger::capability`

```rust
pub fn capability(&self, state: &EvState) -> AssetCapability {
    if !state.plugged {
        return AssetCapability { max_export_kw: 0.0, max_import_kw: 0.0 };
    }
    AssetCapability {
        max_export_kw: if state.soc_pct <= self.min_soc { 0.0 } else { -self.max_discharge_kw },
        max_import_kw: if state.soc_pct >= 1.0          { 0.0 } else {  self.max_charge_kw },
    }
}
```

### `Heater::capability`

Per spec: heater has a non-zero floor when at minimum temperature (forced on).

```rust
pub fn capability(&self, state: &HeaterState) -> AssetCapability {
    let max_import_kw = if state.temperature_c >= self.temp_max_c {
        0.0                       // overheat — forced off
    } else if state.temperature_c <= self.temp_min_c {
        self.min_power_kw         // too cold — forced on at minimum power
    } else {
        self.max_kw
    };
    AssetCapability { max_export_kw: 0.0, max_import_kw }  // heaters never export
}
```

### `PvInverter::capability`

Non-curtailable (point-range):

```rust
pub fn capability(&self, state: &PvState) -> AssetCapability {
    // Point-range: only achievable power is current output. is_fixed() = true.
    AssetCapability {
        max_export_kw: state.actual_power_kw,   // e.g. -2.0
        max_import_kw: state.actual_power_kw,   // same
    }
}
```

### `BaseLoad::capability`

```rust
pub fn capability(&self, state: &BaseLoadState) -> AssetCapability {
    AssetCapability {
        max_export_kw: state.actual_power_kw,
        max_import_kw: state.actual_power_kw,
    }
}
```

---

## `simulator/mod.rs` — SimState changes

### `AssetEntry` (per spec §4 — no `config` field)

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetEntry {
    pub id:            String,
    /// Mutable physics state. Written by the dispatcher every tick.
    pub state:         AssetState,
    /// Last commanded setpoint (kW, signed). Renamed from `setpoint`.
    pub setpoint_kw:   f64,
    /// Actual power from the last tick (kW).
    pub last_power_kw: f64,
    /// Cumulative energy since startup.
    pub energy:        EnergyCounter,
    /// Per-asset history ring buffer. Initialized empty in CP1; wired in CP2.
    pub history:       AssetHistoryBuffer,
}
```

### `SimState` gains `asset_configs`

Config is not part of `AssetEntry` (per spec). It lives in a parallel `Vec` in `SimState`,
loaded from profile on startup and serialized alongside assets in `sim_state.json`.

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimState {
    /// Physics config — parallel to `assets` by index, loaded from profile.
    pub asset_configs: Vec<AssetConfig>,
    /// Mutable state + history.
    pub assets:        Vec<AssetEntry>,
    pub grid:          GridMeter,
    pub last_tick:     DateTime<Utc>,
}
```

### `SimState::from_profile()` — updated

```rust
pub fn from_profile(profile: &Profile) -> Self {
    let mut configs:  Vec<AssetConfig>  = Vec::new();
    let mut entries:  Vec<AssetEntry>   = Vec::new();

    for asset_profile in profile.assets_iter() {   // iterates AssetProfile variants
        let cfg = AssetConfig::from_asset_profile(asset_profile);
        let state = cfg.initial_state();
        let setpoint_kw = cfg.default_setpoint(&state);
        entries.push(AssetEntry {
            id:            asset_profile.id().to_string(),
            state,
            setpoint_kw,
            last_power_kw: 0.0,
            energy:        EnergyCounter::new(),
            history:       AssetHistoryBuffer::new(3600),
        });
        configs.push(cfg);
    }
    // legacy `devices:` fallback follows same pattern with AssetConfig::from_*_config()
    Self { asset_configs: configs, assets: entries, grid: GridMeter::default(), last_tick: Utc::now() }
}
```

### `SimState::tick()` — updated

Before each asset step: set env-sourced fields in config, then dispatch step().

```rust
pub fn tick(&mut self, dt_s: f64, setpoints: HashMap<String, f64>, now: DateTime<Utc>, overrides: &UserOverrides) {
    let hour = /* compute fractional hour from now */;
    let irradiance = overrides.pv_irradiance
        .unwrap_or_else(|| compute_irradiance(hour));  // sin model 6am–6pm

    for (cfg, entry) in self.asset_configs.iter_mut().zip(self.assets.iter_mut()) {
        // ── Set env fields in config before step() ─────────────────────────
        match cfg {
            AssetConfig::Pv(pv)         => pv.irradiance     = irradiance,
            AssetConfig::Heater(heater) => heater.ambient_temp_c
                = overrides.ambient_temp_c.unwrap_or(10.0),
            _ => {}
        }

        // ── Apply UserOverride config mutations ────────────────────────────
        match cfg {
            AssetConfig::Ev(ev) => {
                if let Some(v) = overrides.ev_max_charge_kw { ev.max_charge_kw = v; }
                if let Some(v) = overrides.ev_soc_target    { ev.soc_target = v; }
                if let Some(b) = overrides.ev_plugged {
                    // plugged is state; update via state match below
                    if let AssetState::Ev(s) = &mut entry.state { s.plugged = b; }
                }
            }
            AssetConfig::Heater(h) => {
                if let Some(v) = overrides.heater_max_kw      { h.max_kw = v; }
                if let Some(v) = overrides.heater_temp_min_c  { h.temp_min_c = v; }
                if let Some(v) = overrides.heater_temp_max_c  { h.temp_max_c = v; }
            }
            AssetConfig::Pv(pv) => {
                if let Some(v) = overrides.pv_rated_kw { pv.rated_kw = v; }
            }
            AssetConfig::BaseLoad(bl) => {
                if let Some(w) = overrides.base_load_w { bl.baseline_kw = w / 1000.0; }
            }
            _ => {}
        }

        // ── Dispatch physics ───────────────────────────────────────────────
        let dt = Duration::milliseconds((dt_s * 1000.0) as i64);
        let sp = setpoints.get(&entry.id).copied()
            .unwrap_or_else(|| cfg.default_setpoint(&entry.state));
        let (new_state, actual_kw) = cfg.step(&entry.state, sp, dt);
        entry.state         = new_state;
        entry.last_power_kw = actual_kw;
        entry.setpoint_kw   = sp;
        entry.energy.integrate(actual_kw * 1000.0, dt_s);
    }

    // ── Derive grid meter (unchanged) ──────────────────────────────────────
    let total_kw: f64 = self.assets.iter().map(|a| a.last_power_kw).sum();
    // ... rest of grid meter update unchanged
}
```

The old `entry.state.update(dt_s, setpoint, env)` call and all inline
`match &mut entry.state { AssetState::Ev(ev) => ev.max_charge_kw = ... }` blocks
are replaced by the above pattern.

---

## Persistence note

`sim_state.json` gains a top-level `asset_configs` array alongside `assets`.
**Format change — delete `sim_state.json` on Pi4-Server before first CP1 deployment.**
The VEN starts fresh with profile defaults; no data is lost (buffers are ephemeral,
SoC recovers from first tick).

---

## Files changed (Checkpoint 1 only)

| File | Change |
|---|---|
| `profile.rs` | rename `AssetConfig` → `AssetProfile` everywhere in file |
| `simulator/mod.rs` | update all `use crate::profile::AssetConfig` → `AssetProfile` |
| `assets/mod.rs` | add `AssetCapability`, `Asset` trait, `Trajectory`/`TrajectoryPoint`, `GridState`; rename old `AssetState` → `AssetConfig`; add new `AssetState` enum (state only, incl. `Grid` variant); update all dispatch methods to take `state: &AssetState`; add `AssetConfig::initial_state()`, `Asset` impl for `AssetConfig` |
| `assets/battery.rs` | remove state fields (`soc`, `actual_power_kw`); add `Battery::step()`, `Battery::capability()`; `impl Asset for Battery` |
| `assets/ev.rs` | add `min_soc`; remove state fields (`soc`→rename, `plugged`, `current_kw`→rename); add `EvCharger::step()`, `EvCharger::capability()`; `impl Asset for EvCharger` |
| `assets/heater.rs` | add `ambient_temp_c`, `min_power_kw`, `thermal_mass_kwh_per_c`; remove state field `temp_c`; add `Heater::step()`, `Heater::capability()`; `impl Asset for Heater` |
| `assets/pv.rs` | add `irradiance`; remove state field; add `PvInverter::step()`, `PvInverter::capability()`; `impl Asset for PvInverter` |
| `assets/base_load.rs` | remove state field; add `BaseLoad::step()`, `BaseLoad::capability()`; `impl Asset for BaseLoad` |
| `simulator/mod.rs` | add `asset_configs: Vec<AssetConfig>` to `SimState`; redefine `AssetEntry` (per spec: no `config`, add `history`, rename `setpoint`→`setpoint_kw`); update `from_profile()`, `tick()`, and all accessor methods |
| All call sites reading `entry.state.update(...)` | replace with new `cfg.step()` dispatch |

---

## Success criteria

- `cargo build` compiles without error
- All existing BDD scenarios pass (`docker compose run --build test-runner`)
- Single commit: `refactor(ven): Phase A CP1 — config/state split, step(), capability()`
