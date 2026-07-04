# VEN Asset Interface Spec

**Purpose:** Coding reference. Exact Rust type signatures, field names (with units),
and module assignments for the asset layer.

---

## Module layout

```
VEN/src/
  assets/
    mod.rs            ← Asset trait, all shared types (AssetState, AssetCapability,
                        Trajectory, HistoryPoint, AssetHistoryBuffer)
    battery.rs        ← Battery (implements Asset)
    ev.rs             ← EvCharger (implements Asset)
    heater.rs         ← Heater (implements Asset)
    pv.rs             ← PvInverter (implements Asset)
    base_load.rs      ← BaseLoad (implements Asset)
    grid.rs           ← Grid virtual asset (NEW — implements Asset, read-only)
  controller/
    milp_planner.rs   ← run_planner(), MilpSolution
    dispatcher.rs     ← setpoint overlay, AssetLedger
    envelope.rs       ← compute_envelope(), SiteFlexibilityEnvelope
    openadr_interface.rs ← parse_rate_snapshots(), parse_capacity_state()
    ...
  simulator/
    mod.rs            ← SimState, AssetEntry  (AssetEntry gains history field)
  profile.rs          ← AssetProfile enum (YAML loading) — NOTE: was named AssetConfig
                        before Phase A; renamed to avoid conflict with assets::AssetConfig
```

### Naming note: `AssetProfile` vs `AssetConfig`

`profile.rs` contains an enum for loading asset configuration from YAML.
It was originally named `AssetConfig` but is renamed to **`AssetProfile`** in Phase A
to avoid a naming collision with the runtime `assets::AssetConfig` enum introduced
in Phase A. The two types have distinct roles:

| Type | Location | Role |
|---|---|---|
| `AssetProfile` | `profile.rs` | YAML-loaded profile entry; carries `id` + profile fields |
| `AssetConfig` | `assets/mod.rs` | Runtime enum; dispatches `step()`, `capability()` etc. |

---

## Sign convention (applies everywhere, no exceptions)

**Negative = export direction. Positive = import direction.**

Field names that include a direction word (`import` / `export`) follow this convention
and store signed values. Field names without a direction word (`power_kw`, `net_power_kw`,
`setpoint_kw`) are also signed.

| Field | Sign | Meaning |
|---|---|---|
| `power_kw` / `actual_power_kw` / `setpoint_kw` | + import, − export | Universal |
| `max_import_kw` (AssetCapability) | always ≥ 0 | Ceiling of the feasible power range |
| `max_export_kw` (AssetCapability) | always ≤ 0 | Floor of the feasible power range |
| `import_limit_kw` (GridState) | always ≥ 0 | Site import allowance from VTN |
| `export_limit_kw` (GridState) | always ≤ 0 | Site export allowance from VTN |

**Naming rule:** fields named `max_export_kw` / `max_import_kw` use "max" to mean
*maximum capability in that direction*, not *mathematically largest value*. The sign
still follows the convention: `max_export_kw = -5.0` means the asset can export up to
5 kW. This avoids any conversion between magnitudes and signed values at every use site.

**PV / BaseLoad special case:** Non-curtailable assets have a point-range capability:
`max_import_kw == max_export_kw == actual_power_kw`. For a PV generating 2 kW this gives
`max_import_kw = -2.0` — the ceiling equals the floor, both negative. This looks
unusual but is logically correct: the asset operates at exactly one power level and
has no controllable headroom. `is_fixed()` detects this correctly.

---

## 1. `assets/mod.rs` — Asset trait and shared types

### 1.1 Asset trait

