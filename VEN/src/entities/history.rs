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
    /// PV export limit active at some point in this window (kW, negative = export ceiling).
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

/// A snapshot of the planner's output at the moment a plan cycle completed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlanSnapshot {
    pub created_at: DateTime<Utc>,
    pub horizon_start: DateTime<Utc>,
    pub horizon_end: DateTime<Utc>,
    pub plan_json: String,
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
