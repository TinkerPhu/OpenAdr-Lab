// Module-wide allow: `OadrCapacityState`/`OadrReportObligation` are live (constructed via
// serde deserialization and read elsewhere); `OadrProgramConfig`/`OadrEventCache`/
// `OadrCapacityRequest` below are unwired sketches, kept intentionally (not deleted) —
// see docs/BACKLOG.md BL-24 and docs/reference/TECHNICAL_DEBTS.md R-13 (DISPATCH_SETPOINT).
#![allow(dead_code)]
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::entities::site_meter::PowerSnapshot;
use crate::entities::tariff_snapshot::TariffSnapshot;

/// Current capacity state derived from active OpenADR events.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct OadrCapacityState {
    /// Maximum import power allowed (kW); None = no limit
    pub import_limit_kw: Option<f64>,
    /// Maximum export power allowed (kW); None = no limit
    pub export_limit_kw: Option<f64>,
    /// Subscribed capacity (kW) committed to the grid
    pub import_subscription_kw: Option<f64>,
    /// Active import reservation granted by VTN (kW)
    pub import_reservation_kw: Option<f64>,
    /// Source event ID for the import limit
    pub import_limit_event_id: Option<String>,
    /// Source event ID for the export limit
    pub export_limit_event_id: Option<String>,
    pub last_updated: Option<DateTime<Utc>>,
}

/// Configuration for an OpenADR program this VEN participates in (§5.1).
/// Unwired sketch — never constructed anywhere. See docs/BACKLOG.md BL-24.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OadrProgramConfig {
    pub program_id: String,
    pub program_name: String,
    /// Signal types this program sends (e.g. ["PRICE", "GHG", "EXPORT_PRICE"])
    pub payload_types: Vec<String>,
    /// Report types VTN expects from us (e.g. ["USAGE", "DEMAND"])
    pub report_types: Vec<String>,
    pub currency: Option<String>,  // e.g. "EUR"
    pub units: Option<String>,     // e.g. "KWH"
    pub is_capacity_program: bool, // participates in capacity management
}

/// Internal representation of a received OpenADR event, translated into domain terms (§5.2).
/// Unwired sketch — never constructed anywhere; `dispatch_setpoints` is the storage this
/// would need once DISPATCH_SETPOINT parsing exists. See docs/BACKLOG.md BL-24 and
/// docs/reference/TECHNICAL_DEBTS.md R-13.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct OadrEventCache {
    pub event_id: String,
    pub program_id: String,
    pub event_name: Option<String>,
    pub received_at: Option<DateTime<Utc>>,

    // Translated content
    pub rate_snapshots: Vec<TariffSnapshot>, // PRICE, EXPORT_PRICE, GHG per interval
    pub capacity_limits: Vec<TariffSnapshot>, // IMPORT/EXPORT_CAPACITY_LIMIT per interval
    pub alert_type: Option<String>,          // e.g. "ALERT_GRID_EMERGENCY"
    pub alert_message: Option<String>,
    pub dispatch_setpoints: Vec<PowerSnapshot>, // DISPATCH_SETPOINT per interval

    pub raw: serde_json::Value, // original OpenADR event JSON
}

/// Pending report obligation derived from OpenADR event's reportDescriptors (§5.3).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OadrReportObligation {
    pub id: Uuid,
    pub event_id: String,
    pub program_id: Option<String>,
    /// e.g. "USAGE", "DEMAND", "USAGE_FORECAST", "STORAGE_CHARGE_STATE"
    pub payload_type: String,
    /// e.g. "DIRECT_READ", "FORECAST"
    pub reading_type: String,
    pub resource_name: Option<String>,
    pub due_at: DateTime<Utc>,
    pub interval_duration_s: u64,
    pub fulfilled: bool,
    pub created_at: DateTime<Utc>,
}

impl OadrReportObligation {
    /// True if the obligation is unfulfilled and its due time has passed.
    pub fn is_due(&self, now: DateTime<Utc>) -> bool {
        !self.fulfilled && now >= self.due_at
    }
}

/// A capacity reservation request sent to the VTN (Stage 2+).
/// Unwired sketch — never constructed anywhere. See docs/BACKLOG.md BL-24.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OadrCapacityRequest {
    pub program_id: String,
    pub requested_import_kw: Option<f64>,
    pub requested_export_kw: Option<f64>,
    pub time_window_start: DateTime<Utc>,
    pub time_window_end: DateTime<Utc>,
    pub reason: String,
}
