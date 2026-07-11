//! Design vocabulary — type-level sketches of features not yet implemented.
//!
//! Every type in this module is unreferenced outside its own definition. None of it
//! describes current VEN behaviour — do not cite any of these as shipped behaviour in
//! docs or elsewhere. Each type has a tracked implementation plan in `docs/BACKLOG.md`
//! (see the BL-14 through BL-3x range for the item covering it).
#![allow(dead_code)]

use crate::entities::asset::ComfortRate;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// How adjustable an asset's power consumption/generation is (§1.2).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum PowerAdjustability {
    None,           // observe only (e.g. cooking stove, fixed load)
    Recommendation, // VEN can suggest but not enforce (e.g. washing machine)
    OnOff,          // binary switching — equivalent to Stepped with [0, MaxPower]
    Stepped,        // discrete power levels (e.g. 0/3/6 kW pump, step-controlled charger)
    Stepless,       // continuously adjustable within [min_kw, max_kw]
    Croppable,      // can be curtailed downward only (e.g. PV — can't exceed natural output)
}

/// How a user expressed an energy task request (§1.9).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum UserRequestMode {
    Asap,           // as soon as possible, cost-aware
    AsapFree,       // as soon as possible, only free/surplus energy
    ByDeadline,     // complete by deadline, cost-aware
    ByDeadlineFree, // complete by deadline, only free energy
    MaxCost,        // complete whenever, but within total cost limit
    Opportunistic,  // use only free/surplus energy, no deadline
}

/// BY_DEADLINE is the pre-mode implicit behaviour: cost-aware completion by a
/// deadline. Payloads without the field must keep behaving exactly as before.
impl Default for UserRequestMode {
    fn default() -> Self {
        Self::ByDeadline
    }
}

/// Used in capacity requests: which direction we're requesting from VTN (§1.6).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum FlexibilityDirection {
    Import, // requesting additional import capacity
    Export, // requesting additional export capacity
}

/// Rate type: how the rate is measured (§1.7).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum RateType {
    PerKwh, // €/kWh or gCO2/kWh — per-timeslot optimization
    PerKw,  // €/kW — capacity-based rate (translated to constraints before optimization)
}

/// Rate unit: what the rate is denominated in (§1.8).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum RateUnit {
    Eur,
    Usd,
    Chf,
    #[serde(rename = "g_CO2_eq")]
    GCo2Eq, // grams CO2 equivalent (grid intensity)
    #[serde(rename = "kg_CO2_eq")]
    KgCo2Eq, // kilograms CO2 equivalent (user-facing budgets)
}

/// How the Planner handles slots beyond the last known rate data (§1.10.1).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum StaleRatePolicy {
    LastKnown,         // repeat the last known rate for all future slots
    HeuristicForecast, // use learned rate heuristics (default)
    DeferToFlexible,   // mark all unknown slots as FLEXIBLE
    SafeAverage,       // use a configurable safety rate (e.g. 80th percentile)
}

/// How an asset forecast was generated (§1.11).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ForecastSource {
    WeatherModel,  // derived from external weather/irradiation data (PV)
    DeviceCloud,   // provided by manufacturer's cloud service
    PhysicalModel, // computed from physics (e.g. thermal model for heater)
    Heuristic,     // learned from historical usage patterns
    Manual,        // user-provided schedule
    Optimization,  // derived from the MILP plan's per-slot solution (WP3.6, BL-15)
    None,          // no forecast available (fully controllable assets like battery)
}

/// Type of external data source (§1.12).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ExternalDataSourceType {
    Weather,         // temperature, cloud cover, wind
    Irradiation,     // solar irradiation forecast
    GridCo2Forecast, // CO2 intensity forecast (if not from VTN)
}

/// Severity of a user notification (used in Stage 5).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum UserNotificationSeverity {
    Info,  // informational — plan changed, schedule updated
    Warn,  // tier fallback, budget warning, deadline approaching
    Alert, // packet abandoned, device failed, grid emergency
}

