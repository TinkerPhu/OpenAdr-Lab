use chrono::{DateTime, Duration, Utc};
use std::collections::{HashMap, VecDeque};

use crate::common::{Interpolation, TimeSeries};

pub mod base_load;
pub mod battery;
pub mod ev;
pub mod heater;
pub mod pv;

pub use base_load::{BaseLoad, BaseLoadState};
pub use battery::{Battery, BatteryState};
pub use ev::{EvCharger, EvState};
pub use heater::{Heater, HeaterState};
pub use pv::{PvInverter, PvState};

// ─── Legacy types (still used by planner / dispatcher / routes) ───────────────

/// Planning capability descriptor for a single asset (legacy — kept for planner compat).
#[derive(Debug, Clone)]
pub struct AssetCapabilities {
    pub asset_id: String,
    pub max_import_kw: f64,
    pub max_export_kw: f64,
    pub is_flexible: bool,
    pub energy_state: Option<EnergyState>,
    pub availability: Option<TimeWindow>,
}

/// Storage state for flexible energy assets.
#[derive(Debug, Clone)]
pub struct EnergyState {
    pub current_kwh: f64,
    pub min_kwh: f64,
    pub max_kwh: f64,
}

/// Time window for asset availability.
#[derive(Debug, Clone)]
pub struct TimeWindow {
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
}

/// Input type for a runtime-controllable parameter.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ControlKind {
    Slider,
    Switch,
    NumberInput,
}

/// Descriptor for one controllable parameter exposed via GET /sim/schema.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ControlDescriptor {
    pub key: String,
    pub label: String,
    pub kind: ControlKind,
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub unit: String,
}

// ─── Phase A new types ────────────────────────────────────────────────────────

/// Point-in-time feasible power range. Valid only for the state it was computed from.
///
/// Sign convention: negative = export/generation, positive = import/consumption.
///   max_export_kw ≤ 0  — floor (maximum export magnitude)
///   max_import_kw ≥ 0  — ceiling (maximum import magnitude)
///
/// For non-curtailable assets (PV, BaseLoad): max_export_kw == max_import_kw == actual_power_kw.
#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
pub struct AssetCapability {
    pub max_export_kw: f64,
    pub max_import_kw: f64,
}

impl AssetCapability {
    /// True if the asset has no controllable headroom (point-range capability).
    pub fn is_fixed(&self) -> bool {
        (self.max_import_kw - self.max_export_kw).abs() < 1e-6
    }
}

/// State-only enum. Variants hold only mutable runtime state — no config fields.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(tag = "asset_type", rename_all = "snake_case")]
pub enum AssetState {
    Battery(BatteryState),
    Ev(EvState),
    Heater(HeaterState),
    Pv(PvState),
    BaseLoad(BaseLoadState),
    /// Virtual asset: derived from sum of all other assets + VTN capacity limits.
    Grid(GridState),
}

impl AssetState {
    /// Actual power in this state. Positive = import from grid, negative = export.
    pub fn actual_power_kw(&self) -> f64 {
        match self {
            Self::Battery(s) => s.actual_power_kw,
            Self::Ev(s) => s.actual_power_kw,
            Self::Heater(s) => s.actual_power_kw,
            Self::Pv(s) => s.actual_power_kw,
            Self::BaseLoad(s) => s.actual_power_kw,
            Self::Grid(s) => s.net_power_kw,
        }
    }
}

/// Grid virtual state. Not controllable; derived from sum of all other assets.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct GridState {
    /// Net site power. Positive = importing from grid. Negative = exporting to grid.
    pub net_power_kw: f64,
    /// Maximum site import power allowed by active VTN events. Always ≥ 0.
    pub import_limit_kw: f64,
    /// Maximum site export power allowed by active VTN events. Always ≤ 0.
    pub export_limit_kw: f64,
}

/// Trajectory produced by simulate_forward() / simulate_free().
pub struct Trajectory {
    pub points: Vec<TrajectoryPoint>,
}

/// State is the state AFTER the step at `ts`.
pub struct TrajectoryPoint {
    pub ts: DateTime<Utc>,
    /// Signed: positive = import, negative = export.
    pub power_kw: f64,
    pub state: AssetState,
}

/// One recorded tick in a per-asset history buffer.
#[derive(Debug, Clone)]
pub struct HistoryPoint {
    pub ts: DateTime<Utc>,
    /// Signed: positive = import from grid, negative = export.
    pub power_kw: f64,
    /// Full state snapshot at this tick.
    pub state: AssetState,
}