```rust
pub trait Asset: Send + Sync {
    // ── Identity ──────────────────────────────────────────────────────────
    fn id(&self) -> &str;

    // ── State ─────────────────────────────────────────────────────────────
    fn current_state(&self) -> AssetState;

    // ── Capability ────────────────────────────────────────────────────────
    /// Feasible power range given `state`.
    /// Returns [max_export_kw, max_import_kw] — the signed power bounds.
    /// Always SoC-dependent for storage — never call with a stale state.
    fn capability(&self, state: &AssetState) -> AssetCapability;

    // ── Physics step ──────────────────────────────────────────────────────
    /// Advance by `dt`. Returns (new_state, actual_power_kw).
    /// `actual_power_kw` may differ from `setpoint_kw` due to physics
    /// (e.g. SoC ceiling clamps effective charge rate below the commanded value).
    /// Sign convention: positive = import/charge, negative = export/discharge.
    fn step(
        &self,
        state:       &AssetState,
        setpoint_kw: f64,
        dt:          Duration,
    ) -> (AssetState, f64);

    // ── History ───────────────────────────────────────────────────────────
    /// Slice of this asset's own ring buffer over [now − window, now].
    fn history(&self, window: Duration) -> Vec<HistoryPoint>;

    // ── Derived (default implementations — do not override unless needed) ─

    /// Project state forward given an explicit setpoint schedule.
    /// `setpoints` is a list of (slot_start, setpoint_kw) pairs in ascending
    /// time order. Each pair defines the setpoint held until the next entry.
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

    /// Free-run: asset behaves with no external setpoint — idle, thermostat
    /// control, or irradiance-following. Asset implementations override this
    /// with their natural idle behaviour if it is not setpoint = 0.0.
    fn simulate_free(&self, initial: &AssetState, duration: Duration) -> Trajectory {
        let now = Utc::now();
        self.simulate_forward(initial, &[(now, 0.0), (now + duration, 0.0)])
    }

    /// How capability evolves over time in free-run.
    /// Used by the planner to project capability forward (e.g. "when does the
    /// battery fill at idle?").
    fn capability_trajectory(
        &self,
        initial:    &AssetState,
        duration:   Duration,
        resolution: Duration,
    ) -> Vec<(DateTime<Utc>, AssetCapability)> {
        let now  = Utc::now();
        let traj = self.simulate_free(initial, duration);
        traj.points
            .into_iter()
            .filter(|p| (p.ts - now).num_seconds() % resolution.num_seconds() == 0)
            .map(|p| (p.ts, self.capability(&p.state)))
            .collect()
    }
}
```

### 1.2 AssetCapability

```rust
/// Point-in-time feasible power range. Valid only for the state it was computed from.
///
/// Both fields follow the sign convention: negative = export, positive = import.
///   max_export_kw ≤ 0  — floor of the range (maximum export magnitude)
///   max_import_kw ≥ 0  — ceiling of the range (maximum import magnitude)
///
/// To clamp a setpoint to the feasible range:
///   setpoint = setpoint_kw.clamp(cap.max_export_kw, cap.max_import_kw)
///
/// For uncontrollable assets (PV, BaseLoad) max_export_kw == max_import_kw
/// == actual_power_kw. For a PV generating 2 kW both fields equal -2.0.
/// This looks unusual but correctly encodes a point-range: no headroom either way.
#[derive(Debug, Clone, Copy)]
pub struct AssetCapability {
    /// Maximum export / discharge power. Always ≤ 0.0. 0.0 = no export capability.
    pub max_export_kw: f64,
    /// Maximum import / charge power. Always ≥ 0.0. 0.0 = no import capability.
    /// Exception: equals max_export_kw (and may be negative) for non-curtailable
    /// assets that currently operate in export territory (see PV note above).
    pub max_import_kw: f64,
}

impl AssetCapability {
    /// True if the asset has no controllable headroom (point-range capability).
    pub fn is_fixed(&self) -> bool {
        (self.max_import_kw - self.max_export_kw).abs() < 1e-6
    }
}
```

### 1.3 AssetState

`AssetState` is the enum that unifies all asset states for the planner and timeline.
Each variant struct is defined in the corresponding asset module and re-exported here.

```rust
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(tag = "asset_type", rename_all = "snake_case")]
pub enum AssetState {
    Battery(BatteryState),
    Ev(EvState),
    Heater(HeaterState),
    Pv(PvState),
    BaseLoad(BaseLoadState),
    Grid(GridState),
}

impl AssetState {
    /// Actual power in this state. Positive = import from grid, negative = export.
    pub fn actual_power_kw(&self) -> f64 { /* match arms */ }
}
```

#### BatteryState

```rust
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BatteryState {
    /// State of charge in [0.0, 1.0]. 0.0 = empty, 1.0 = full.
    pub soc_pct: f64,
    /// Actual power last tick. Positive = charging (import). Negative = discharging (export).
    pub actual_power_kw: f64,
}
```

