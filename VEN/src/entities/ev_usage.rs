//! What an EV tells the diagnostics surface about its configured usage schedule
//! (`GET /ev-usage-sim`). Field names are the wire names; the asset builds it.

use chrono::{DateTime, Utc};
use serde::Serialize;

use super::asset_params::EvUsageMode;

/// One upcoming (or in-progress) simulated trip.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct NextTrip {
    pub leave_at: DateTime<Utc>,
    pub return_at: DateTime<Utc>,
    pub expected_soc_drop_pct: f64,
}

/// Serves both usage classes — `usage_sim` and `usage_forecast` share one schedule shape,
/// so they share one diagnostics view, distinguished by `mode` rather than by a sibling route.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct EvUsageSimState {
    pub mode: EvUsageMode,
    pub engage_charge_planning: bool,
    pub next_trip: Option<NextTrip>,
}
