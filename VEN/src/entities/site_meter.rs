use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

/// A point-in-time snapshot of site-level power measurements.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PowerSnapshot {
    pub ts: DateTime<Utc>,
    pub net_import_w: f64,
    pub import_w: f64,
    pub export_w: f64,
}

/// Site-level meter aggregating all asset measurements.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SiteMeter {
    /// Current net import (kW)
    pub net_import_kw: f64,
    /// Current gross import (kW)
    pub import_kw: f64,
    /// Current gross export (kW)
    pub export_kw: f64,
    /// Per-asset current power (asset_id → kW, positive = import)
    pub asset_power: HashMap<String, f64>,
    pub last_updated: Option<DateTime<Utc>>,
}

/// What the dispatcher is currently targeting for each asset.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DispatchState {
    /// Current plan slot index being dispatched
    pub current_slot: Option<usize>,
    /// Per-asset commanded power (asset_id → kW)
    pub commanded_power: HashMap<String, f64>,
    /// Per-asset actual power (asset_id → kW)
    pub actual_power: HashMap<String, f64>,
    /// Net deviation from plan (kW)
    pub net_deviation_kw: f64,
    pub last_dispatch: Option<DateTime<Utc>>,
}

/// Tracks one active packet's execution on a device.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceSession {
    pub packet_id: Uuid,
    pub asset_id: String,
    pub started_at: DateTime<Utc>,
    pub accumulated_energy_kwh: f64,
    pub accumulated_cost_eur: f64,
    pub accumulated_co2_kg: f64,
    pub ended_at: Option<DateTime<Utc>>,
    pub end_reason: Option<String>,
}
