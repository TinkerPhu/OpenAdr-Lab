use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Per-asset cumulative energy/cost/CO₂ since VEN startup.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AssetLedgerEntry {
    pub asset_id: String,
    pub energy_kwh: f64,
    pub cost_eur: f64,
    pub co2_g: f64,
    pub updated_at: Option<DateTime<Utc>>,
    pub started_at: Option<DateTime<Utc>>,
}

impl AssetLedgerEntry {
    pub fn new(asset_id: &str, now: DateTime<Utc>) -> Self {
        Self {
            asset_id: asset_id.to_string(),
            started_at: Some(now),
            ..Default::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_sets_started_at_from_injected_clock() {
        let now = "2026-07-16T12:00:00Z".parse::<DateTime<Utc>>().unwrap();
        let entry = AssetLedgerEntry::new("battery", now);
        assert_eq!(entry.asset_id, "battery");
        assert_eq!(entry.started_at, Some(now));
        assert_eq!(entry.energy_kwh, 0.0);
    }
}
