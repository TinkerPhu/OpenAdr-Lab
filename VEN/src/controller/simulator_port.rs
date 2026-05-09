/// SimulatorPort trait — the boundary between controller logic and the physics simulator.
///
/// All 6 named controller functions (build_setpoints, apply_surplus_ev_overlay,
/// apply_battery_correction_overlay, apply_deviation_absorption, record_tick,
/// compute_envelope) accept `&SimSnapshot` or `&dyn SimulatorPort` rather than `&SimState`.
/// This allows unit testing without a running simulator.
use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Abstraction over the physics simulator for controller logic.
pub trait SimulatorPort: Send + Sync {
    /// Take a point-in-time snapshot of simulator state.
    fn snapshot(&self) -> Result<SimSnapshot, SnapshotError>;
    /// Push override state into the simulator (e.g., from UI injections).
    fn inject(&self, state: SimInjectState);
}

/// Point-in-time snapshot of all simulator state needed by controller logic.
#[derive(Debug, Clone, Serialize)]
pub struct SimSnapshot {
    pub ts: DateTime<Utc>,
    pub grid: GridSnapshot,
    /// Asset snapshots keyed by asset ID.
    pub assets: HashMap<String, AssetSnapshot>,
}

/// Grid meter snapshot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GridSnapshot {
    pub net_power_w: f64,
    pub voltage_v: f64,
    pub import_kwh: f64,
    pub export_kwh: f64,
}

/// Per-asset snapshot with precomputed capability fields.
///
/// Extended beyond the minimal `{ power_kw, values }` shape to support all
/// controller functions without importing `SimState` or `AssetConfig`.
///
/// When serialized, `values` keys are flattened into the same JSON object as the typed fields,
/// preserving backward-compatibility with the `/sim` API response.
#[derive(Debug, Clone, Serialize)]
pub struct AssetSnapshot {
    /// Actual power from last tick (kW). Positive = import, negative = export.
    pub power_kw: f64,
    /// Asset type discriminant for type-based dispatch.
    /// Values: `"battery"`, `"ev"`, `"heater"`, `"pv"`, `"base_load"`.
    pub asset_type: String,
    /// Physical capability: maximum import rate (kW) from config + state.
    pub cap_max_import_kw: f64,
    /// Physical capability: maximum export rate (kW, magnitude) from config + state.
    pub cap_max_export_kw: f64,
    /// Available energy for discharge (kWh), if applicable (battery/EV only).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub available_discharge_kwh: Option<f64>,
    /// Available energy for charge (kWh), if applicable (battery/EV only).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub available_charge_kwh: Option<f64>,
    /// Default setpoint for this asset (kW) when no plan is active.
    pub default_setpoint_kw: f64,
    /// Last setpoint applied by the VEN controller (kW).
    pub setpoint_kw: f64,
    /// Asset-specific state values from `cfg.state_values()` (soc, temp_c, etc.).
    /// Flattened into the top-level JSON object for backward-compatibility.
    #[serde(flatten)]
    pub values: HashMap<String, f64>,
}

impl AssetSnapshot {
    /// Convenience: get a value from the state `values` map.
    #[inline]
    pub fn val(&self, key: &str) -> Option<f64> {
        self.values.get(key).copied()
    }
}

/// State overrides for the `inject()` port method (used by MockSimulatorPort and tests).
///
/// This is the controller-side inject state type. The UI layer uses the richer
/// `crate::state::SimInjectState` which is mapped to this type before calling `inject()`.
#[derive(Debug, Clone)]
pub struct SimInjectState {
    pub ambient_temp_c_override: Option<f64>,
    pub pv_irradiance_override: Option<f64>,
    pub base_load_kw_override: Option<f64>,
    pub ev_plugged_override: Option<bool>,
    pub ev_soc_target_override: Option<f64>,
    pub pv_alpha: f64,
    pub base_load_alpha: f64,
}

/// Error returned by `SimulatorPort::snapshot()`.
#[derive(Debug, Clone, thiserror::Error)]
pub enum SnapshotError {
    #[error("simulator not yet initialized")]
    Uninitialized,
    #[error("transient error; retry on next poll")]
    Transient,
    #[error("fatal simulator error")]
    Fatal,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn _assert_send_sync<T: Send + Sync>() {}

    #[test]
    fn snapshot_error_is_send_sync() {
        _assert_send_sync::<SnapshotError>();
    }

    #[test]
    fn asset_snapshot_val_helper() {
        let mut values = HashMap::new();
        values.insert("soc".to_string(), 0.5_f64);
        let snap = AssetSnapshot {
            power_kw: 0.0,
            asset_type: "battery".to_string(),
            cap_max_import_kw: 5.0,
            cap_max_export_kw: 5.0,
            available_discharge_kwh: Some(2.0),
            available_charge_kwh: Some(5.0),
            default_setpoint_kw: 0.0,
            setpoint_kw: 0.0,
            values,
        };
        assert_eq!(snap.val("soc"), Some(0.5));
        assert_eq!(snap.val("nonexistent"), None);
    }
}
