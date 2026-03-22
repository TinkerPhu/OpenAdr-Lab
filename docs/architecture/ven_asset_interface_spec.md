# VEN Asset Interface Spec

**Purpose:** Coding reference. Exact Rust type signatures, field names (with units),
and module assignments for Phase A and downstream phases.
Rationale for every decision is in [ven_planning_architecture.md](ven_planning_architecture.md).

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
    reservation.rs    ← Reservation, ReservationLayer, FlexDirection,
                        ReservationSource  (NEW — Phase B)
    flexibility_policy.rs ← FlexibilityPolicy, ScheduledWindow  (NEW — Phase C)
    planner.rs        ← run_planner(), PlanStep, PlanReason,
                        LookaheadContext, SiteContext
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
| `reserved_up_kw` / `reserved_down_kw` | always ≥ 0 | Reservation magnitudes (directionless) |

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
    /// Used by the planner to build LookaheadContext (e.g. "when does the
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

## 2. `controller/reservation.rs` — Reservation layer (Phase B)

```rust
use uuid::Uuid;
use chrono::{DateTime, Utc};

#[derive(Debug, Clone)]
pub struct Reservation {
    pub id:       Uuid,
    pub window:   (DateTime<Utc>, DateTime<Utc>),
    /// None = site-level reservation (distributed across all assets proportionally).
    pub asset_id: Option<String>,
    /// Magnitude of reserved power. Always ≥ 0.
    /// Direction is carried by `direction`, not by sign.
    pub kw:        f64,
    pub direction: FlexDirection,
    pub source:    ReservationSource,
    /// Lower number = higher priority. 0 = hard grid constraint (VTN capacity limit).
    pub priority:  u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlexDirection {
    /// Hold headroom for consumption reduction (or export increase).
    /// Reduces available max_import_kw.
    Up,
    /// Hold headroom for consumption increase (or export reduction).
    /// Reduces available max_export_kw (makes it less negative).
    Down,
}

#[derive(Debug, Clone)]
pub enum ReservationSource {
    /// VTN SIMPLE-type demand response event: "reduce consumption by kw kW."
    /// Used only for FIRM obligations. Capacity limits (IMPORT/EXPORT_CAPACITY_LIMIT)
    /// are NOT modelled here — see "Capacity limits" note below.
    VtnFirmEvent   { event_id: String },
    PolicySchedule { policy_id: String },
    PolicyDefault,
    UserRequest    { request_id: Uuid },
}

/// Per-asset reservation totals at a specific instant.
#[derive(Debug, Clone, Default)]
pub struct AssetReservation {
    /// Total kW locked for upward flexibility (consumption reduction). Always ≥ 0.
    pub reserved_up_kw:   f64,
    /// Total kW locked for downward flexibility (consumption increase). Always ≥ 0.
    pub reserved_down_kw: f64,
}

pub struct ReservationLayer {
    reservations: Vec<Reservation>,
}

// ── Capacity limits — design note ────────────────────────────────────────────
//
// OpenADR IMPORT_CAPACITY_LIMIT and EXPORT_CAPACITY_LIMIT events do NOT produce
// Reservation records and are NOT stored in the ReservationLayer.
//
// Rationale: Reservation.kw encodes a *reduction magnitude* (headroom held, ≥ 0).
// A VTN capacity limit is an *absolute ceiling*, not a reduction amount. The delta
// between the physical grid connection and the limit is site-specific and not
// available in the event payload. Treating a limit value as a reduction would
// silently produce wrong available_cap() results.
//
// The correct owner is the Grid virtual asset (assets/grid.rs):
//   - Capacity limit events update GridState.import_limit_kw / export_limit_kw.
//   - Grid.capability() returns [export_limit_kw, import_limit_kw].
//   - The planner reads site limits from SiteContext.import_limit_kw (§3.4),
//     populated from the Grid asset's constrained capability.
//
// Until the Grid virtual asset is implemented, capacity limits flow through
// OadrCapacityState → PlanTimeSlot.import_cap_kw / .export_cap_kw (legacy path).
//
// The ReservationLayer handles only demand-response obligations expressible as
// reduction magnitudes: SIMPLE FIRM events, FlexibilityPolicy reserves, UserRequests.
// ─────────────────────────────────────────────────────────────────────────────

impl ReservationLayer {
    pub fn new() -> Self;
    pub fn insert(&mut self, r: Reservation);
    pub fn remove(&mut self, id: Uuid);

    /// Sum of all reservations active at `t` for the given asset (incl. site-level).
    pub fn query_asset(&self, asset_id: &str, t: DateTime<Utc>) -> AssetReservation;

    /// Shrinks `phys_cap` by active reservations.
    ///
    /// Up reservation reduces max_import_kw (keeps room to cut consumption):
    ///   avail.max_import_kw = phys_cap.max_import_kw − reserved_up_kw  (floor at 0)
    ///
    /// Down reservation reduces max_export_kw toward zero (keeps room to add consumption):
    ///   avail.max_export_kw = phys_cap.max_export_kw + reserved_down_kw  (ceiling at 0)
    pub fn available_cap(
        &self,
        asset_id: &str,
        phys_cap: AssetCapability,
        t:        DateTime<Utc>,
    ) -> AssetCapability {
        let res = self.query_asset(asset_id, t);
        AssetCapability {
            max_import_kw: (phys_cap.max_import_kw - res.reserved_up_kw)
                               .max(phys_cap.max_export_kw),  // can't go below export floor
            max_export_kw: (phys_cap.max_export_kw + res.reserved_down_kw)
                               .min(0.0),                     // stays ≤ 0
        }
    }
}
```

