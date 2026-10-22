//! `SimState::to_sensor_snapshot`/`to_sim_snapshot`/`to_timeline_snapshot` —
//! split into their own file to keep `simulator/mod.rs` under the file-size
//! cap; behave as ordinary `impl SimState` methods.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::assets::AssetState;
use crate::controller::sim_snapshot::{AssetSnapshot, GridSnapshot, SimSnapshot};
use crate::entities::sensor::{SensorRaw, SensorSnapshot};
use crate::entities::timeline::{TimelineAssetData, TimelinePoint, TimelineSnapshot};

use super::SimState;

impl SimState {
    /// Build a SensorSnapshot for backward compatibility with /sensors endpoint.
    /// The sensor has one temperature: the first heater's in roster order, found by its state, not
    /// its id (W7).
    pub fn to_sensor_snapshot(&self) -> SensorSnapshot {
        let temp_c = self.assets.iter().find_map(|e| match &e.state {
            AssetState::Heater(s) => Some(s.temperature_c),
            _ => None,
        });
        SensorSnapshot {
            id: Uuid::new_v4(),
            ts: self.last_tick,
            temperature_c: temp_c,
            power_kw: Some(self.grid.net_power_kw),
            voltage_v: Some(self.grid.voltage_v),
            raw: SensorRaw {
                source: Some("simulator".into()),
                import_kw: Some(self.grid.import_kw),
                export_kw: Some(self.grid.export_kw),
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
        for (roster_index, (entry, cfg)) in self.iter_assets().enumerate() {
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
                    asset_type: cfg.kind(),
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
                    embodied_co2_g_kwh: cfg.embodied_co2_g_kwh(),
                    emergency_what_ifs: cfg.emergency_what_ifs(&entry.state),
                    ac_ceiling_kw: cfg.ac_ceiling_kw(),
                    arbiter_lever: cfg.arbiter_lever(),
                    surplus_claim_rank: cfg.surplus_claim_rank(),
                    full_effort_time_to_target_s: cfg.full_effort_time_to_target_s(&entry.state),
                    dispatch_without_request: cfg.dispatch_without_request(&entry.state),
                    roster_index,
                },
            );
        }

        SimSnapshot {
            ts: self.last_tick,
            grid: GridSnapshot {
                net_power_kw: self.grid.net_power_kw,
                voltage_v: self.grid.voltage_v,
                import_kwh: self.grid.import_kwh,
                export_kwh: self.grid.export_kwh,
                import_limit_kw: self.grid_asset.state.import_limit_kw,
                export_limit_kw: self.grid_asset.state.export_limit_kw,
            },
            assets: assets_map,
        }
    }

    /// Each `Thermostat` asset's own setpoint while a user comfort target is held
    /// (`heater_setpoint_c` inject); empty when no target is set. The target itself reaches the
    /// asset as its ceiling (`TickInputs::heater_target_c_override`), where it cuts out.
    pub fn thermostat_setpoints_kw(&self, target_c: Option<f64>) -> HashMap<String, f64> {
        if target_c.is_none() {
            return HashMap::new();
        }
        self.iter_assets()
            .filter_map(|(entry, cfg)| {
                cfg.as_thermostat()
                    .map(|t| (entry.id.clone(), t.thermostat_setpoint_kw()))
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
    use crate::entities::asset_params::{AssetParams, HeaterParams};
    use crate::entities::sensor::SensorRaw;
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
        assert!(wire["import_kw"].is_number() && wire["export_kw"].is_number());
        assert_eq!(wire.as_object().unwrap().len(), 3);

        assert_eq!(
            serde_json::to_value(SensorRaw::default()).unwrap(),
            serde_json::json!({})
        );
    }

    /// `/sim` lists an asset with its declared kind, whatever its id.
    #[test]
    fn sim_snapshot_carries_the_declared_kind_not_the_id() {
        let sim = SimState::from_params(
            &[AssetParams::Ev(crate::entities::asset_params::EvParams {
                id: "car-a".into(),
                ..Default::default()
            })],
            chrono::Utc::now(),
        );
        let snap = sim.to_sim_snapshot();
        assert_eq!(
            snap.assets["car-a"].asset_type,
            crate::entities::asset::AssetKind::Ev
        );
        let wire = serde_json::to_value(&snap.assets["car-a"]).unwrap();
        assert_eq!(wire["asset_type"], "ev");
    }

    /// `/sensors` reports the heater's temperature whatever the heater's id (W7).
    #[test]
    fn sensor_snapshot_reads_the_heater_by_its_type_not_its_id() {
        let sim = SimState::from_params(
            &[AssetParams::Heater(HeaterParams {
                id: "boiler".into(),
                temp_initial_c: 47.0,
                ..Default::default()
            })],
            chrono::Utc::now(),
        );
        assert_eq!(sim.to_sensor_snapshot().temperature_c, Some(47.0));
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
            Some(&3.0),
            "the target is the cut-out ceiling, not a comparison made here"
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
