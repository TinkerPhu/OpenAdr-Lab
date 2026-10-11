//! Design vocabulary — shared domain enums and types used by running code
//! (`UserRequestMode`, `StaleRatePolicy`, `ForecastSource`, `UserNotificationSeverity`,
//! `AssetHeuristics`, `AssetForecast`, `TimeRange`). The type-level sketches of unbuilt
//! features that used to sit here were deleted (R-132); git keeps them.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// How a user expressed an energy task request (§1.9).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, TS)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum UserRequestMode {
    Asap,           // as soon as possible, cost-aware
    AsapFree,       // as soon as possible, only free/surplus energy
    ByDeadline,     // complete by deadline, cost-aware
    ByDeadlineFree, // complete by deadline, only free energy
    MaxCost,        // complete whenever, but within total cost limit
    Opportunistic,  // use only free/surplus energy, no deadline
}

impl UserRequestMode {
    /// Does this mode promise a target by a deadline, as opposed to bidding for it?
    ///
    /// Only the cost-aware deadline modes do. The `*_FREE` and `OPPORTUNISTIC`
    /// modes are gated by whatever surplus exists, so they cannot promise anything;
    /// `MAX_COST` is capped by a budget rather than bounded by a deadline. Declared
    /// here, beside the variants, so a new mode has to answer the question rather
    /// than quietly inherit an answer from a `match` arm somewhere else
    /// (`declare-dont-branch`).
    ///
    /// A firm *mode* is still not a firm obligation on its own: `EvSession`'s
    /// `soft_deadline` can waive it, in which case the comfort curve decides how far
    /// to charge.
    pub fn states_a_firm_deadline(&self) -> bool {
        matches!(self, Self::ByDeadline | Self::Asap)
    }

    /// Whether this mode's deadline ends its charging window. The deadline modes
    /// charge *for* a departure, so nothing is charged after it. The others charge
    /// whenever their own condition holds (free energy, a budget) and carry a
    /// deadline only because the request shape has one.
    pub fn charges_until_departure(&self) -> bool {
        matches!(self, Self::ByDeadline | Self::Asap | Self::ByDeadlineFree)
    }
}

/// BY_DEADLINE is the pre-mode implicit behaviour: cost-aware completion by a
/// deadline. Payloads without the field must keep behaving exactly as before.
impl Default for UserRequestMode {
    fn default() -> Self {
        Self::ByDeadline
    }
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
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, TS)]
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

/// Severity of a user notification (used in Stage 5).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, TS)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum UserNotificationSeverity {
    Info,  // informational — plan changed, schedule updated
    Warn,  // tier fallback, budget warning, deadline approaching
    Alert, // packet abandoned, device failed, grid emergency
}

/// Learned behavioral patterns for uncontrollable/implicit assets (§3.3).
/// Populated by `services::heuristics::learn_asset_heuristics` (WP5.2,
/// BL-14) — unlike most types in this file, this one is shipped behavior.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AssetHeuristics {
    pub asset_id: String,
    /// Typical power by time of day, one 24-hour curve per day of the week:
    /// index = `chrono::Weekday::num_days_from_monday()`, i.e. `[0]` = Monday
    /// … `[6]` = Sunday. A single scaled curve can't represent a genuinely
    /// different daily shape (e.g. brunch replacing coffee+lunch on Sat/Sun,
    /// or a school-schedule-driven difference between individual weekdays),
    /// so each day of the week gets its own learned curve rather than one
    /// curve times a per-day multiplier.
    pub daytime_profile_kw: [Vec<f64>; 7],
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
    /// `daytime_profile_kw[day_of_week_bucket][hour] × seasonal_factor`.
    /// Shared by `services::forecast::build_heuristic_forecasts` (the
    /// `/forecast` API) and `controller::milp_planner::inputs` (the
    /// planner's own solve inputs) so both consumers can never silently
    /// diverge into two different sampling formulas.
    pub fn sample_kw(&self, slot_t: DateTime<Utc>) -> f64 {
        use chrono::{Datelike, Timelike};
        let bucket = slot_t.weekday().num_days_from_monday() as usize;
        let hour = slot_t.hour() as usize;
        let base = self.daytime_profile_kw[bucket]
            .get(hour)
            .copied()
            .unwrap_or(0.0);
        base * self.seasonal_factor
    }
}

/// Predicted power profile for an asset over the planning horizon (§3.6).
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct AssetForecast {
    pub asset_id: String,
    pub updated_at: DateTime<Utc>,
    pub source: ForecastSource,
    /// 0.0–1.0 overall forecast confidence
    pub confidence: f64,
    /// Predicted power at each planning step (kW, positive = import)
    pub power_kw: Vec<f64>,
    /// Predicted SoC at each step (None for non-storage assets)
    #[serde(rename = "soc")]
    pub soc_frac: Option<Vec<f64>>,
    /// Predicted connection/availability windows (None = always available)
    pub availability_windows: Option<Vec<TimeRange>>,
}

/// A time window with start and end (used for availability forecasts).
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct TimeRange {
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn sample_kw_multiplies_hour_and_seasonal_factor() {
        let mut monday_profile = vec![0.5; 24];
        monday_profile[8] = 2.0;
        let h = AssetHeuristics {
            asset_id: "base_load".to_string(),
            daytime_profile_kw: std::array::from_fn(|i| {
                if i == 0 {
                    monday_profile.clone()
                } else {
                    vec![0.0; 24]
                }
            }),
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
            daytime_profile_kw: std::array::from_fn(|_| vec![]),
            seasonal_factor: 1.0,
            last_updated: None,
            recent_mean_abs_error_kw: None,
        };
        let now = Utc.with_ymd_and_hms(2023, 1, 2, 8, 0, 0).unwrap();
        assert_eq!(h.sample_kw(now), 0.0);
    }

    #[test]
    fn sample_kw_picks_the_correct_day_of_week_bucket() {
        // One distinct profile per day of the week (Mon=0..Sun=6), each
        // peaking at hour 8 with a value equal to its bucket index + 1 —
        // proves sample_kw resolves all 7 buckets independently, not just
        // a binary weekday/weekend split.
        let daytime_profile_kw: [Vec<f64>; 7] = std::array::from_fn(|bucket| {
            let mut profile = vec![0.0; 24];
            profile[8] = (bucket + 1) as f64;
            profile
        });
        let h = AssetHeuristics {
            asset_id: "base_load".to_string(),
            daytime_profile_kw,
            seasonal_factor: 1.0,
            last_updated: None,
            recent_mean_abs_error_kw: None,
        };

        // 2023-01-02 was a Monday, so 2023-01-02..08 covers Mon..Sun.
        for (offset, expected) in (0..7).zip(1..=7) {
            let day = Utc.with_ymd_and_hms(2023, 1, 2 + offset, 8, 0, 0).unwrap();
            assert!(
                (h.sample_kw(day) - expected as f64).abs() < 1e-9,
                "day offset {offset} expected {expected}, got {}",
                h.sample_kw(day)
            );
        }
    }
}
