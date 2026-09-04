//! `SimState::to_sensor_snapshot`/`to_sim_snapshot`/`to_timeline_snapshot` —
//! split into their own file to keep `simulator/mod.rs` under the file-size
//! cap; behave as ordinary `impl SimState` methods.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::assets::AssetState;
use crate::controller::simulator_port::{AssetSnapshot, GridSnapshot, SimSnapshot};
use crate::entities::timeline::{TimelineAssetData, TimelinePoint, TimelineSnapshot};

use super::SimState;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SensorSnapshot {
    pub id: Uuid,
    pub ts: DateTime<Utc>,
    pub temperature_c: Option<f64>,
    pub power_w: Option<f64>,
    pub voltage_v: Option<f64>,
    pub raw: serde_json::Value,
}

impl SensorSnapshot {
    pub fn empty_now() -> Self {
        Self {
            id: Uuid::new_v4(),
            ts: Utc::now(),
            temperature_c: None,
            power_w: None,
            voltage_v: None,
            raw: serde_json::json!({}),
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
pub struct SensorInput {
    pub temperature_c: Option<f64>,
    pub power_w: Option<f64>,
    pub voltage_v: Option<f64>,
    pub raw: Option<serde_json::Value>,
}

impl SimState {
    /// Build a SensorSnapshot for backward compatibility with /sensors endpoint.
    pub fn to_sensor_snapshot(&self) -> SensorSnapshot {
        let temp_c = self.asset(crate::ids::ASSET_HEATER).and_then(|e| {
            if let AssetState::Heater(s) = &e.state {
                Some(s.temperature_c)
            } else {
                None
            }
        });
        SensorSnapshot {
            id: Uuid::new_v4(),
            ts: self.last_tick,
            temperature_c: temp_c,
            power_w: Some(self.grid.net_power_w),
            voltage_v: Some(self.grid.voltage_v),
            raw: serde_json::json!({
                "source": "simulator",
                "import_w": self.grid.import_w,
                "export_w": self.grid.export_w,
            }),
        }
    }

    /// Build a SimSnapshot for the /sim endpoint and for controller functions.
    ///
    /// Extended fields (cap_max_import_kw, cap_max_export_kw, etc.) are precomputed here
    /// so that controller logic never needs to import `SimState` or `AssetConfig`.
    pub fn to_sim_snapshot(&self) -> SimSnapshot {
        let mut assets_map = HashMap::new();
        for (entry, cfg) in self.iter_assets() {
            let values = cfg.state_values(&entry.state);
            let cap = cfg.capability(&entry.state);
            let (available_discharge_kwh, available_charge_kwh) = match cfg
                .as_request_resolvable()
                .and_then(|r| r.available_storage_kwh(&entry.state))
            {
                Some((dis, ch)) => (Some(dis), Some(ch)),
                None => (None, None),
            };
            assets_map.insert(
                entry.id.clone(),
                AssetSnapshot {
                    power_kw: entry.last_power_kw,
                    asset_type: cfg.asset_type_str().to_string(),
                    cap_max_import_kw: cap.max_import_kw,
                    cap_max_export_kw: cap.max_export_kw,
                    available_discharge_kwh,
                    available_charge_kwh,
                    default_setpoint_kw: cfg.default_setpoint(),
                    setpoint_kw: entry.setpoint_kw,
                    values,
                },
            );
        }

        SimSnapshot {
            ts: self.last_tick,
            grid: GridSnapshot {
                net_power_w: self.grid.net_power_w,
                voltage_v: self.grid.voltage_v,
                import_kwh: self.grid.import_kwh,
                export_kwh: self.grid.export_kwh,
                import_limit_kw: self.grid_asset.state.import_limit_kw,
                export_limit_kw: self.grid_asset.state.export_limit_kw,
            },
            assets: assets_map,
        }
    }

    /// Build a domain-only `TimelineSnapshot`. All infra→domain conversions happen here
    /// before the sim lock is released; no `AssetHistoryBuffer`/`AssetConfig`/`AssetState`
    /// escapes to the domain layer.
    pub fn to_timeline_snapshot(&self) -> TimelineSnapshot {
        let now = Utc::now();
        let w = chrono::Duration::seconds(3600);
        let assets = self
            .iter_assets()
            .map(|(entry, cfg)| {
                let history: Vec<TimelinePoint> = entry
                    .history
                    .slice(w, now)
                    .into_iter()
                    .map(|p| TimelinePoint {
                        ts: p.ts,
                        power_kw: p.power_kw,
                        state_values: cfg.state_values(&p.state),
                    })
                    .collect();
                let current_power_kw = entry
                    .history
                    .recent_avg_power(chrono::Duration::seconds(60), now)
                    .unwrap_or_else(|| entry.history.latest().map(|p| p.power_kw).unwrap_or(0.0));
                let current_state_values = cfg.state_values(&entry.state);
                let asset_type = cfg.asset_type();
                // Was a third inline copy of Heater's plan-trajectory math
                // (alongside `Heater::plan_trajectory` and
                // `Thermostat::plan_trajectory`) — now just calls through the
                // capability trait, same as everywhere else that needs it.
                let plan_trajectory = cfg
                    .as_thermostat()
                    .and_then(|t| t.plan_trajectory(&entry.state));
                (
                    entry.id.clone(),
                    TimelineAssetData {
                        asset_id: entry.id.clone(),
                        asset_type,
                        history,
                        current_power_kw,
                        current_state_values,
                        plan_trajectory,
                    },
                )
            })
            .collect();
        let grid_history: Vec<TimelinePoint> = self
            .grid_asset
            .history
            .slice(w, now)
            .into_iter()
            .map(|p| TimelinePoint {
                ts: p.ts,
                power_kw: p.power_kw,
                state_values: HashMap::new(),
            })
            .collect();
        let grid_current_kw = self
            .grid_asset
            .history
            .latest()
            .map(|p| p.power_kw)
            .unwrap_or(0.0);
        TimelineSnapshot {
            assets,
            grid_history,
            grid_current_kw,
        }
    }
}
