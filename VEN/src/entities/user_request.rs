/// Stage 5 — UserRequest: the user-facing representation of an energy task.
///
/// A UserRequest is a higher-level object than EnergyPacket — it captures
/// the user's intent (deadline tiers, budget) and links to the generated packet.
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

/// A user-originated energy task request, linking to an EnergyPacket.
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
    pub packet_id: Uuid,                 // linked EnergyPacket
    pub status: UserRequestStatus,
    pub estimated_cost_eur: f64,
    pub estimated_co2_g: f64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