`Battery.capability(state)`:
```
max_export_kw = if soc_pct <= min_soc { 0.0 } else { -max_discharge_kw }
max_import_kw = if soc_pct >= 1.0    { 0.0 } else {  max_charge_kw }
```

#### EvState

```rust
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct EvState {
    /// State of charge in [0.0, 1.0].
    pub soc_pct: f64,
    pub plugged: bool,
    /// Actual power last tick. Positive = charging (import).
    /// Negative = V2G discharge (export) — only if V2G is configured.
    pub actual_power_kw: f64,
}
```

`EvCharger.capability(state)`:
```
if !plugged            → max_export_kw = 0.0,          max_import_kw = 0.0
if soc_pct >= 1.0      → max_import_kw = 0.0           (full — no more charging)
if soc_pct <= min_soc  → max_export_kw = 0.0           (V2G unavailable at low SoC)
otherwise              → max_export_kw = -max_v2g_kw,  max_import_kw = max_charge_kw
```

#### HeaterState

```rust
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct HeaterState {
    pub temperature_c: f64,
    /// Actual power last tick. Always ≥ 0 (heaters only consume).
    pub actual_power_kw: f64,
}
```

`Heater.capability(state)`:
```
if temperature_c >= comfort_max_c → max_import_kw = 0.0         (overheat — forced off)
if temperature_c <= comfort_min_c → max_import_kw = min_power_kw (forced on, non-zero floor)
otherwise                         → max_export_kw = 0.0, max_import_kw = max_power_kw
// Heaters never export: max_export_kw = 0.0 always.
```

#### PvState

```rust
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PvState {
    /// Actual power last tick. Always ≤ 0 (PV only exports). Unit: kW.
    pub actual_power_kw: f64,
}
```

`PvInverter.capability(state)` — non-curtailable variant:
```
// Point-range: asset is fixed at its current output.
max_export_kw = actual_power_kw   // e.g. -2.0 (generating 2 kW)
max_import_kw = actual_power_kw   // same — no headroom in either direction

// The negative max_import_kw is correct: the ceiling of this asset's power
// range is -2.0 kW. is_fixed() returns true. No contradiction.
```

`PvInverter.capability(state)` — curtailable variant (if inverter supports it):
```
max_export_kw = actual_power_kw   // floor: maximum generation at current irradiance
max_import_kw = 0.0               // ceiling: can curtail all the way to zero
```

Curtailment is modelled via setpoint in `[actual_power_kw, 0.0]`, not via a reservation.

#### BaseLoadState

```rust
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BaseLoadState {
    /// Actual power last tick. Always ≥ 0 (consumption only). Unit: kW.
    pub actual_power_kw: f64,
}
```

`BaseLoad.capability(state)`: point-range, same as non-curtailable PV:
```
max_export_kw = actual_power_kw
max_import_kw = actual_power_kw
```

#### GridState

Virtual asset. Not controllable; derived from the sum of all other assets.

```rust
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct GridState {
    /// Net site power. Positive = importing from grid. Negative = exporting to grid.
    pub net_power_kw: f64,

    /// Maximum site import power allowed by active VTN events. Always ≥ 0.
    pub import_limit_kw: f64,

    /// Maximum site export power allowed by active VTN events. Always ≤ 0.
    /// Stored negative per sign convention: -3.0 means the site may export up to 3 kW.
    /// 0.0 means no export is permitted.
    pub export_limit_kw: f64,
}
```

`Grid.capability(state)` — direct assignment, no conversion needed:
```
max_export_kw = state.export_limit_kw   // already ≤ 0
max_import_kw = state.import_limit_kw   // already ≥ 0
```

### 1.4 Trajectory

```rust
pub struct Trajectory {
    pub points: Vec<TrajectoryPoint>,
}

/// State is the state AFTER the step at `ts`.
/// The planner reads `points.last().state` as the starting state for the
/// next planning segment (enables chaining across planning steps).
pub struct TrajectoryPoint {
    pub ts:       DateTime<Utc>,
    pub power_kw: f64,        // signed: positive = import, negative = export
    pub state:    AssetState,
}
```

### 1.5 HistoryPoint

