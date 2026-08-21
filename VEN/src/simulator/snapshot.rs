//! `SimState::to_sensor_snapshot`/`to_sim_snapshot`/`to_timeline_snapshot` —
//! split into their own file to keep `simulator/mod.rs` under the file-size
//! cap; behave as ordinary `impl SimState` methods.

use std::collections::HashMap;

use chrono::Utc;
use uuid::Uuid;

use crate::assets::AssetState;
use crate::controller::simulator_port::{AssetSnapshot, GridSnapshot, SimSnapshot};
use crate::entities::asset::AssetType;
use crate::entities::timeline::{
    HeaterPlanTrajectory, TimelineAssetData, TimelinePoint, TimelineSnapshot,
};
use crate::models::SensorSnapshot;

use super::{AssetConfig, SimState};

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
            let (available_discharge_kwh, available_charge_kwh) =
                match cfg.available_storage_kwh(&entry.state) {
                    Some((dis, ch)) => (Some(dis), Some(ch)),
                    None => (None, None),
                };
            let asset_type = match cfg {
                AssetConfig::Battery(_) => "battery",
                AssetConfig::Ev(_) => "ev",
                AssetConfig::Heater(_) => "heater",
                AssetConfig::Pv(_) => "pv",
                AssetConfig::BaseLoad(_) => "base_load",
            }
            .to_string();
            assets_map.insert(
                entry.id.clone(),
                AssetSnapshot {
                    power_kw: entry.last_power_kw,
                    asset_type,
                    cap_max_import_kw: cap.max_import_kw,
                    cap_max_export_kw: cap.max_export_kw,
                    available_discharge_kwh,
                    available_charge_kwh,
                    default_setpoint_kw: cfg.default_setpoint(&entry.state),
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
                let asset_type = match cfg {
                    AssetConfig::Battery(_) => AssetType::Battery,
                    AssetConfig::Ev(_) => AssetType::Ev,
                    AssetConfig::Heater(_) => AssetType::Heater,
                    AssetConfig::Pv(_) => AssetType::Pv,
                    AssetConfig::BaseLoad(_) => AssetType::GenericConsumer,
                };
                let plan_trajectory = match (cfg, &entry.state) {
                    (AssetConfig::Heater(h), AssetState::Heater(s)) => {
                        let e_max_kwh = (h.temp_max_c - h.temp_min_c) * h.thermal_mass_kwh_per_c;
                        let e_kwh = ((s.temperature_c - h.temp_min_c) * h.thermal_mass_kwh_per_c)
                            .clamp(0.0, e_max_kwh);
                        Some(HeaterPlanTrajectory {
                            e_kwh,
                            temp_min_c: h.temp_min_c,
                            thermal_mass: h.thermal_mass_kwh_per_c,
                            q_dem_kw: h.forecast_demand_kw(h.ambient_temp_c),
                            e_max_kwh,
                        })
                    }
                    _ => None,
                };
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