/// Rolling per-asset ring buffer of `HistoryPoint` values.
///
/// Capacity defaults to 3600 entries ≈ 1 h at 1 s tick rate.
/// Oldest point is evicted automatically when full.
#[derive(Debug, Clone)]
pub struct AssetHistoryBuffer {
    capacity: usize,
    points: VecDeque<HistoryPoint>,
}

impl AssetHistoryBuffer {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity,
            points: VecDeque::with_capacity(capacity),
        }
    }

    /// Append a point, evicting the oldest when at capacity.
    pub fn push(&mut self, point: HistoryPoint) {
        if self.points.len() == self.capacity {
            self.points.pop_front();
        }
        self.points.push_back(point);
    }

    /// All points in `[now − window, now]`, ordered ascending.
    pub fn slice(&self, window: Duration, now: DateTime<Utc>) -> Vec<HistoryPoint> {
        let start = now - window;
        self.points
            .iter()
            .filter(|p| p.ts >= start && p.ts <= now)
            .cloned()
            .collect()
    }

    /// Most recent point.
    pub fn latest(&self) -> Option<&HistoryPoint> {
        self.points.back()
    }

    /// Last-observation-carried-forward power at or before `t`.
    /// Returns `None` if no point exists at or before `t`.
    pub fn power_at(&self, t: DateTime<Utc>) -> Option<f64> {
        self.points
            .iter()
            .rev()
            .find(|p| p.ts <= t)
            .map(|p| p.power_kw)
    }

    pub fn len(&self) -> usize {
        self.points.len()
    }

    pub fn is_empty(&self) -> bool {
        self.points.is_empty()
    }
}

/// Runtime config dispatch enum. Holds physics config for each asset type.
/// This is the renamed + restructured successor to what was previously called `AssetState`
/// (which conflated config and state).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(tag = "asset_type", rename_all = "snake_case")]
pub enum AssetConfig {
    Battery(Battery),
    Ev(EvCharger),
    Heater(Heater),
    Pv(PvInverter),
    BaseLoad(BaseLoad),
}

impl AssetConfig {
    // ── Asset trait dispatch ────────────────────────────────────────────────

    pub fn step(&self, state: &AssetState, setpoint_kw: f64, dt: Duration) -> (AssetState, f64) {
        use Asset as _;
        match self {
            Self::Battery(cfg) => cfg.step(state, setpoint_kw, dt),
            Self::Ev(cfg) => cfg.step(state, setpoint_kw, dt),
            Self::Heater(cfg) => cfg.step(state, setpoint_kw, dt),
            Self::Pv(cfg) => cfg.step(state, setpoint_kw, dt),
            Self::BaseLoad(cfg) => cfg.step(state, setpoint_kw, dt),
        }
    }

    pub fn capability(&self, state: &AssetState) -> AssetCapability {
        use Asset as _;
        match self {
            Self::Battery(cfg) => cfg.capability(state),
            Self::Ev(cfg) => cfg.capability(state),
            Self::Heater(cfg) => cfg.capability(state),
            Self::Pv(cfg) => cfg.capability(state),
            Self::BaseLoad(cfg) => cfg.capability(state),
        }
    }

    // ── Dispatch methods (previously on AssetState) ─────────────────────────

    pub fn default_setpoint(&self, _state: &AssetState) -> f64 {
        match self {
            Self::Battery(cfg) => cfg.default_setpoint(),
            Self::Ev(cfg) => cfg.default_setpoint(),
            Self::Heater(cfg) => cfg.default_setpoint(),
            Self::Pv(cfg) => cfg.default_setpoint(),
            Self::BaseLoad(cfg) => cfg.default_setpoint(),
        }
    }

    pub fn state_values(&self, state: &AssetState) -> HashMap<String, f64> {
        match (self, state) {
            (Self::Battery(cfg), AssetState::Battery(s)) => cfg.state_values(s),
            (Self::Ev(cfg), AssetState::Ev(s)) => cfg.state_values(s),
            (Self::Heater(cfg), AssetState::Heater(s)) => cfg.state_values(s),
            (Self::Pv(cfg), AssetState::Pv(s)) => cfg.state_values(s),
            (Self::BaseLoad(cfg), AssetState::BaseLoad(s)) => cfg.state_values(s),
            _ => HashMap::new(),
        }
    }