/// Condition that triggers a penalty rule check (§6.7).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum PenaltyCondition {
    PeakDemandExceeded,   // any timestep power > threshold_kW during period
    EnergyBudgetExceeded, // total consumption > threshold_kWh during period
    EventNoncompliance,   // didn't follow DR/emergency event within tolerance
    ExportLimitExceeded,  // exported more than allowed at any timestep
}

/// Power range for an asset: minimum and maximum controllable power (§2.3).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PowerRange {
    pub min_kw: f64,
    pub max_kw: f64,
    /// For Stepped adjustability: explicit discrete levels in kW (ascending).
    /// Empty = treat as OnOff with levels [0, max_kw].
    #[serde(default)]
    pub power_steps_kw: Vec<f64>,
}

/// Parameters for thermal model — heater / heat pump (§3.1.1).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThermalModelParams {
    pub thermal_mass_kwh_per_k: f64,     // energy to raise mass by 1K
    pub insulation_factor_kw_per_k: f64, // heat loss rate in kW/K
    pub min_temperature_c: f64,
    pub max_temperature_c: f64,
}

/// Static configuration of a device — set at installation/configuration time (§3.1).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetProfile {
    pub asset_id: String,
    pub asset_type: crate::entities::asset::AssetType,
    pub name: String, // human-readable, e.g. "Rooftop PV"
    pub power_range: PowerRange,
    pub adjustability: PowerAdjustability,
    pub auto_follow: bool,             // can device auto-adjust to fill gaps?
    pub bidirectional: bool,           // can both consume and produce? (battery, V2G)
    pub has_storage: bool,             // does it have an energy buffer?
    pub max_capacity_kwh: Option<f64>, // storage capacity if has_storage
    pub min_soc: Option<f64>,          // minimum SoC for discharge (e.g. 0.10)
    pub efficiency: f64,               // round-trip or conversion efficiency (0.0–1.0)
    pub response_delay_s: f64,         // expected time to confirm setpoint change
    pub deviation_threshold_kw: f64,   // |actual - planned| above this triggers replan
    pub default_value_curve: Option<DefaultValueCurve>,
    pub thermal_model: Option<ThermalModelParams>,
    pub oadr_resource_name: String, // maps to OpenADR resource.resourceName
}

/// Default value curve for an asset (used when no user-provided bid is given).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DefaultValueCurve {
    pub rates: Vec<ComfortRate>,
}

/// Live snapshot of device status — updated every measurement cycle (§3.2).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetState {
    pub ts: DateTime<Utc>,
    pub asset_id: String,

    pub commanded_kw: f64,       // setpoint sent to device
    pub actual_kw: f64,          // measured power from device (positive = import)
    pub power_deviation_kw: f64, // = actual_kw - commanded_kw (derived)

    pub responsiveness: crate::entities::asset::DeviceResponsiveness,
    pub last_confirmed_response: Option<DateTime<Utc>>,

    pub soc: Option<f64>,           // 0.0..1.0 for batteries/EV, None otherwise
    pub temperature_c: Option<f64>, // for thermal assets
    pub is_connected: bool,         // physically connected (EV plugged in, etc.)
    pub is_available: bool,         // logically available for control
}

/// Learned behavioral patterns for uncontrollable/implicit assets (§3.3).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AssetHeuristics {
    pub asset_id: String,
    /// Typical power by time of day (power_kw values, one per hour of day, 24 entries)
    pub daytime_profile_kw: Vec<f64>,
    /// Relative activity per weekday: Mon=0..Sun=6
    pub weekday_weights: Vec<f64>, // 7 values
    /// Multiplier for current season (e.g. 1.2 = 20% more in winter)
    pub seasonal_factor: f64,
    pub last_updated: Option<DateTime<Utc>>,
}

