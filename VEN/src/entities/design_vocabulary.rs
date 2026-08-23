//! Design vocabulary — type-level sketches of features not yet implemented.
//!
//! Every type in this module is unreferenced outside its own definition. None of it
//! describes current VEN behaviour — do not cite any of these as shipped behaviour in
//! docs or elsewhere. Each type has a tracked implementation plan in `docs/BACKLOG.md`
//! (see the BL-14 through BL-3x range for the item covering it).
//!
//! `PowerAdjustability` moved to `entities::asset` (BL-27) once it became live,
//! referenced code — `AssetProfile` below still uses it via that import, but the
//! enum itself is no longer a sketch.
#![allow(dead_code)]

use crate::entities::asset::{ComfortRate, PowerAdjustability};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

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
    pub oadr_resource_name: String, // maps to OpenADR resource.resourceName
}

/// Default value curve for an asset (used when no user-provided bid is given).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DefaultValueCurve {
    pub rates: Vec<ComfortRate>,
}

/// Learned behavioral patterns for uncontrollable/implicit assets (§3.3).
/// Populated by `services::heuristics::learn_asset_heuristics` (WP5.2,
/// BL-14) — unlike most types in this file, this one is shipped behavior.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AssetHeuristics {
    pub asset_id: String,
    /// Typical power by time of day, one 24-hour curve per weekday/weekend
    /// bucket: `[0]` = weekday (Mon-Fri), `[1]` = weekend (Sat/Sun). A
    /// single scaled curve can't represent a genuinely different daily
    /// shape (e.g. brunch replacing coffee+lunch), so weekday and weekend
    /// each get their own learned curve rather than one curve times a
    /// per-day multiplier.
    pub daytime_profile_kw: [Vec<f64>; 2],
    /// Multiplier for current season (e.g. 1.2 = 20% more in winter)
    pub seasonal_factor: f64,
    pub last_updated: Option<DateTime<Utc>>,
    /// R-60: recency-weighted mean absolute error between each tick's actual
    /// `power_kw` and what the *previously learned* heuristic (the one in
    /// effect at that tick's time, if any) would have predicted for it —
    /// computed by `services::heuristics::learn_asset_heuristics`. `None`
    /// when there was no previous heuristic to compare against (first-ever
    /// learning run for this asset). Purely additive instrumentation: not
    /// consumed by `sample_kw` or any other current reader — a future
    /// consumer (e.g. planner uncertainty weighting) can act on it.
    pub recent_mean_abs_error_kw: Option<f64>,
}

impl AssetHeuristics {
    /// Sample this heuristic's expected power at `slot_t`:
    /// `daytime_profile_kw[weekday_bucket][hour] × seasonal_factor`.
    /// Shared by `services::forecast::build_heuristic_forecasts` (the
    /// `/forecast` API) and `controller::milp_planner::inputs` (the
    /// planner's own solve inputs) so both consumers can never silently
    /// diverge into two different sampling formulas.
    pub fn sample_kw(&self, slot_t: DateTime<Utc>) -> f64 {
        use chrono::{Datelike, Timelike, Weekday};
        let bucket = if matches!(slot_t.weekday(), Weekday::Sat | Weekday::Sun) {
            1
        } else {
            0
        };
        let hour = slot_t.hour() as usize;
        let base = self.daytime_profile_kw[bucket]
            .get(hour)
            .copied()
            .unwrap_or(0.0);
        base * self.seasonal_factor
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn sample_kw_multiplies_hour_and_seasonal_factor() {
        let mut weekday_profile = vec![0.5; 24];
        weekday_profile[8] = 2.0;
        let h = AssetHeuristics {
            asset_id: "base_load".to_string(),
            daytime_profile_kw: [weekday_profile, vec![0.0; 24]],
            seasonal_factor: 1.1,
            last_updated: None,
            recent_mean_abs_error_kw: None,
        };

        // 2023-01-02 08:00 UTC was a Monday.
        let monday_8am = Utc.with_ymd_and_hms(2023, 1, 2, 8, 0, 0).unwrap();
        let expected = 2.0 * 1.1;
        assert!((h.sample_kw(monday_8am) - expected).abs() < 1e-9);
    }

    #[test]
    fn sample_kw_defaults_missing_indices_gracefully() {
        let h = AssetHeuristics {
            asset_id: "x".to_string(),
            daytime_profile_kw: [vec![], vec![]],
            seasonal_factor: 1.0,
            last_updated: None,
            recent_mean_abs_error_kw: None,
        };
        let now = Utc.with_ymd_and_hms(2023, 1, 2, 8, 0, 0).unwrap();
        assert_eq!(h.sample_kw(now), 0.0);
    }

    #[test]
    fn sample_kw_picks_weekday_bucket_for_weekday_and_weekend_bucket_for_weekend() {
        let mut weekday_profile = vec![0.0; 24];
        weekday_profile[8] = 1.2; // weekday coffee peak
        let mut weekend_profile = vec![0.0; 24];
        weekend_profile[10] = 2.2; // weekend brunch peak
        let h = AssetHeuristics {
            asset_id: "base_load".to_string(),
            daytime_profile_kw: [weekday_profile, weekend_profile],
            seasonal_factor: 1.0,
            last_updated: None,
            recent_mean_abs_error_kw: None,
        };

        // 2023-01-02 was a Monday, 2023-01-07 a Saturday.
        let tuesday_8am = Utc.with_ymd_and_hms(2023, 1, 3, 8, 0, 0).unwrap();
        let saturday_10am = Utc.with_ymd_and_hms(2023, 1, 7, 10, 0, 0).unwrap();

        assert!((h.sample_kw(tuesday_8am) - 1.2).abs() < 1e-9);
        assert!((h.sample_kw(saturday_10am) - 2.2).abs() < 1e-9);
        // Cross-check: weekday bucket has nothing at hour 10, weekend
        // bucket has nothing at hour 8 — proves the bucket actually
        // switches rather than falling back to a shared curve.
        assert_eq!(
            h.sample_kw(Utc.with_ymd_and_hms(2023, 1, 3, 10, 0, 0).unwrap()),
            0.0
        );
        assert_eq!(
            h.sample_kw(Utc.with_ymd_and_hms(2023, 1, 7, 8, 0, 0).unwrap()),
            0.0
        );
    }
}
