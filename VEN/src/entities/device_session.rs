use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A device-centric EV charging session.
///
/// Replaces the generic `EnergyPacket` for EV scheduling.
/// Only carries user intent — sim state (current_soc, plugged) is
/// injected at solve time from `SimState::ev_state()`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvSession {
    pub id: Uuid,
    /// Target SoC (0.0–1.0). E.g. 0.80 = "charge to 80%".
    pub target_soc: f64,
    /// When the EV must be ready (departure time).
    pub departure_time: DateTime<Utc>,
    /// If true, MILP treats as MayRun (soft reward); if false, MustRun (hard constraint).
    #[serde(default)]
    pub opportunistic: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// A device-centric heater temperature target.
///
/// Replaces the generic `EnergyPacket` for heater scheduling.
/// Only carries user intent — sim state (current_temp_c) is
/// injected at solve time from `SimState::heater_state()`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeaterTarget {
    pub id: Uuid,
    /// Desired water/room temperature in °C.
    pub target_temp_c: f64,
    /// When the target temperature must be reached.
    pub ready_by: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
