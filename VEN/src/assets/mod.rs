use chrono::{DateTime, Duration, Utc};
use std::collections::{HashMap, VecDeque};

use crate::common::{Interpolation, TimeSeries};

pub mod base_load;
pub mod battery;
pub mod ev;
pub mod grid;
pub mod heater;
pub mod pv;

pub use base_load::{BaseLoad, BaseLoadState};
pub use battery::{Battery, BatteryState};
pub use ev::{EvCharger, EvState};
pub use grid::Grid;
pub use heater::{Heater, HeaterState};
pub use pv::{PvInverter, PvState};

// ─── Input type for a runtime-controllable parameter ─────────────────────────

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
    /// UI display multiplier: raw value × display_scale for display; divide by scale on send.
    /// E.g. display_scale=100.0 renders SoC fraction 0.8 as "80 %".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_scale: Option<f64>,
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

    /// State of charge in [0.0, 1.0] for storage assets; None for all others.
    pub fn soc(&self) -> Option<f64> {
        match self {
            Self::Battery(s) => Some(s.soc),
            Self::Ev(s) => Some(s.soc),
            _ => None,
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

    /// Time-weighted average of `power_kw` over the last `window` ending at `now`.
    ///
    /// Uses LOCF (last-observation-carried-forward): each recorded value is held
    /// until the next point. Falls back to the single latest value when no points
    /// fall within the window.
    pub fn recent_avg_power(&self, window: Duration, now: DateTime<Utc>) -> Option<f64> {
        let window_start = now - window;
        let in_window: Vec<_> = self
            .points
            .iter()
            .filter(|p| p.ts >= window_start && p.ts <= now)
            .collect();

        if in_window.is_empty() {
            return self.latest().map(|p| p.power_kw);
        }

        let mut weighted_sum = 0.0f64;
        let mut total_weight = 0.0f64;

        // Seed the LOCF value: the last point *before* the window start, if any.
        let seed_power = self
            .points
            .iter()
            .rev()
            .find(|p| p.ts < window_start)
            .map(|p| p.power_kw)
            .unwrap_or(in_window[0].power_kw);

        let mut last_power = seed_power;
        let mut last_t_ms = window_start.timestamp_millis();

        for pt in &in_window {
            let t_ms = pt.ts.timestamp_millis();
            if t_ms > last_t_ms {
                let dt = (t_ms - last_t_ms) as f64;
                weighted_sum += last_power * dt;
                total_weight += dt;
            }
            last_power = pt.power_kw;
            last_t_ms = t_ms;
        }

        // Carry the last in-window point forward to `now`.
        let now_ms = now.timestamp_millis();
        if now_ms > last_t_ms {
            let dt = (now_ms - last_t_ms) as f64;
            weighted_sum += last_power * dt;
            total_weight += dt;
        }

        if total_weight > 0.0 {
            Some(weighted_sum / total_weight)
        } else {
            self.latest().map(|p| p.power_kw)
        }
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

    /// Returns a stateful trajectory computer seeded from the current live state,
    /// or `None` for assets without planned state to recompute (battery, EV, PV, etc.).
    pub fn plan_trajectory(&self, live_state: &AssetState) -> Option<heater::HeaterPlanTrajectory> {
        match self {
            Self::Heater(cfg) => Heater::plan_trajectory(cfg, live_state),
            _ => None,
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

    /// Available storage energy. Returns `(discharge_kwh, charge_kwh)`.
    /// Returns `None` for non-storage assets or an unplugged EV.
    pub fn available_storage_kwh(&self, state: &AssetState) -> Option<(f64, f64)> {
        match (self, state) {
            (Self::Battery(b), AssetState::Battery(s)) => Some((
                (s.soc - b.min_soc).max(0.0) * b.capacity_kwh,
                (1.0 - s.soc).max(0.0) * b.capacity_kwh,
            )),
            (Self::Ev(e), AssetState::Ev(s)) if s.plugged => Some((
                (s.soc - e.min_soc).max(0.0) * e.battery_kwh,
                (1.0 - s.soc).max(0.0) * e.battery_kwh,
            )),
            _ => None,
        }
    }

    /// Thermostat ON/OFF setpoint [kW] for heating assets given a target temperature.
    /// Returns `None` for non-thermostat assets.
    pub fn thermostat_setpoint_kw(&self, state: &AssetState, target_c: f64) -> Option<f64> {
        match (self, state) {
            (Self::Heater(hcfg), AssetState::Heater(hs)) => Some(if hs.temperature_c < target_c {
                hcfg.max_kw
            } else {
                0.0
            }),
            _ => None,
        }
    }

    /// Surplus-charge absorption [kW] for assets that can opportunistically consume excess PV.
    /// Returns `None` when the asset cannot absorb surplus right now.
    pub fn surplus_charge_kw(&self, state: &AssetState, surplus_kw: f64) -> Option<f64> {
        match (self, state) {
            (Self::Ev(ecfg), AssetState::Ev(es)) if es.plugged && es.soc < ecfg.soc_target => {
                Some(surplus_kw.min(ecfg.max_charge_kw))
            }
            _ => None,
        }
    }

    /// Build the MILP context for this asset, or `None` for non-MILP assets (PV, base load, grid).
    pub fn build_milp_context(
        &self,
        state: &AssetState,
        n: usize,
        step_s: u64,
        now: DateTime<Utc>,
        ev_session: Option<&crate::entities::device_session::EvSession>,
        heater_target: Option<&crate::entities::device_session::HeaterTarget>,
        ev_min_charge_kw: f64,
        v_ev_extra_eur_kwh: f64,
        lambda_sw: f64,
    ) -> Option<AnyMilpContext> {
        match self {
            Self::Battery(cfg) => Some(AnyMilpContext::Battery(
                battery::BatteryMilpContext::from_state(state, cfg),
            )),
            Self::Ev(cfg) => Some(AnyMilpContext::Ev(ev::EvMilpContext::from_state(
                state,
                cfg,
                n,
                step_s,
                now,
                ev_session,
                ev_min_charge_kw,
                v_ev_extra_eur_kwh,
            ))),
            Self::Heater(cfg) => Some(AnyMilpContext::Heater(
                heater::HeaterMilpContext::from_state(
                    state,
                    cfg,
                    n,
                    step_s,
                    now,
                    heater_target,
                    lambda_sw,
                ),
            )),
            _ => None,
        }
    }
}

/// Unified MILP context for one asset and planning cycle.
/// One variant per MILP-capable asset type; non-MILP assets produce `None` from
/// `AssetConfig::build_milp_context()`.
pub enum AnyMilpContext {
    Battery(battery::BatteryMilpContext),
    Ev(ev::EvMilpContext),
    Heater(heater::HeaterMilpContext),
}

/// Full Asset trait. Combines the physics interface (Phase A) with the identity and
/// history interface (Phase B/C) needed for `&dyn Asset` trait objects.
///
/// Physics types (`Battery`, `EvCharger`, etc.) implement only `step()` and `capability()`.
/// They inherit the three identity/history methods with panicking defaults — those methods
/// must only be called via `AssetHandle`, which properly implements them.
pub trait Asset: Send + Sync {
    // ── Identity / observability (Phase B/C) ──────────────────────────────────

    /// Unique asset identifier (e.g. "battery", "ev", "grid").
    /// Default panics — call via `AssetHandle`, not a bare physics type.
    fn id(&self) -> &str {
        unimplemented!("Asset::id() must be called via AssetHandle, not a bare physics type")
    }

    /// Current live state snapshot. Positive = import from grid, negative = export.
    /// Default panics — call via `AssetHandle`, not a bare physics type.
    fn current_state(&self) -> AssetState {
        unimplemented!(
            "Asset::current_state() must be called via AssetHandle, not a bare physics type"
        )
    }

    /// Slice of this asset's own ring buffer over [now − window, now].
    /// Default panics — call via `AssetHandle`, not a bare physics type.
    fn history(&self, _window: Duration) -> Vec<HistoryPoint> {
        unimplemented!("Asset::history() must be called via AssetHandle, not a bare physics type")
    }

    // ── Physics primitives (Phase A) ──────────────────────────────────────────

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

// ─── AssetHandle ──────────────────────────────────────────────────────────────

/// Wraps individual fields from a `(AssetConfig, AssetEntry)` pair to implement
/// the full `Asset` trait, including `id()`, `current_state()`, and `history()`.
///
/// Takes individual field references instead of `&AssetEntry` to avoid a circular
/// dependency (`AssetEntry` lives in `simulator`, which imports from `assets`).
///
/// Usage:
/// ```ignore
/// let handle = AssetHandle {
///     config: &entry_config,
///     id: &entry.id,
///     state: &entry.state,
///     history: &entry.history,
/// };
/// ```
pub struct AssetHandle<'a> {
    pub config: &'a AssetConfig,
    pub id: &'a str,
    pub state: &'a AssetState,
    pub history: &'a AssetHistoryBuffer,
}

impl<'a> Asset for AssetHandle<'a> {
    fn id(&self) -> &str {
        self.id
    }

    fn current_state(&self) -> AssetState {
        self.state.clone()
    }

    fn history(&self, window: Duration) -> Vec<HistoryPoint> {
        self.history.slice(window, Utc::now())
    }

    fn capability(&self, state: &AssetState) -> AssetCapability {
        self.config.capability(state)
    }

    fn step(&self, state: &AssetState, setpoint_kw: f64, dt: Duration) -> (AssetState, f64) {
        self.config.step(state, setpoint_kw, dt)
    }

    // simulate_forward, simulate_free, capability_trajectory: default impls inherited from Asset
}

#[cfg(test)]
mod handle_tests {
    use super::*;

    fn make_battery_state(soc: f64, power_kw: f64) -> AssetState {
        AssetState::Battery(BatteryState {
            soc,
            actual_power_kw: power_kw,
        })
    }

    fn make_battery_config(capacity_kwh: f64, max_kw: f64) -> AssetConfig {
        AssetConfig::Battery(Battery {
            capacity_kwh,
            max_charge_kw: max_kw,
            max_discharge_kw: max_kw,
            round_trip_efficiency: 1.0,
            min_soc: 0.1,
        })
    }

    #[test]
    fn handle_id_returns_given_id() {
        let state = make_battery_state(0.5, 0.0);
        let config = make_battery_config(10.0, 5.0);
        let history = AssetHistoryBuffer::new(3600);
        let handle = AssetHandle {
            config: &config,
            id: "bat-01",
            state: &state,
            history: &history,
        };
        assert_eq!(handle.id(), "bat-01");
    }

    #[test]
    fn handle_current_state_returns_state() {
        let state = make_battery_state(0.7, 2.0);
        let config = make_battery_config(10.0, 5.0);
        let history = AssetHistoryBuffer::new(3600);
        let handle = AssetHandle {
            config: &config,
            id: "bat",
            state: &state,
            history: &history,
        };
        match handle.current_state() {
            AssetState::Battery(s) => {
                assert!((s.soc - 0.7).abs() < 1e-9);
                assert!((s.actual_power_kw - 2.0).abs() < 1e-9);
            }
            _ => panic!("expected Battery state"),
        }
    }

    #[test]
    fn handle_history_delegates_to_buffer() {
        let state = make_battery_state(0.5, 0.0);
        let config = make_battery_config(10.0, 5.0);
        let mut history = AssetHistoryBuffer::new(3600);
        let now = Utc::now();
        history.push(HistoryPoint {
            ts: now,
            power_kw: 3.0,
            state: make_battery_state(0.5, 3.0),
        });
        let handle = AssetHandle {
            config: &config,
            id: "bat",
            state: &state,
            history: &history,
        };
        let hist = handle.history(Duration::seconds(60));
        assert_eq!(hist.len(), 1);
        assert!((hist[0].power_kw - 3.0).abs() < 1e-9);
    }

    #[test]
    fn handle_capability_delegates_to_config() {
        let state = make_battery_state(0.5, 0.0);
        let config = make_battery_config(10.0, 5.0);
        let history = AssetHistoryBuffer::new(3600);
        let handle = AssetHandle {
            config: &config,
            id: "bat",
            state: &state,
            history: &history,
        };
        let cap = handle.capability(&state);
        // mid-SoC battery (soc=0.5, min_soc=0.1): can charge up to 5 kW and discharge up to 5 kW
        assert!((cap.max_import_kw - 5.0).abs() < 1e-9);
        assert!((cap.max_export_kw + 5.0).abs() < 1e-9); // -5.0
    }

    #[test]
    fn handle_step_delegates_to_config() {
        let state = make_battery_state(0.5, 0.0);
        let config = make_battery_config(10.0, 5.0);
        let history = AssetHistoryBuffer::new(3600);
        let handle = AssetHandle {
            config: &config,
            id: "bat",
            state: &state,
            history: &history,
        };
        let (new_state, actual_kw) = handle.step(&state, 5.0, Duration::seconds(3600));
        // 1 hour at 5 kW on 10 kWh battery → SoC goes from 0.5 to 1.0 (full)
        match new_state {
            AssetState::Battery(s) => assert!((s.soc - 1.0).abs() < 1e-6),
            _ => panic!("expected Battery state"),
        }
        assert!(actual_kw > 0.0);
    }
}

#[cfg(test)]
mod history_buffer_tests {
    use super::*;

    fn make_point(ts: DateTime<Utc>, power_kw: f64) -> HistoryPoint {
        // Use a battery state as a stand-in — the type is irrelevant for power tests.
        HistoryPoint {
            ts,
            power_kw,
            state: AssetState::Battery(crate::assets::battery::BatteryState {
                soc: 0.5,
                actual_power_kw: power_kw,
            }),
        }
    }

    fn secs(n: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(n, 0).unwrap()
    }

    // ── recent_avg_power ─────────────────────────────────────────────────────

    #[test]
    fn recent_avg_power_empty_buffer_returns_none() {
        let buf = AssetHistoryBuffer::new(100);
        let now = secs(100);
        assert!(buf.recent_avg_power(Duration::seconds(60), now).is_none());
    }

    #[test]
    fn recent_avg_power_constant_power_returns_that_power() {
        let mut buf = AssetHistoryBuffer::new(100);
        let now = secs(100);
        for i in 0..10 {
            buf.push(make_point(secs(i * 10), 1.5));
        }
        let avg = buf.recent_avg_power(Duration::seconds(60), now).unwrap();
        assert!((avg - 1.5).abs() < 1e-9, "expected 1.5, got {avg}");
    }

    #[test]
    fn recent_avg_power_alternating_returns_time_weighted_mean() {
        // 10 points alternating 0 and 2.5 at 1-second intervals in [0, 9].
        // Window = [0, 10]. Average should be ≈ 1.25 kW.
        let mut buf = AssetHistoryBuffer::new(100);
        for i in 0..10i64 {
            let power = if i % 2 == 0 { 0.0 } else { 2.5 };
            buf.push(make_point(secs(i), power));
        }
        let now = secs(10);
        let avg = buf.recent_avg_power(Duration::seconds(10), now).unwrap();
        // Tolerance of 0.5 kW: LOCF means off periods contribute 0, on periods 2.5
        assert!(avg > 0.5 && avg < 2.0, "expected ~1.25, got {avg}");
    }

    #[test]
    fn recent_avg_power_all_points_before_window_returns_latest() {
        // All points are older than the window → fallback to latest().
        let mut buf = AssetHistoryBuffer::new(100);
        buf.push(make_point(secs(0), 3.0));
        buf.push(make_point(secs(1), 4.0));
        let now = secs(100); // window = [40, 100], all points at t<40
        let avg = buf.recent_avg_power(Duration::seconds(60), now).unwrap();
        assert!((avg - 4.0).abs() < 1e-9, "expected 4.0 (latest), got {avg}");
    }

    #[test]
    fn recent_avg_power_window_larger_than_buffer_uses_all_points() {
        let mut buf = AssetHistoryBuffer::new(100);
        for i in 0..5 {
            buf.push(make_point(secs(i), 2.0));
        }
        let now = secs(4);
        let avg = buf.recent_avg_power(Duration::hours(1), now).unwrap();
        assert!((avg - 2.0).abs() < 1e-9, "expected 2.0, got {avg}");
    }
}
