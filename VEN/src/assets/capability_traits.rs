//! The capability traits an `Asset` may offer (`Asset::as_milp_participant`, `as_thermostat`, ...),
//! split out of `asset_trait.rs` to keep it under the file-size cap.

use chrono::{DateTime, Utc};

use super::AssetState;
use crate::entities::asset_params::{PvCurtailmentSource, RequestDefaults};
use crate::entities::device_session::{EvSession, HeaterTarget};
use crate::entities::timeline::HeaterPlanTrajectory;

/// Everything one planning cycle hands an asset to build its MILP context from: which asset it
/// is, its live state, the cycle's grid, the sessions and targets it may serve, and the planner's
/// own weights. One object rather than a positional list, so a kind reads the fields it needs and
/// a new field does not touch every implementor (R-71).
pub struct MilpBuildInputs<'a> {
    /// The roster id this context is built for; the context reports it as its `asset_id()`.
    pub asset_id: &'a str,
    pub state: &'a AssetState,
    /// Number of planning slots.
    pub n: usize,
    /// Cumulative slot boundary offsets from `now` [s], len = n + 1.
    pub cum_s: &'a [i64],
    pub now: DateTime<Utc>,
    /// Every queued EV session; an EV decides itself which ones it can serve.
    pub ev_sessions: &'a [EvSession],
    pub heater_target: Option<&'a HeaterTarget>,
    /// The asset's effective comfort curve (user override, else built-in default). Empty when
    /// the asset has none.
    pub comfort_rates: &'a [crate::entities::asset::ComfortRate],
    pub planner: &'a crate::entities::planner_params::PlannerParams,
    /// This asset's terminal value of stored energy [EUR/kWh]; 0.0 when it has none.
    pub c_terminal_eur_kwh: f64,
    /// Per-slot power the plan in force pins [kW]; empty for an asset that takes no anchor.
    pub heater_anchor: Vec<Option<f64>>,
    /// The PV forecast pin [kW] (sim inject `pv_plan_kw`, for deterministic tests): when set, a
    /// PV forecasts this, floored at 0, in every slot instead of its own forecast.
    pub pv_forecast_override: Option<f64>,
}

#[cfg(test)]
impl<'a> MilpBuildInputs<'a> {
    /// A four-slot cycle with no session, target, comfort curve, terminal value or anchor, and
    /// `planner`'s weights: what an asset's own context test varies from.
    pub(crate) fn for_test(
        asset_id: &'a str,
        state: &'a AssetState,
        now: DateTime<Utc>,
        planner: &'a crate::entities::planner_params::PlannerParams,
    ) -> Self {
        Self {
            asset_id,
            state,
            n: 4,
            cum_s: &[300, 600, 900, 1200],
            now,
            ev_sessions: &[],
            heater_target: None,
            comfort_rates: &[],
            planner,
            c_terminal_eur_kwh: 0.0,
            heater_anchor: vec![],
            pv_forecast_override: None,
        }
    }
}

/// Capability: this asset participates in MILP planning. Implemented by
/// Battery, EV, Heater and ShiftableLoad. See design.md Decision D4's
/// correction note for why `default_comfort_rates`/`default_completion_policy`/
/// `default_post_deadline_comfort_bid` are NOT here despite sounding
/// MILP-specific — all 5 asset kinds already implement them for real, so
/// they're universal `Asset` methods instead.
pub trait MilpParticipant {
    /// Build this asset's MILP context for one planning cycle.
    fn build_milp_context(
        &self,
        inputs: &MilpBuildInputs<'_>,
    ) -> Box<dyn crate::controller::milp_planner::AssetMilpContext>;
}

/// Capability: a user can issue a direct request against this asset (a target
/// SoC/power, or opportunistic surplus absorption). Implemented by the two
/// storage-shaped assets, Battery and EV.
pub trait RequestResolvable {
    /// The asset's own answer to "what would a user request resolve against": its current
    /// SoC, default target, capacity and charge rate. `AssetRequestSlice::resolve_request_target`
    /// is the one place the request arithmetic happens.
    fn request_defaults(&self, state: &AssetState) -> RequestDefaults;

    /// `(discharge_kwh, charge_kwh)` currently available, or `None` if the
    /// asset can't participate right now (e.g. an unplugged EV).
    fn available_storage_kwh(&self, state: &AssetState) -> Option<(f64, f64)>;

