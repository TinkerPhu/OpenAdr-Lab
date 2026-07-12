use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::entities::design_vocabulary::UserRequestMode;

/// A device-centric EV charging session.
/// Only carries user intent — sim state (current_soc, plugged) is
/// injected at solve time from `SimState::ev_state()`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvSession {
    pub id: Uuid,
    /// Target SoC (0.0–1.0). E.g. 0.80 = "charge to 80%".
    pub target_soc: f64,
    /// When the EV must be ready (departure time).
    pub departure_time: DateTime<Utc>,
    /// If true, MILP treats as MayRun (soft reward, best-effort by departure).
    /// If false (default), MustRun (hard constraint, must reach target SoC by departure).
    #[serde(default)]
    pub soft_deadline: bool,
    /// How the user expressed this request (BL-28); BY_DEADLINE = legacy behaviour.
    #[serde(default)]
    pub mode: UserRequestMode,
    /// MAX_COST (WP4.1-c): total charging-cost ceiling [€]. None = no cap.
    #[serde(default)]
    pub budget_eur: Option<f64>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// A device-centric heater temperature target.
/// Only carries user intent — sim state (current_temp_c) is
/// injected at solve time from `SimState::heater_state()`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeaterTarget {
    pub id: Uuid,
    /// Desired water/room temperature in °C.
    pub target_temp_c: f64,
    /// When the target temperature must be reached.
    pub ready_by: DateTime<Utc>,
    /// How the user expressed this request (BL-28); BY_DEADLINE = legacy behaviour.
    #[serde(default)]
    pub mode: UserRequestMode,
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
    /// How the user expressed this request (BL-28); BY_DEADLINE = legacy behaviour.
    #[serde(default)]
    pub mode: UserRequestMode,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Runtime state for an active shiftable load (e.g. washing machine running).
///
/// Created when the dispatcher detects a plan-slot allocation for a shiftable
/// load. Tracks countdown until the load finishes; NOT a physics sim asset.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShiftableLoadRuntime {
    /// FK → ShiftableLoad.id
    pub load_id: Uuid,
    /// Asset identifier (e.g. "wm").
    pub asset_id: String,
    /// Fixed power level while running [kW].
    pub power_kw: f64,
    /// When the load was started.
    pub started_at: DateTime<Utc>,
    /// When the load will finish (started_at + duration_min).
    pub ends_at: DateTime<Utc>,
}

impl ShiftableLoadRuntime {
    pub fn is_running(&self, now: DateTime<Utc>) -> bool {
        now >= self.started_at && now < self.ends_at
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entities::design_vocabulary::UserRequestMode;

    /// Payloads from before the mode field existed must default to BY_DEADLINE
    /// (today's implicit behaviour).
    #[test]
    fn test_ev_session_deserialize_missing_mode_defaults_by_deadline() {
        let json = r#"{
            "id": "3fa85f64-5717-4562-b3fc-2c963f66afa6",
            "target_soc": 0.8,
            "departure_time": "2026-07-12T06:00:00Z",
            "created_at": "2026-07-11T20:00:00Z",
            "updated_at": "2026-07-11T20:00:00Z"
        }"#;
        let s: EvSession = serde_json::from_str(json).unwrap();
        assert_eq!(s.mode, UserRequestMode::ByDeadline);
    }

    /// Mode survives a serde roundtrip in SCREAMING_SNAKE_CASE wire form.
    #[test]
    fn test_ev_session_serde_roundtrip_preserves_mode() {
        let session = EvSession {
            id: Uuid::new_v4(),
            target_soc: 0.9,
            departure_time: Utc::now(),
            soft_deadline: false,
            mode: UserRequestMode::Opportunistic,
            budget_eur: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        let json = serde_json::to_string(&session).unwrap();
        assert!(json.contains("\"OPPORTUNISTIC\""));
        let back: EvSession = serde_json::from_str(&json).unwrap();
        assert_eq!(back.mode, UserRequestMode::Opportunistic);
    }

    #[test]
    fn test_heater_target_deserialize_missing_mode_defaults_by_deadline() {
        let json = r#"{
            "id": "3fa85f64-5717-4562-b3fc-2c963f66afa6",
            "target_temp_c": 55.0,
            "ready_by": "2026-07-12T06:00:00Z",
            "created_at": "2026-07-11T20:00:00Z",
            "updated_at": "2026-07-11T20:00:00Z"
        }"#;
        let t: HeaterTarget = serde_json::from_str(json).unwrap();
        assert_eq!(t.mode, UserRequestMode::ByDeadline);
    }

    #[test]
    fn test_shiftable_load_deserialize_missing_mode_defaults_by_deadline() {
        let json = r#"{
            "id": "3fa85f64-5717-4562-b3fc-2c963f66afa6",
            "asset_id": "wm",
            "power_kw": 2.0,
            "duration_min": 60,
            "earliest_start": "2026-07-11T20:00:00Z",
            "latest_end": "2026-07-12T06:00:00Z",
            "created_at": "2026-07-11T20:00:00Z",
            "updated_at": "2026-07-11T20:00:00Z"
        }"#;
        let l: ShiftableLoad = serde_json::from_str(json).unwrap();
        assert_eq!(l.mode, UserRequestMode::ByDeadline);
    }
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
