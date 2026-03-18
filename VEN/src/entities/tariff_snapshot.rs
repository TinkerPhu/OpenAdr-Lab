use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// A single tariff data point for a time interval (tariff = price per kWh).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TariffSnapshot {
    pub interval_start: DateTime<Utc>,
    pub interval_end: DateTime<Utc>,
    pub import_price_eur_kwh: Option<f64>,
    pub export_price_eur_kwh: Option<f64>,
    pub co2_g_kwh: Option<f64>,
    /// Source event ID that provided this tariff
    pub source_event_id: Option<String>,
}

/// Collection of planned (future) tariff snapshots.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PlannedTariffs {
    pub snapshots: Vec<TariffSnapshot>,
}

impl PlannedTariffs {
    /// Find the tariff snapshot covering a given timestamp.
    pub fn tariff_at(&self, ts: DateTime<Utc>) -> Option<&TariffSnapshot> {
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

/// Collection of past (measured) tariff snapshots for cost attribution.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PastTariffs {
    pub snapshots: Vec<TariffSnapshot>,
}

/// Heuristic tariff forecast used when live price data is unavailable.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TariffHeuristic {
    /// Hourly average import prices (24 values, one per hour of day)
    pub hourly_avg_import_eur_kwh: Vec<f64>,
    pub last_updated: Option<DateTime<Utc>>,
}

impl Default for TariffHeuristic {
    fn default() -> Self {
        Self {
            hourly_avg_import_eur_kwh: vec![0.20; 24],
            last_updated: None,
        }
    }
}
