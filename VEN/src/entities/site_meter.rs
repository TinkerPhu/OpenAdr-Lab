use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

use crate::entities::asset::DeviceResponsiveness;

/// Command issued by the Dispatcher to an asset each dispatch cycle (§7.1).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DispatchCommand {
    pub asset_id: String,
    pub ts: DateTime<Utc>,
    pub commanded_power_kw: f64,
    /// Which EnergyPacket this serves; None if auto-follow or idle
    pub source_packet_id: Option<Uuid>,
    /// e.g. "plan", "auto-follow", "emergency override"
    pub reason: String,
}

/// A point-in-time power measurement (§2.5).
/// Positive = consuming/importing, negative = producing/exporting.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PowerSnapshot {
    pub ts: DateTime<Utc>,
    pub power_kw: f64, // positive = import, negative = export
}

/// The grid connection point meter (§3.9).
/// Measures actual power flow between the site and the grid.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SiteMeter {
    pub meter_id: String,
    pub ts: DateTime<Utc>,               // last measurement time
    pub net_import_kw: f64,              // positive = importing from grid, negative = exporting
    pub voltage_v: Option<f64>,          // grid voltage (optional, for power quality)
    pub frequency_hz: Option<f64>,       // grid frequency (optional)
    pub cumulative_import_kwh: f64,      // total imported energy (meter reading)
    pub cumulative_export_kwh: f64,      // total exported energy (meter reading)
    pub measurement_interval_s: u64,     // how often the meter is read
    pub is_online: bool,                 // meter communication status

    /// Per-asset current power (asset_id → kW, positive = import)
    pub asset_power: HashMap<String, f64>,
}

impl Default for SiteMeter {
    fn default() -> Self {
        Self {
            meter_id: "site".to_string(),
            ts: Utc::now(),
            net_import_kw: 0.0,
            voltage_v: None,
            frequency_hz: None,
            cumulative_import_kwh: 0.0,
            cumulative_export_kwh: 0.0,
            measurement_interval_s: 1,
            is_online: true,
            asset_power: HashMap::new(),
        }
    }
}

/// Snapshot of the current dispatch reality (§7.2).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DispatchState {
    pub ts: Option<DateTime<Utc>>,
    /// Active commands issued to all assets this cycle
    pub commands: Vec<DispatchCommand>,
    /// Measured total site power this cycle (positive = import) (kW)
    pub net_actual_power_kw: f64,
    /// What the plan said for this timestep (kW)
    pub net_planned_power_kw: f64,
    /// = Actual - Planned (kW)
    pub net_deviation_kw: f64,
    /// True if |NetDeviation| > threshold for ReplanTriggerDuration
    pub deviation_significant: bool,

    // Supplementary lookup maps kept for O(1) UI/API access
    /// Per-asset commanded power derived from commands (asset_id → kW)
    #[serde(default)]
    pub commanded_power: HashMap<String, f64>,
    /// Per-asset actual power from SiteMeter (asset_id → kW)
    #[serde(default)]
    pub actual_power: HashMap<String, f64>,
}

/// Tracks one active EnergyPacket's real-time execution on a device (§4.2).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceSession {
    pub session_id: Uuid,
    pub packet_id: Uuid,
    pub asset_id: String,

    pub start_time: DateTime<Utc>,
    pub end_time: Option<DateTime<Utc>>,

    pub commanded_setpoint_kw: f64,       // current setpoint sent to device
    pub measured_power_kw: f64,           // current actual power from device
    pub cumulative_delivered_kwh: f64,    // energy delivered in this session

    pub responsiveness: DeviceResponsiveness,
    /// How many consecutive deviation-above-threshold readings
    pub deviation_count: u32,

    pub end_reason: Option<String>,
}
