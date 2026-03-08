use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

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

/// Configuration for an OpenADR program this VEN participates in.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OadrProgramConfig {
    pub program_id: String,
    pub program_name: String,
    /// Is this VEN enrolled in this program?
    pub enrolled: bool,
    /// Report descriptors from the program definition
    pub report_descriptors: Vec<serde_json::Value>,
}

/// Cache of active OpenADR events (refreshed by the poll loop).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct OadrEventCache {
    pub events: Vec<serde_json::Value>,
    pub last_fetched: Option<DateTime<Utc>>,
}

/// A pending report obligation derived from an OpenADR event's reportDescriptors.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OadrReportObligation {
    pub id: Uuid,
    pub event_id: String,
    pub program_id: Option<String>,
    /// Report type: "USAGE", "DEMAND", "USAGE_FORECAST", "STORAGE_CHARGE_STATE", …
    pub report_type: String,
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