    /// How much of `surplus_kw` this asset could opportunistically absorb
    /// right now, or `None` if it can't absorb surplus at all (e.g. EV
    /// already at its charge target, or unplugged).
    fn surplus_charge_kw(&self, state: &AssetState, surplus_kw: f64) -> Option<f64>;
}

/// Capability: this asset has thermostat-shaped behavior (a target
/// temperature driving an on/off or discrete-stage setpoint). Implemented by
/// Heater only, today.
pub trait Thermostat {
    /// A stateful trajectory computer seeded from the live state, for
    /// recomputing the plan's own thermal trajectory — `None` if the current
    /// state gives no basis to project from.
    fn plan_trajectory(&self, live_state: &AssetState) -> Option<HeaterPlanTrajectory>;

    /// The on/off (or discrete-stage) setpoint \[kW\] that drives temperature
    /// toward `target_c` from the current state.
    fn thermostat_setpoint_kw(&self, state: &AssetState, target_c: f64) -> f64;

    /// The target (°C) a user request aims for when it states none, or `None` when this
    /// thermostat declares no default (then such a request is refused, not guessed).
    fn default_request_target_c(&self) -> Option<f64>;

    /// What a request is sized against: the current temperature, the energy per degree and the
    /// rating (R-124: a request that states only a temperature gets its energy from here).
    fn thermal_request_defaults(
        &self,
        state: &AssetState,
    ) -> crate::entities::asset_params::ThermalRequestDefaults;
}

/// Capability: this asset accepts tick-time environment/Behaviour-C overrides
/// from `SimState::tick()`. Implemented by Pv/Heater/BaseLoad/Ev — Battery has
/// no arm in `tick()`'s current match, so it declines.
///
/// Deferred from Spec A's Phase 0 (design.md Decision D5's addendum): the
/// self-contained `&mut self` shape only works once each implementor's
/// cross-cutting tick-level state (PV's/BaseLoad's smoothing) is resolved
/// *before* this is called, not inside it — see `SimState::tick()`, which
/// resolves `TickOverrides`' fields ahead of the per-asset loop.
pub trait TickOverridable {
    /// `state` is threaded through (unlike the other three capability traits)
    /// because EV's plugged-state override writes to `AssetState`, not just
    /// its own config — every other implementor ignores it.
    fn apply_tick_overrides(&mut self, state: &mut AssetState, overrides: &TickOverrides);
}

/// Bundles the per-tick override inputs `TickOverridable` implementors need,
/// pre-resolved where resolution requires cross-asset state (`pv_irradiance`/
/// `pv_irradiance_offset`, `base_load_baseline_kw` — see `SimState::tick()`'s
/// pre-loop resolution). One flat struct shared by 4 heterogeneous asset
/// kinds, each reading only its own fields — same shape as
/// `MilpParticipant::build_milp_context`'s signature, shared by 3 kinds.
pub struct TickOverrides {
    /// This tick's instant — the moment every live input below was captured.
    pub now: DateTime<Utc>,

    // PV
    pub pv_irradiance: f64,
    pub pv_irradiance_offset: f64,
    pub pv_tau_s: f64,
    pub pv_generation_limit_kw: Option<f64>,
    pub pv_curtailment_source: PvCurtailmentSource,
    pub pv_weather_power_kw: Option<f64>,
    /// Full weather-forecast series (`pv-competence-consolidation`) — see
    /// `PvInverter.weather_forecast`'s own doc comment.
    pub pv_weather_forecast: Option<Vec<crate::entities::solar::WeatherPvForecastSlot>>,
    pub pv_measured_power_kw: Option<f64>,
    pub pv_irradiance_forced: bool,

    // Heater
    pub heater_ambient_temp_c_override: Option<f64>,
    pub heater_temp_min_override: Option<f64>,
    pub heater_temp_max_override: Option<f64>,
    pub heater_emergency_curtail_override: Option<bool>,
    pub heater_emergency_absorb_override: Option<bool>,

    // BaseLoad — `base_load_baseline_kw` is the pre-resolved
    // `self.base_load_smoothing.update(...)` result, `None` if no BaseLoad
    // asset is configured (see `SimState::tick()`).
    pub base_load_measured_kw: Option<f64>,
    pub base_load_baseline_kw: Option<f64>,
    /// See `BaseLoad.heuristic`'s doc comment.
    pub base_load_heuristic: Option<crate::entities::design_vocabulary::AssetHeuristics>,

    // EV
    pub ev_plugged_override: Option<bool>,
    pub ev_soc_target_override: Option<f64>,
    pub ev_departure_time: Option<DateTime<Utc>>, // see EvCharger.departure_time
}
