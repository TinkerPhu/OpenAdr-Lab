use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// A single rate data point for a time interval.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateSnapshot {
    pub interval_start: DateTime<Utc>,
    pub interval_end: DateTime<Utc>,
    pub import_price_eur_kwh: Option<f64>,
    pub export_price_eur_kwh: Option<f64>,
    pub co2_g_kwh: Option<f64>,
    /// Source event ID that provided this rate
    pub source_event_id: Option<String>,
    /// Whether this is a measured past rate or a forecast
    pub is_forecast: bool,
}

/// Collection of planned (future) rate snapshots.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PlannedRates {
    pub snapshots: Vec<RateSnapshot>,
}

impl PlannedRates {
    /// Find the rate snapshot covering a given timestamp.
    pub fn rate_at(&self, ts: DateTime<Utc>) -> Option<&RateSnapshot> {
        self.snapshots
            .iter()
            .find(|s| s.interval_start <= ts && ts < s.interval_end)
    }

    /// Average import price over all planned snapshots; returns 0.20 if empty.
    pub fn avg_import_price(&self) -> f64 {
        let prices: Vec<f64> = self
            .snapshots
            .iter()
            .filter_map(|s| s.import_price_eur_kwh)
            .collect();
        if prices.is_empty() {
            return 0.20; // fallback flat rate
        }
        prices.iter().sum::<f64>() / prices.len() as f64
    }
}

/// Collection of past (measured) rate snapshots for cost attribution.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PastRates {
    pub snapshots: Vec<RateSnapshot>,
}

/// Heuristic rate forecast used when live price data is unavailable.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateHeuristic {
    /// Hourly average import prices (24 values, one per hour of day)
    pub hourly_avg_import_eur_kwh: Vec<f64>,
    pub last_updated: Option<DateTime<Utc>>,
}

impl Default for RateHeuristic {
    fn default() -> Self {
        Self {
            hourly_avg_import_eur_kwh: vec![0.20; 24],
            last_updated: None,
        }
    }
}