```rust
pub struct HistoryPoint {
    pub ts:       DateTime<Utc>,
    pub power_kw: f64,        // signed: positive = import, negative = export
    pub state:    AssetState,
}
```

### 1.6 AssetHistoryBuffer

Owned by each `AssetEntry` (§4). Replaces `ControllerTrace.asset_history`.

```rust
pub struct AssetHistoryBuffer {
    capacity: usize,                 // rows; default 3600 = 1 h at 1 s tick
    points:   VecDeque<HistoryPoint>,
}

impl AssetHistoryBuffer {
    pub fn new(capacity: usize) -> Self;

    /// Called by the dispatcher every tick. Evicts the oldest point when full.
    pub fn push(&mut self, point: HistoryPoint) {
        if self.points.len() == self.capacity { self.points.pop_front(); }
        self.points.push_back(point);
    }

    /// All points in [now − window, now], ordered ascending.
    pub fn slice(&self, window: Duration, now: DateTime<Utc>) -> Vec<HistoryPoint>;

    /// Most recent point. Used for the "current" segment of the timeline.
    pub fn latest(&self) -> Option<&HistoryPoint>;

    /// Last-observation-carried-forward value at or before `t`.
    /// Used for cross-asset aggregation: sum each asset's power_at(t) independently.
    pub fn power_at(&self, t: DateTime<Utc>) -> Option<f64>;
}
```

---



## 2. `simulator/mod.rs` — AssetEntry

```rust
pub struct AssetEntry {
    pub id:          String,
    /// Current physics state. Written by the dispatcher every tick.
    pub state:       AssetState,
    /// Last commanded setpoint (kW, signed).
    pub setpoint_kw: f64,
    /// Per-asset history ring buffer. Single source of truth for this asset's past.
    /// Capacity: 3600 points ≈ 1 h at 1 s tick rate.
    pub history:     AssetHistoryBuffer,
}
```

`ControllerTrace.asset_history` (`HashMap<String, AssetHistoryBuffer>`) is **removed**.
The dispatcher writes directly to `AssetEntry.history`.
Cross-asset aggregation queries each `AssetEntry.history.power_at(t)` independently —
no central mirror, no duplicated truth.

---



## 3. Sign conventions reference

**The single rule: negative = export / generation direction. Positive = import / consumption direction.**

This applies to every power value in the codebase without exception.

| Field | Always | Example | Meaning |
|---|---|---|---|
| `actual_power_kw` | signed | `+3.0` | consuming 3 kW from grid |
| `actual_power_kw` | signed | `-2.0` | exporting 2 kW to grid |
| `setpoint_kw` | signed | `-5.0` | command to discharge / export 5 kW |
| `max_import_kw` | ≥ 0 | `+7.0` | asset can import up to 7 kW |
| `max_export_kw` | ≤ 0 | `-5.0` | asset can export up to 5 kW |
| `import_limit_kw` (GridState) | ≥ 0 | `+10.0` | site may import up to 10 kW |
| `export_limit_kw` (GridState) | ≤ 0 | `-3.0` | site may export up to 3 kW |
| `net_power_kw` (GridState) | signed | `-1.5` | site currently exporting 1.5 kW |


### The feasible power range

`AssetCapability` defines a closed interval `[max_export_kw, max_import_kw]`.
Any setpoint within this interval is physically achievable:

```
max_export_kw  ──────────────────────────────  max_import_kw
     -5.0        feasible setpoints             +7.0
      │                                          │
  discharge                                   charge
  (export)                                   (import)
```

For uncontrollable assets (non-curtailable PV, BaseLoad) this interval collapses
to a single point: `max_export_kw == max_import_kw == actual_power_kw`. A PV
generating 2 kW has both fields set to `-2.0`. This means: the only achievable
power is −2.0 kW — no headroom in either direction. `is_fixed()` returns `true`.

### Arithmetic patterns

Clamp a desired setpoint to the feasible range:
```rust
let sp = desired_kw.clamp(cap.max_export_kw, cap.max_import_kw);
```

Compute flexibility headroom from current operating point:
```rust
// How much consumption can be shed (distance down to export floor):
up_kw   = (actual_power_kw - avail.max_export_kw).max(0.0);

// How much consumption can be added (distance up to import ceiling):
down_kw = (avail.max_import_kw - actual_power_kw).max(0.0);
```
