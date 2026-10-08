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
    pub raw: SensorRaw,
}

/// What a sensor reading carries beyond its typed values: where it came from and, from the
/// simulator, the meter's import and export. Any other key a `POST /sensors` client sends is kept
/// as it came (`extra`) and echoed back; the VEN never reads it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SensorRaw {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub import_w: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub export_w: Option<f64>,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl SensorSnapshot {
    /// A sensor snapshot nobody has written yet. Stamped at the epoch rather
    /// than "now": the value has no reading time, and a fresh-looking `ts`
    /// would be a made-up one (and a hidden wall-clock read).
    pub fn never_sampled() -> Self {
        Self {
            id: Uuid::new_v4(),
            ts: DateTime::<Utc>::UNIX_EPOCH,
            temperature_c: None,
            power_w: None,
            voltage_v: None,
            raw: SensorRaw::default(),
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
pub struct SensorInput {
    pub temperature_c: Option<f64>,
    pub power_w: Option<f64>,
    pub voltage_v: Option<f64>,
    pub raw: Option<SensorRaw>,
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
            raw: SensorRaw {
                source: Some("simulator".into()),
                import_w: Some(self.grid.import_w),
                export_w: Some(self.grid.export_w),
                extra: Default::default(),
            },
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
                    forced_power_kw: cfg.forced_power_kw(&entry.state),
                    response: cap.response.clone(),
                    default_setpoint_kw: cfg.default_setpoint(),
                    setpoint_kw: entry.setpoint_kw,
                    values,
                    history: cfg.history_view(&entry.state),
                    emergency_what_ifs: cfg.emergency_what_ifs(&entry.state),
                    ac_ceiling_kw: cfg.ac_ceiling_kw(),
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

    /// Each `Thermostat` asset's own setpoint for a user comfort target
    /// (`heater_setpoint_c` inject); empty when no target is set. The
    /// dispatcher applies these instead of deciding on/off from temperatures.
    pub fn thermostat_setpoints_kw(&self, target_c: Option<f64>) -> HashMap<String, f64> {
        let Some(target_c) = target_c else {
            return HashMap::new();
        };
        self.iter_assets()
            .filter_map(|(entry, cfg)| {
                cfg.as_thermostat().map(|t| {
                    (
                        entry.id.clone(),
                        t.thermostat_setpoint_kw(&entry.state, target_c),
                    )
                })
            })
            .collect()
    }

    /// Build a domain-only `TimelineSnapshot`. All infra→domain conversions happen here
    /// before the sim lock is released; no `AssetHistoryBuffer`/`AssetConfig`/`AssetState`
    /// escapes to the domain layer.
    pub fn to_timeline_snapshot(&self, now: DateTime<Utc>) -> TimelineSnapshot {
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

#[cfg(test)]
mod tests {
    use super::SensorRaw;
    use crate::entities::asset_params::{AssetParams, HeaterParams};
    use crate::simulator::SimState;

    /// R-114: the typed `raw` keeps the JSON it replaced - the simulator's three keys, a
    /// client's own keys as they came, and `{}` for nothing.
    #[test]
    fn sensor_raw_round_trips_the_json_it_replaced() {
        let client = serde_json::json!({"source": "test", "probe": {"id": 7}, "rssi": -61});
        let raw: SensorRaw = serde_json::from_value(client.clone()).unwrap();
        assert_eq!(raw.source.as_deref(), Some("test"));
        assert_eq!(serde_json::to_value(&raw).unwrap(), client);

        let sim = SimState::from_params(&[], chrono::Utc::now()).to_sensor_snapshot();
        let wire = serde_json::to_value(&sim.raw).unwrap();
        assert_eq!(wire["source"], "simulator");
        assert!(wire["import_w"].is_number() && wire["export_w"].is_number());
        assert_eq!(wire.as_object().unwrap().len(), 3);

        assert_eq!(
            serde_json::to_value(SensorRaw::default()).unwrap(),
            serde_json::json!({})
        );
    }

    #[test]
    fn thermostat_setpoints_come_from_each_thermostat_asset() {
        let now = chrono::Utc::now();
        let sim = SimState::from_params(
            &[AssetParams::Heater(HeaterParams {
                id: crate::ids::ASSET_HEATER.to_string(),
                temp_initial_c: 20.0,
                max_kw: 3.0,
                ..Default::default()
            })],
            now,
        );
        let heater = crate::ids::ASSET_HEATER;
        assert_eq!(
            sim.thermostat_setpoints_kw(Some(22.0)).get(heater),
            Some(&3.0)
        );
        assert_eq!(
            sim.thermostat_setpoints_kw(Some(18.0)).get(heater),
            Some(&0.0)
        );
        assert!(sim.thermostat_setpoints_kw(None).is_empty());
    }

    #[test]
    fn sim_snapshot_carries_the_assets_own_forced_power() {
        // Controllers read the heater's thermostat override from the snapshot
        // instead of re-deriving it from temperatures.
        let now = chrono::Utc::now();
        let sim = SimState::from_params(
            &[AssetParams::Heater(HeaterParams {
                id: crate::ids::ASSET_HEATER.to_string(),
                temp_initial_c: 10.0, // far below temp_min_c: emergency heat
                ..Default::default()
            })],
            now,
        );
        let snap = sim.to_sim_snapshot();
        let heater = &snap.assets[crate::ids::ASSET_HEATER];
        let max_kw = heater.val("max_kw").unwrap();
        assert_eq!(heater.forced_power_kw, Some(max_kw));
    }
}
