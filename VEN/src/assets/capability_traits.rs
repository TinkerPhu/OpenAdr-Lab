//! The capability traits an `Asset` may offer (`Asset::as_milp_participant`, `as_thermostat`, ...),
//! split out of `asset_trait.rs` to keep it under the file-size cap.

use chrono::{DateTime, Utc};

use super::AssetState;
use crate::entities::asset_params::{PvCurtailmentSource, RequestDefaults};
use crate::entities::device_session::{EvSession, HeaterTarget};
use crate::entities::timeline::HeaterPlanTrajectory;

/// Capability: this asset participates in MILP planning. Implemented by
/// Battery/EV/Heater — exactly `AssetMilpContext`'s existing `AssetKind` scope
/// (`VEN/src/controller/asset_milp_port.rs`). See design.md Decision D4's
/// correction note for why `default_comfort_rates`/`default_completion_policy`/
/// `default_post_deadline_comfort_bid` are NOT here despite sounding
/// MILP-specific — all 5 asset kinds already implement them for real, so
/// they're universal `Asset` methods instead.
#[allow(dead_code)] // implemented starting Spec A Phase 2a (asset-dispatch-trait-objects tasks.md sec. 4); no implementor yet
pub trait MilpParticipant {
    /// Build the MILP context for this asset. Signature carries the full
    /// per-planning-cycle context every implementor needs (session/target
    /// state, reward weights) even though most parameters apply to only one
    /// or two asset kinds — see `AssetConfig::build_milp_context`'s existing
    /// doc history for why this wasn't split further.
    #[allow(clippy::too_many_arguments)] // one entry point for 4 heterogeneous asset kinds' MILP setup — see trait doc
    fn build_milp_context(
        &self,
        // Needed only by ShiftableLoadAsset (shiftable-load-as-asset): unlike
        // Battery/EV/Heater's compile-time-fixed ids, a shiftable load's id is
        // per-instance and not otherwise available inside this method.
        asset_id: &str,
        state: &AssetState,
        n: usize,
        cum_s: &[i64],
        now: DateTime<Utc>,
        ev_sessions: &[EvSession],
        heater_target: Option<&HeaterTarget>,
        // The asset's effective comfort curve (user override, else built-in
        // default). Empty when the asset has none.
        comfort_rates: &[crate::entities::asset::ComfortRate],
        ev_min_charge_kw: f64,
        v_ev_extra_eur_kwh: f64,
        v_ev_core_eur_kwh: f64,
        asap_lateness_eur_kwh_h: f64,
        v_ev_free_charge_eur_kwh: f64,
        lambda_sw: f64,
        c_terminal_eur_kwh: f64,
        heater_anchor: Vec<Option<f64>>,
        w_ghg_eur_kg: f64,
    ) -> Box<dyn crate::controller::milp_planner::AssetMilpContext>;
}

/// Capability: a user can issue a direct request against this asset (a target
/// SoC/power, or opportunistic surplus absorption). Implemented by the two
/// storage-shaped assets, Battery and EV.
#[allow(dead_code)] // implemented starting Spec A Phase 2a (asset-dispatch-trait-objects tasks.md sec. 4); no implementor yet
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
