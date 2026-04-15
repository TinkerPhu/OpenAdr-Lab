/// Stage 5 — UserRequest: the user-facing representation of an energy task.
///
/// A UserRequest captures the user's intent (deadline tiers, budget) and
/// links to the generated device session (EvSession or HeaterTarget).
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A single deadline tier from the user's request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestDeadline {
    pub latest_end: DateTime<Utc>,
    pub max_total_cost_eur: Option<f64>,
    pub max_marginal_rate_eur_kwh: Option<f64>,
    pub min_completion: f64,
}

/// Status of the overall user request.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum UserRequestStatus {
    Active,    // packet is scheduled or executing
    Completed, // packet reached target
    Cancelled, // user cancelled via DELETE /requests/:id
    Failed,    // packet failed
}

/// A user-originated energy task request, linking to a device session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserRequest {
    pub id: Uuid,
    pub asset_id: String,
    pub target_soc: Option<f64>,
    pub target_energy_kwh: f64,
    pub desired_power_kw: f64,
    pub deadlines: Vec<RequestDeadline>,
    pub completion_policy: String,
    pub max_total_cost_eur: Option<f64>, // from first tier (for API convenience)
    pub tier_count: usize,               // number of deadline tiers
    pub session_id: Option<Uuid>,        // linked DeviceSession (EvSession or HeaterTarget)
    pub status: UserRequestStatus,
    pub estimated_cost_eur: f64,
    pub estimated_co2_g: f64,
    // ── Leeway fields (§8.2 / ven_asset_interface_spec §7) ──────────────────
    pub interruptible: bool,        // planner may pause/resume this packet
    pub tolerance_min: Option<i64>, // ±N minutes around deadline acceptable
    pub budget_eur: Option<f64>,    // max the user is willing to pay (top-level ceiling)
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
