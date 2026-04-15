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

/// A shiftable load (e.g. washing machine, heat pump cycle).
///
/// Fixed power level for a fixed duration; the MILP chooses optimal
/// start time within `[earliest_start, latest_end - duration]`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShiftableLoad {
    pub id: Uuid,
    /// Asset identifier (e.g. "wm", "hp").
    pub asset_id: String,
    /// Fixed power level while running [kW].
    pub power_kw: f64,
    /// Total run time [minutes].
    pub duration_min: u32,
    /// Earliest allowed start time.
    pub earliest_start: DateTime<Utc>,
    /// Latest allowed end time (load must finish by this time).
    pub latest_end: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// A single slot in a baseline override.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BaselineSlot {
    /// Start time of the slot (aligned to planning grid).
    pub slot_start: DateTime<Utc>,
    /// Additive power adjustment [kW]. Positive = more load.
    pub add_kw: f64,
}

/// User-specified additive adjustments to the non-controllable baseline.
///
/// E.g. "I know the dishwasher will run 1.5 kW from 14:00–15:00".
/// Applied in `build_milp_inputs()` as `p_base_kw[t] += add_kw`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BaselineOverride {
    pub id: Uuid,
    pub slots: Vec<BaselineSlot>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
