//! Phase 1 (A-1) — row types persisted through `HistoryPort`.
//!
//! `payload_json` fields are the raw wire object (DTO-passthrough rule) —
//! typed columns exist only for the fields the query API filters/sorts on.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// One asset's mean power (and, where applicable, SoC/temperature) over a
/// 1-minute downsample window.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TickSample {
    pub ts: DateTime<Utc>,
    pub asset_id: String,
    pub power_kw: f64,
    pub soc_pct: Option<f64>,
    pub temperature_c: Option<f64>,
    /// PV generation limit active at some point in this window (kW, negative = generation ceiling).
    /// `None` when no source commanded any limit during the whole window — never a sentinel
    /// value. Within a window, the highest-priority source (capacity > plan) determines both
    /// this and `curtailment_source`, so a brief unplanned event is never masked by surrounding
    /// plan-sourced or unlimited samples. See `openspec/changes/pv-curtailment-history/`.
    #[serde(default)]
    pub generation_limit_kw: Option<f64>,
    /// Source of `generation_limit_kw`: `"plan"` or `"capacity"`. `None` iff `generation_limit_kw` is
    /// `None`.
    #[serde(default)]
    pub curtailment_source: Option<String>,
}

/// Site-level grid exchange and prevailing tariff over a 1-minute downsample window.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GridSample {
    pub ts: DateTime<Utc>,
    pub import_kw: f64,
    pub export_kw: f64,
    pub import_tariff_eur_kwh: Option<f64>,
    pub export_tariff_eur_kwh: Option<f64>,
    pub co2_g_kwh: Option<f64>,
}

/// An OpenADR event as accepted from the VTN.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EventReceived {
    pub received_at: DateTime<Utc>,
    pub event_id: String,
    pub event_type: String,
    pub payload_json: String,
}

/// An OpenADR report as submitted to the VTN.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReportSent {
    pub sent_at: DateTime<Utc>,
    pub report_type: String,
    pub event_id: String,
    pub payload_json: String,
}

/// A closed accounting period for one asset (BL-16 AssetLedger rollup).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LedgerPeriod {
    pub asset_id: String,
    pub period_start: DateTime<Utc>,
    pub period_end: DateTime<Utc>,
    pub energy_kwh: f64,
    pub cost_eur: f64,
    pub co2_kg: f64,
}

/// Lead-time bucket for a persisted forecast sample. `Near` = the plan's `slots[1]` (the closest
/// genuinely-future instant — `slots[0]` is what's currently being commanded, not a forecast
/// about to be tested); `Far` = `slots.last()`, the longest-lead prediction the current horizon
/// reaches. See `openspec/changes/forecast-accuracy-tracking/design.md` Decisions 1-2.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ForecastLeadKind {
    Near,
    Far,
}

impl std::fmt::Display for ForecastLeadKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            ForecastLeadKind::Near => "near",
            ForecastLeadKind::Far => "far",
        })
    }
}

impl std::str::FromStr for ForecastLeadKind {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "near" => Ok(ForecastLeadKind::Near),
            "far" => Ok(ForecastLeadKind::Far),
            other => Err(format!(
                "invalid lead_kind '{other}': expected 'near' or 'far'"
            )),
        }
    }
}

/// One forecast made for `target_ts`, recorded at `predicted_at` (the plan cycle that produced
/// it). `actual_kw`/`actual_at` start `None` and are filled in once `history_sampler` flushes a
/// downsampled window covering `target_ts` (`forecast-accuracy-tracking`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ForecastAccuracySample {
    pub asset_id: String,
    pub lead_kind: ForecastLeadKind,
    pub target_ts: DateTime<Utc>,
    pub predicted_kw: f64,
    pub predicted_at: DateTime<Utc>,
    pub actual_kw: Option<f64>,
    pub actual_at: Option<DateTime<Utc>>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn forecast_lead_kind_round_trips_through_its_string_form() {
        assert_eq!(ForecastLeadKind::Near.to_string(), "near");
        assert_eq!(ForecastLeadKind::Far.to_string(), "far");
        assert_eq!(
            ForecastLeadKind::from_str("near").unwrap(),
            ForecastLeadKind::Near
        );
        assert_eq!(
            ForecastLeadKind::from_str("far").unwrap(),
            ForecastLeadKind::Far
        );
    }

    #[test]
    fn forecast_lead_kind_rejects_an_invalid_string() {
        assert!(ForecastLeadKind::from_str("soon").is_err());
        assert!(ForecastLeadKind::from_str("").is_err());
    }
}
