use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// How adjustable an asset's power consumption/generation is.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum PowerAdjustability {
    None,           // observe only (e.g. cooking stove, fixed load)
    Recommendation, // VEN can suggest but not enforce (e.g. washing machine)
    OnOff,          // binary switching — equivalent to Stepped with [0, MaxPower]
    Stepped,        // discrete power levels (e.g. 0/3/6 kW pump, step-controlled charger)
    Stepless,       // continuously adjustable within [min_kw, max_kw]
    Croppable,      // can be curtailed downward only (e.g. PV — can't produce more than natural)
}

/// Device health and communication status (not response speed).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DeviceResponsiveness {
    Responsive,   // device confirms setpoints within expected delay
    Degraded,     // device responds but outside expected parameters
    Unresponsive, // device not confirming setpoint changes
    Offline,      // device not communicating at all
}

/// How to handle completion when the last explicit DeadlineTier expires and packet is not done.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CompletionPolicy {
    /// Terminate immediately → PARTIAL_COMPLETED if FillPercentage < 1.0.
    /// Use when the asset is needed for another task or partial result is acceptable.
    Stop,
    /// Keep going, bidding at PostDeadlineComfortBid for priority.
    /// The bid determines how aggressively the packet competes after the deadline.
    Continue,
}

/// How a user requested an energy task — determines Planner behavior.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum UserRequestMode {
    Asap,            // as soon as possible, cost-aware
    AsapFree,        // as soon as possible, only free/surplus energy
    ByDeadline,      // complete by deadline, cost-aware
    ByDeadlineFree,  // complete by deadline, only free energy
    MaxCost,         // complete whenever, but within total cost limit
    Opportunistic,   // use only free/surplus energy, no deadline
}

/// What triggered a plan recomputation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum PlanTrigger {
    Periodic,        // regular planning cycle (every PlanTimeStep)
    RateChange,      // new PRICE/GHG/EXPORT_PRICE event from VTN
    CapacityChange,  // new capacity limit/reservation from VTN
    Alert,           // emergency/flex alert from VTN
    UserRequest,     // new or modified EnergyPacket from user
    DeviceDeviation, // significant actual vs. planned deviation detected
    AssetStateChange, // device connected/disconnected/failed
}

/// Power range for an asset: minimum and maximum controllable power in kW.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PowerRange {
    pub min_kw: f64,
    pub max_kw: f64,
}

/// Defines how adjustable the asset is over its power range.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetPowerAdjustability {
    pub adjustability: PowerAdjustability,
    pub responsiveness: DeviceResponsiveness,
    pub power_range: PowerRange,
    /// For `Steps` adjustability: the allowed discrete power levels in kW (ascending).
    /// Ignored for other adjustability types. If empty, treated as OnOff.
    #[serde(default)]
    pub step_values_kw: Vec<f64>,
    /// Can the asset export power (V2G, battery discharge)?
    pub can_export: bool,
}

/// Parameters for thermal model (heater / heat pump).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThermalModelParams {
    pub thermal_mass_kwh_per_c: f64, // thermal mass of building
    pub heat_loss_kw_per_c: f64,     // ambient heat loss coefficient
    pub temp_min_c: f64,
    pub temp_max_c: f64,
    pub ambient_temp_c: f64, // outdoor temperature
}

/// A single point in a comfort/value curve:
/// at `fill` fraction of task completion the marginal bid (willingness to pay) is `bid_eur_kwh`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComfortRate {
    pub fill: f64,        // 0.0..1.0 task completion fraction
    pub bid_eur_kwh: f64, // €/kWh marginal bid at this fill level
}

/// Default value curve for an asset (used when no user-provided bid is given).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DefaultValueCurve {
    pub rates: Vec<ComfortRate>,
}

/// Static profile for an asset — set at configuration time.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetProfile {
    pub asset_id: String,
    pub asset_type: String, // "EV", "HEATER", "PV", "BATTERY", "SITE_RESIDUAL", …
    pub adjustability: AssetPowerAdjustability,
    pub default_value_curve: Option<DefaultValueCurve>,
    pub thermal_model: Option<ThermalModelParams>,
    pub min_soc: Option<f64>, // minimum SoC (battery / EV)
}

/// Live state for an asset at a given instant.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetState {
    pub asset_id: String,
    pub current_kw: f64,   // positive = import, negative = export
    pub soc: Option<f64>,  // for batteries / EV: 0.0..1.0
    pub temp_c: Option<f64>, // for heaters
    pub available: bool,   // is device currently accessible?
}

/// Forecast of an asset's baseline power over a future planning window.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetForecast {
    pub asset_id: String,
    /// Forecasted power at each planning step (kW, positive = import)
    pub power_kw: Vec<f64>,
    /// SoC at each planning step (None for non-storage assets)
    pub soc: Option<Vec<f64>>,
}

/// Flexibility offered by an asset to the grid over a future window.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetFlexibility {
    pub asset_id: String,
    pub energy_available_kwh: f64, // total flexible energy
    pub power_range: PowerRange,   // min/max power modulation
    pub time_window_start: DateTime<Utc>,
    pub time_window_end: DateTime<Utc>,
    /// Price range where asset is available: (min_bid, max_bid) €/kWh
    pub rate_range: Option<(f64, f64)>,
}

/// Per-asset accumulated ledger for the current billing period.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AssetLedger {
    pub asset_id: String,
    pub import_kwh: f64,
    pub export_kwh: f64,
    pub cost_eur: f64,
    pub co2_kg: f64,
    pub period_start: Option<DateTime<Utc>>,
}

/// Heuristics derived from historical usage patterns.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AssetHeuristics {
    pub asset_id: String,
    /// Average hourly load profile (24 values, one per hour of day)
    pub hourly_avg_kw: Vec<f64>,
    pub last_updated: Option<DateTime<Utc>>,
}