/// Predicted power profile for an asset over the planning horizon (§3.6).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetForecast {
    pub asset_id: String,
    pub updated_at: DateTime<Utc>,
    pub source: ForecastSource,
    /// 0.0–1.0 overall forecast confidence
    pub confidence: f64,
    /// Predicted power at each planning step (kW, positive = import)
    pub power_kw: Vec<f64>,
    /// Predicted SoC at each step (None for non-storage assets)
    pub soc: Option<Vec<f64>>,
    /// Predicted connection/availability windows (None = always available)
    pub availability_windows: Option<Vec<TimeRange>>,
}

/// A time window with start and end (used for availability forecasts).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimeRange {
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
}

/// Per-asset accumulated ledger for the current billing period (§3.7).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AssetLedger {
    pub asset_id: String,
    pub period_start: Option<DateTime<Utc>>,
    pub period_end: Option<DateTime<Utc>>, // None = ongoing

    pub total_consumption_kwh: f64, // all energy consumed (packets + untracked)
    pub total_production_kwh: f64,  // all energy produced
    pub total_import_cost_eur: f64, // cost of imported energy attributed to this asset
    pub total_export_revenue_eur: f64,
    pub total_co2_g: f64,

    pub untracked_energy_kwh: f64, // = total_consumption - tracked (standby, uncontrolled)
}

/// Flexibility this asset offers right now — computed on demand, not stored (§3.5).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetFlexibility {
    pub asset_id: String,
    pub ts: DateTime<Utc>,

    pub can_increase_consumption_kw: f64,
    pub can_decrease_consumption_kw: f64,
    pub can_increase_production_kw: f64,
    pub can_decrease_production_kw: f64,
}

/// External data source for weather, irradiation, or CO2 forecasts (§2.11).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExternalDataSource {
    pub source_id: String,
    pub source_type: ExternalDataSourceType,
    pub url: String,
    pub poll_interval_s: u64,
    pub last_fetch: Option<DateTime<Utc>>,
    pub fetch_status: ExternalDataFetchStatus,
    pub cached_data: Option<serde_json::Value>,
}

/// Status of an ExternalDataSource's last fetch.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ExternalDataFetchStatus {
    Ok,
    Stale,
    Failed,
    NeverFetched,
}

/// Trigger condition thresholds for a PenaltyRule (§6.8).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PenaltyThreshold {
    /// kW threshold for PEAK_DEMAND_EXCEEDED and EXPORT_LIMIT_EXCEEDED
    pub threshold_kw: Option<f64>,
    /// kWh threshold for ENERGY_BUDGET_EXCEEDED
    pub threshold_kwh: Option<f64>,
    /// Event reference for EVENT_NONCOMPLIANCE
    pub event_id: Option<String>,
    /// Tolerance for EVENT_NONCOMPLIANCE (e.g. 0.10 = 10% deviation allowed)
    pub tolerance_percent: Option<f64>,
}

/// Models a periodic/conditional charge triggered by threshold breach (§6.6).
/// Treated as binary barriers by the Planner: if allocation would cross threshold → add full penalty cost.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PenaltyRule {
    pub rule_id: String,
    pub description: String,
    pub condition: PenaltyCondition,
    pub threshold: PenaltyThreshold,
    /// Total cost in local currency if triggered (e.g. €100)
    pub cost: f64,
    pub cost_unit: RateUnit,
    /// Billing period (seconds), e.g. 2592000 = 30 days
    pub period_s: u64,
    /// Rolling average window for threshold evaluation (seconds), e.g. 900 = 15 min
    pub measurement_window_s: u64,
    pub active: bool,
    pub breached_this_period: bool,
    pub breach_timestamp: Option<DateTime<Utc>>,
    /// Current peak measured value (kW or kWh depending on condition)
    pub current_peak_value: f64,
    /// Rolling average power over measurement window (kW)
    pub rolling_average_kw: f64,
}