---

## 3. `controller/planner.rs` — Planning types (Phase D)

### 3.1 PlanStep

```rust
pub struct PlanStep {
    pub ts:                DateTime<Utc>,
    pub asset_id:          String,
    pub state_before:      AssetState,
    /// Physical capability at state_before (before reservations are applied).
    pub capability:        AssetCapability,
    pub reserved_up_kw:    f64,    // magnitude ≥ 0
    pub reserved_down_kw:  f64,    // magnitude ≥ 0
    /// Available range after reservations. Setpoint must lie in [avail_max_export_kw, avail_max_import_kw].
    pub avail_max_export_kw: f64,  // = capability.max_export_kw + reserved_down_kw  (≤ 0)
    pub avail_max_import_kw: f64,  // = capability.max_import_kw − reserved_up_kw    (≥ 0)
    pub setpoint_kw:       f64,
    /// Actual power after physics step. May differ from setpoint_kw (e.g. SoC clamp).
    pub actual_power_kw:   f64,
    pub reason:            PlanReason,
}
```

### 3.2 PlanReason

```rust
#[derive(Debug, Clone)]
pub enum PlanReason {
    FirmObligation  { source: ReservationSource, required_kw: f64 },
    CheapTariff     { tariff_eur_per_kwh: f64, threshold_eur_per_kwh: f64 },
    ExpensiveTariff { tariff_eur_per_kwh: f64, threshold_eur_per_kwh: f64 },
    GridImportLimit { limit_kw: f64 },
    GridExportLimit { limit_kw: f64 },  // limit_kw ≤ 0
    SocCeiling      { soc_pct: f64 },
    SocFloor        { soc_pct: f64 },
    ComfortBound    { bound_type: ComfortBoundType },
    UserOverride    { request_id: Uuid, mode: UserRequestMode },
    PolicyReserve   { policy_id: String },
    /// Deliberate non-action: documents why a seemingly good opportunity was skipped.
    /// Enables operator visibility ("cheap tariff but battery reserved for DR window").
    OpportunityMissed { reason: String },
    Idle,
}

#[derive(Debug, Clone, Copy)]
pub enum ComfortBoundType { MinTemperature, MaxTemperature, MinSoc, MaxSoc }
```