    pub fn capabilities(&self, asset_id: &str, state: &AssetState) -> AssetCapabilities {
        match (self, state) {
            (Self::Battery(cfg), AssetState::Battery(s)) => cfg.capabilities(asset_id, s),
            (Self::Ev(cfg), AssetState::Ev(s)) => cfg.capabilities(asset_id, s),
            (Self::Heater(cfg), AssetState::Heater(s)) => cfg.capabilities(asset_id, s),
            (Self::Pv(cfg), AssetState::Pv(s)) => cfg.capabilities(asset_id, s),
            (Self::BaseLoad(cfg), AssetState::BaseLoad(s)) => cfg.capabilities(asset_id, s),
            _ => AssetCapabilities {
                asset_id: asset_id.to_string(),
                max_import_kw: 0.0,
                max_export_kw: 0.0,
                is_flexible: false,
                energy_state: None,
                availability: None,
            },
        }
    }

    pub fn control_schema(&self) -> Vec<ControlDescriptor> {
        match self {
            Self::Battery(cfg) => cfg.control_schema(),
            Self::Ev(cfg) => cfg.control_schema(),
            Self::Heater(cfg) => cfg.control_schema(),
            Self::Pv(cfg) => cfg.control_schema(),
            Self::BaseLoad(cfg) => cfg.control_schema(),
        }
    }

    pub fn reset(&self, state: &mut AssetState, values: HashMap<String, f64>) {
        match (self, state) {
            (Self::Battery(cfg), AssetState::Battery(s)) => cfg.reset(s, values),
            (Self::Ev(cfg), AssetState::Ev(s)) => cfg.reset(s, values),
            (Self::Heater(cfg), AssetState::Heater(s)) => cfg.reset(s, values),
            (Self::Pv(cfg), AssetState::Pv(s)) => cfg.reset(s, values),
            (Self::BaseLoad(cfg), AssetState::BaseLoad(s)) => cfg.reset(s, values),
            _ => {}
        }
    }

    pub fn update_config(&mut self, values: HashMap<String, f64>) {
        match self {
            Self::Battery(cfg) => cfg.update_config(values),
            Self::Ev(cfg) => cfg.update_config(values),
            Self::Heater(cfg) => cfg.update_config(values),
            Self::Pv(cfg) => cfg.update_config(values),
            Self::BaseLoad(cfg) => cfg.update_config(values),
        }
    }

    pub fn forecast(&self, state: &AssetState, timespan: Duration) -> TimeSeries {
        match (self, state) {
            (Self::Battery(cfg), AssetState::Battery(s)) => cfg.forecast(s, timespan),
            (Self::Ev(cfg), AssetState::Ev(s)) => cfg.forecast(s, timespan),
            (Self::Heater(cfg), AssetState::Heater(s)) => cfg.forecast(s, timespan),
            (Self::Pv(cfg), AssetState::Pv(s)) => cfg.forecast(s, timespan),
            (Self::BaseLoad(cfg), AssetState::BaseLoad(s)) => cfg.forecast(s, timespan),
            _ => TimeSeries::empty(Interpolation::Linear),
        }
    }

    pub fn resolve_request_target(
        &self,
        state: &AssetState,
        target_soc: Option<f64>,
        desired_power_kw: Option<f64>,
    ) -> Option<(f64, f64)> {
        match (self, state) {
            (Self::Battery(cfg), AssetState::Battery(s)) => {
                cfg.resolve_request_target(s, target_soc, desired_power_kw)
            }
            (Self::Ev(cfg), AssetState::Ev(s)) => {
                cfg.resolve_request_target(s, target_soc, desired_power_kw)
            }
            _ => None,
        }
    }

    pub fn default_comfort_rates(&self) -> Vec<crate::entities::asset::ComfortRate> {
        match self {
            Self::Battery(cfg) => cfg.default_comfort_rates(),
            Self::Ev(cfg) => cfg.default_comfort_rates(),
            Self::Heater(cfg) => cfg.default_comfort_rates(),
            Self::Pv(cfg) => cfg.default_comfort_rates(),
            Self::BaseLoad(cfg) => cfg.default_comfort_rates(),
        }
    }

    pub fn default_completion_policy(&self) -> crate::entities::asset::CompletionPolicy {
        match self {
            Self::Battery(cfg) => cfg.default_completion_policy(),
            Self::Ev(cfg) => cfg.default_completion_policy(),
            Self::Heater(cfg) => cfg.default_completion_policy(),
            Self::Pv(cfg) => cfg.default_completion_policy(),
            Self::BaseLoad(cfg) => cfg.default_completion_policy(),
        }
    }

    pub fn default_post_deadline_comfort_bid(&self) -> Option<f64> {
        match self {
            Self::Battery(cfg) => cfg.default_post_deadline_comfort_bid(),
            Self::Ev(cfg) => cfg.default_post_deadline_comfort_bid(),
            Self::Heater(cfg) => cfg.default_post_deadline_comfort_bid(),
            Self::Pv(cfg) => cfg.default_post_deadline_comfort_bid(),
            Self::BaseLoad(cfg) => cfg.default_post_deadline_comfort_bid(),
        }
    }

    pub fn simulate_free(&self, state: &AssetState, duration: Duration) -> Trajectory {
        use Asset as _;
        match self {
            Self::Battery(cfg) => cfg.simulate_free(state, duration),
            Self::Ev(cfg) => cfg.simulate_free(state, duration),
            Self::Heater(cfg) => cfg.simulate_free(state, duration),
            Self::Pv(cfg) => cfg.simulate_free(state, duration),
            Self::BaseLoad(cfg) => cfg.simulate_free(state, duration),
        }
    }

    pub fn capability_trajectory(
        &self,
        state: &AssetState,
        duration: Duration,
        resolution: Duration,
    ) -> Vec<(DateTime<Utc>, AssetCapability)> {
        use Asset as _;
        match self {
            Self::Battery(cfg) => cfg.capability_trajectory(state, duration, resolution),
            Self::Ev(cfg) => cfg.capability_trajectory(state, duration, resolution),
            Self::Heater(cfg) => cfg.capability_trajectory(state, duration, resolution),
            Self::Pv(cfg) => cfg.capability_trajectory(state, duration, resolution),
            Self::BaseLoad(cfg) => cfg.capability_trajectory(state, duration, resolution),
        }
    }
}

/// Phase A subset of the Asset trait. Full trait (with id(), current_state(), history())
/// added in Phase B/C when trait objects (&dyn Asset) are used by the planner.
pub trait Asset: Send + Sync {
    /// Pure physics step. Returns (new_state, actual_power_kw).
    /// actual_power_kw may differ from setpoint_kw (e.g. SoC ceiling clamps charge rate).
    /// Sign convention: positive = import/charge, negative = export/discharge.
    fn step(&self, state: &AssetState, setpoint_kw: f64, dt: Duration) -> (AssetState, f64);

    /// Point-in-time feasible power range given current state.
    fn capability(&self, state: &AssetState) -> AssetCapability;

    /// Free-run: step with setpoint=0.0 for `duration`. Single physics step.
    /// Override for assets where "free run" means something other than zero setpoint.
    fn simulate_free(&self, initial: &AssetState, duration: Duration) -> Trajectory {
        let now = Utc::now();
        self.simulate_forward(initial, &[(now, 0.0), (now + duration, 0.0)])
    }

    /// Capability at each `resolution` step in free-run (setpoint=0.0).
    /// Steps `duration / resolution` times; returns (timestamp, capability) pairs.
    /// Used by `precompute_lookahead()`.
    fn capability_trajectory(
        &self,
        initial: &AssetState,
        duration: Duration,
        resolution: Duration,
    ) -> Vec<(DateTime<Utc>, AssetCapability)> {
        let now = Utc::now();
        let n = (duration.num_seconds() / resolution.num_seconds().max(1)) as usize;
        let mut state = initial.clone();
        let mut result = Vec::with_capacity(n);
        for i in 1..=n {
            let (next, _) = self.step(&state, 0.0, resolution);
            result.push((now + resolution * i as i32, self.capability(&next)));
            state = next;
        }
        result
    }

    /// Project state forward over an explicit setpoint schedule (default impl).
    /// `setpoints` is a list of (slot_start, setpoint_kw) pairs in ascending time order.
    fn simulate_forward(
        &self,
        initial: &AssetState,
        setpoints: &[(DateTime<Utc>, f64)],
    ) -> Trajectory {
        let mut state = initial.clone();
        let mut points = Vec::new();
        for window in setpoints.windows(2) {
            let (ts, sp) = window[0];
            let dt = window[1].0 - ts;
            let (next, actual_kw) = self.step(&state, sp, dt);
            points.push(TrajectoryPoint {
                ts,
                power_kw: actual_kw,
                state: state.clone(),
            });
            state = next;
        }
        if let Some(&(ts, sp)) = setpoints.last() {
            let (_, actual_kw) = self.step(&state, sp, Duration::seconds(0));
            points.push(TrajectoryPoint {
                ts,
                power_kw: actual_kw,
                state,
            });
        }
        Trajectory { points }
    }
}