### 3.3 LookaheadContext

Computed once per asset per planning run via `capability_trajectory()`.
Passed read-only into `rules.choose()`.

```rust
pub struct LookaheadContext {
    /// Capability at each future step in free-run (from capability_trajectory()).
    pub capability_trajectory: Vec<(DateTime<Utc>, AssetCapability)>,
    /// Cheapest import tariff in [now, now + lookahead_window].
    pub tariff_min_ahead_eur_per_kwh: f64,
    /// Most expensive import tariff in [now, now + lookahead_window].
    pub tariff_max_ahead_eur_per_kwh: f64,
    /// When the asset hits its import ceiling (SoC full / comfort max) in free-run.
    /// None if not within the planning horizon.
    pub ceiling_eta: Option<DateTime<Utc>>,
    /// When the asset hits its export floor (SoC empty / comfort min) in free-run.
    pub floor_eta:   Option<DateTime<Utc>>,
}
```

### 3.4 SiteContext

Built incrementally as each asset is resolved in the planning loop.
Asset resolution order: PV → BaseLoad → Grid → controllable assets.
This ensures uncontrollable outputs are known before controllable assets are allocated.

```rust
pub struct SiteContext {
    /// Sum of already-committed setpoints at this time step (kW, signed).
    /// Controllable assets use this to stay within the site import/export limits.
    pub planned_others_kw: f64,
    /// Active site import limit (≥ 0). From Grid asset state.
    pub import_limit_kw:   f64,
    /// Active site export limit (≤ 0). From Grid asset state.
    pub export_limit_kw:   f64,
    /// PV free-run forecast at this step (≤ 0, kW).
    pub pv_forecast_kw:    f64,
}
```

### 3.5 Planner entry point (updated signature)

```rust
pub fn run_planner(
    assets:       &[&dyn Asset],
    tariffs:      &TariffTimeSeries,
    packets:      &[EnergyPacket],
    reservations: &ReservationLayer,    // Phase B: replaces inline capacity checks
    profile:      &Profile,
    now:          DateTime<Utc>,
    trigger:      PlanTrigger,
) -> (Plan, Vec<PlanStep>)              // Plan + full audit trail
```

---

## 4. `simulator/mod.rs` — AssetEntry (Phase A change)

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

## 5. `controller/flexibility_policy.rs` — FlexibilityPolicy (Phase C)

```rust
pub struct FlexibilityPolicy {
    pub default_reserve:    DefaultReserve,
    pub scheduled_windows:  Vec<ScheduledWindow>,
}

pub struct DefaultReserve {
    /// Always hold this much upward flexibility headroom across the site (kW, magnitude).
    pub up_kw:   f64,
    /// Always hold this much downward flexibility headroom across the site (kW, magnitude).
    pub down_kw: f64,
}

pub struct ScheduledWindow {
    pub id:               String,
    pub days:             Vec<Weekday>,
    pub time_start:       NaiveTime,
    pub time_end:         NaiveTime,
    pub reserve_up_kw:    f64,    // magnitude ≥ 0
    pub reserve_down_kw:  f64,    // magnitude ≥ 0
    /// Begin reserving this many minutes before the window opens.
    /// Must be long enough for storage assets to reach the required SoC.
    pub pre_load_minutes: u32,
}

impl FlexibilityPolicy {
    /// Materialise Reservation records covering [from, until].
    /// Called at startup, on config change, and daily to extend the window.
    pub fn generate_reservations(
        &self,
        from:  DateTime<Utc>,
        until: DateTime<Utc>,
    ) -> Vec<Reservation>;
}
```

YAML profile extension (additive, backwards compatible — absent = zero reserve):

```yaml
flexibility_policy:
  default_reserve:
    up_kw: 3.0
    down_kw: 3.0
  scheduled_windows:
    - id: "peak_dr_weekday"
      days: [Mon, Tue, Wed, Thu, Fri]
      time_start: "16:00"
      time_end: "20:00"
      reserve_up_kw: 10.0
      reserve_down_kw: 0.0
      pre_load_minutes: 60
```

---

## 6. Flexibility envelope (Phase E)

Lives in `controller/flexibility.rs` (new file) or `controller/planner.rs`.

```rust
pub struct FlexibilityEnvelope {
    pub ts:                        DateTime<Utc>,
    /// How much total consumption the VEN can shed right now (kW, magnitude).
    pub up_kw:                     f64,
    /// How much total consumption the VEN can increase right now (kW, magnitude).
    pub down_kw:                   f64,
    /// Conservative estimate of how long up_kw can be sustained.
    pub up_sustainable_duration:   Duration,
    /// Conservative estimate of how long down_kw can be sustained.
    pub down_sustainable_duration: Duration,
}

/// Computable at any time without running a full planning cycle.
///
/// up_kw:   distance from current operating point down to max_export floor
///   = Σ (actual_power_kw − avail.max_export_kw).max(0)
///
/// down_kw: distance from current operating point up to max_import ceiling
///   = Σ (avail.max_import_kw − actual_power_kw).max(0)
pub fn compute_envelope(
    assets:       &[&dyn Asset],
    reservations: &ReservationLayer,
    now:          DateTime<Utc>,
) -> FlexibilityEnvelope {
    let mut up_kw   = 0.0_f64;
    let mut down_kw = 0.0_f64;
    for asset in assets {
        let state = asset.current_state();
        let phys  = asset.capability(&state);
        let avail = reservations.available_cap(asset.id(), phys, now);
        let p     = state.actual_power_kw();
        up_kw   += (p - avail.max_export_kw).max(0.0);
        down_kw += (avail.max_import_kw - p).max(0.0);
    }
    // sustainable_duration: simulate_free each asset; find first step where
    // headroom drops below 50% of current value (conservative, not shown here).
    FlexibilityEnvelope { ts: now, up_kw, down_kw, /* durations */ }
}
```

---

## 7. User request leeway fields (Phase F)

Extension to `entities/user_request.rs`. All new fields are optional.

```rust
pub struct UserRequest {
    // existing fields …
    pub asset_id:       String,
    pub mode:           UserRequestMode,
    pub target_soc_pct: Option<f64>,
    pub deadline:       Option<DateTime<Utc>>,
    pub interruptible:  bool,

    // leeway fields (Phase F)
    /// Deadline may shift by ±tolerance_min minutes without violating the request.
    /// Widens the planner's scheduling window around the deadline.
    pub tolerance_min:  Option<i64>,
    /// Maximum the user is willing to pay. Planner reschedules away from expensive
    /// slots once accumulated cost approaches this ceiling.
    pub budget_eur:     Option<f64>,
}
```

---

## 8. Sign conventions reference

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
| `reserved_up_kw` | ≥ 0 | `2.0` | reservation magnitude — directionless |
| `reserved_down_kw` | ≥ 0 | `2.0` | reservation magnitude — directionless |


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

Apply a reservation (shrink the range from both ends):
```rust
// Up reservation reduces the import ceiling (holds room to cut consumption):
avail.max_import_kw = (cap.max_import_kw - reserved_up_kw).max(cap.max_export_kw);

// Down reservation raises the export floor toward zero (holds room to add consumption):
avail.max_export_kw = (cap.max_export_kw + reserved_down_kw).min(0.0);
```

Compute flexibility headroom from current operating point:
```rust
// How much consumption can be shed (distance down to export floor):
up_kw   = (actual_power_kw - avail.max_export_kw).max(0.0);

// How much consumption can be added (distance up to import ceiling):
down_kw = (avail.max_import_kw - actual_power_kw).max(0.0);
```
