//! `SimHandle`: the shared simulator behind the ports the application and adapter rings use.
//!
//! Owns the `Arc<Mutex<SimState>>` the tick loop and the other tasks share (they keep the
//! concrete type), and implements the domain-typed ports on top of it. Every method locks for
//! the length of that one call and releases before returning; nothing hands out a guard, so a
//! caller cannot hold the simulator across an `.await`.

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::Mutex;

use super::SimState;
use crate::assets::ShiftableLoadAsset;
use crate::controller::headroom_port::HeadroomPort;
use crate::controller::sim_roster_port::{CancelOutcome, SimRosterPort};
use crate::entities::capacity_curve::CapacityCurves;
use crate::entities::device_session::ShiftableLoad;
use crate::entities::plan::{Plan, SiteFlexibilityEnvelope};
use chrono::{DateTime, Utc};

#[derive(Clone)]
pub struct SimHandle {
    sim: Arc<Mutex<SimState>>,
}

impl SimHandle {
    pub fn new(sim: Arc<Mutex<SimState>>) -> Self {
        Self { sim }
    }
}

#[async_trait]
impl SimRosterPort for SimHandle {
    async fn add_shiftable(&self, load: &ShiftableLoad) -> Result<(), String> {
        self.sim.lock().await.add_shiftable(
            &load.asset_id,
            ShiftableLoadAsset {
                power_kw: load.power_kw,
                duration_min: load.duration_min,
                earliest_start: load.earliest_start,
                latest_end: load.latest_end,
            },
        )
    }

    async fn cancel_if_cancellable(&self, asset_id: &str) -> CancelOutcome {
        let mut sim = self.sim.lock().await;
        match sim.find_asset(asset_id) {
            None => CancelOutcome::NoSuchAsset,
            Some((entry, cfg)) if !cfg.is_cancellable(&entry.state) => {
                CancelOutcome::NotCancellable
            }
            Some(_) => {
                sim.remove_asset(asset_id);
                CancelOutcome::Removed
            }
        }
    }

    async fn reset_asset(&self, asset_id: &str, values: HashMap<String, f64>) -> bool {
        let mut sim = self.sim.lock().await;
        match sim.find_asset_mut(asset_id) {
            Some((entry, cfg)) => {
                cfg.reset(&mut entry.state, values);
                true
            }
            None => false,
        }
    }

    async fn update_asset_config(&self, asset_id: &str, values: HashMap<String, f64>) -> bool {
        let mut sim = self.sim.lock().await;
        match sim.find_asset_mut(asset_id) {
            Some((_entry, cfg)) => {
                cfg.update_config(values);
                true
            }
            None => false,
        }
    }
}

#[async_trait]
impl HeadroomPort for SimHandle {
    async fn site_headroom(
        &self,
        now: DateTime<Utc>,
        phys_import_kw: f64,
        phys_export_kw: f64,
    ) -> SiteFlexibilityEnvelope {
        // Computed under the lock and returned by value: nothing here awaits while it is held.
        let sim = self.sim.lock().await;
        super::site_headroom::compute_site_headroom(&sim, now, phys_import_kw, phys_export_kw)
    }

    async fn capacity_curves_at(
        &self,
        plan: &Plan,
        start: DateTime<Utc>,
        now: DateTime<Utc>,
        phys_import_kw: f64,
        phys_export_kw: f64,
    ) -> Option<CapacityCurves> {
        let sim = self.sim.lock().await;
        super::capacity_headroom::compute_site_capacity_curves_at(
            &sim,
            plan,
            start,
            now,
            phys_import_kw,
            phys_export_kw,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assets::{AssetState, ShiftableLoadState};
    use crate::entities::asset_params::{AssetParams, BatteryParams};
    use chrono::{Duration, Utc};
    use uuid::Uuid;

    fn handle_with(params: &[AssetParams]) -> (SimHandle, Arc<Mutex<SimState>>) {
        let sim = Arc::new(Mutex::new(SimState::from_params(params, Utc::now())));
        (SimHandle::new(sim.clone()), sim)
    }

    fn load(asset_id: &str) -> ShiftableLoad {
        let now = Utc::now();
        ShiftableLoad {
            id: Uuid::new_v4(),
            asset_id: asset_id.to_string(),
            power_kw: 2.0,
            duration_min: 60,
            earliest_start: now,
            latest_end: now + Duration::hours(4),
            mode: Default::default(),
            created_at: now,
            updated_at: now,
        }
    }

    fn all_kinds() -> Vec<AssetParams> {
        use crate::entities::asset_params::{BaseLoadParams, EvParams, HeaterParams, PvParams};
        vec![
            AssetParams::Battery(BatteryParams::default()),
            AssetParams::Ev(EvParams::default()),
            AssetParams::Heater(HeaterParams::default()),
            AssetParams::Pv(PvParams::default()),
            AssetParams::BaseLoad(BaseLoadParams::default()),
        ]
    }

    /// The port answers exactly what the direct computation answers for the same simulator.
    #[tokio::test]
    async fn site_headroom_equals_the_direct_computation() {
        let (handle, sim) = handle_with(&all_kinds());
        let now = Utc::now();
        let direct =
            super::super::site_headroom::compute_site_headroom(&*sim.lock().await, now, 10.0, 8.0);
        let via_port = handle.site_headroom(now, 10.0, 8.0).await;
        assert_eq!(
            serde_json::to_value(via_port).unwrap(),
            serde_json::to_value(direct).unwrap()
        );
    }

    #[tokio::test]
    async fn capacity_curves_at_equals_the_direct_computation() {
        use crate::services::test_support::plans::flat_plan;
        let (handle, sim) = handle_with(&all_kinds());
        let now = Utc::now();
        let plan = flat_plan(900, 8, now);
        let start = now + Duration::minutes(30);
        let direct = super::super::capacity_headroom::compute_site_capacity_curves_at(
            &*sim.lock().await,
            &plan,
            start,
            now,
            10.0,
            8.0,
        );
        let via_port = handle
            .capacity_curves_at(&plan, start, now, 10.0, 8.0)
            .await;
        assert!(via_port.is_some(), "a plan with slots left anchors curves");
        assert_eq!(via_port, direct);
    }

    #[tokio::test]
    async fn add_shiftable_adds_the_load_to_the_roster_and_rejects_a_duplicate() {
        let (handle, sim) = handle_with(&[]);
        handle.add_shiftable(&load("wm")).await.unwrap();
        assert!(sim.lock().await.find_asset("wm").is_some());
        assert!(handle.add_shiftable(&load("wm")).await.is_err());
        assert_eq!(sim.lock().await.assets.len(), 1);
    }

    #[tokio::test]
    async fn cancel_if_cancellable_removes_a_pending_load() {
        let (handle, sim) = handle_with(&[]);
        handle.add_shiftable(&load("wm")).await.unwrap();
        assert_eq!(
            handle.cancel_if_cancellable("wm").await,
            CancelOutcome::Removed
        );
        assert!(sim.lock().await.find_asset("wm").is_none());
    }

    #[tokio::test]
    async fn cancel_if_cancellable_keeps_a_load_that_has_started() {
        let (handle, sim) = handle_with(&[]);
        handle.add_shiftable(&load("wm")).await.unwrap();
        sim.lock().await.asset_mut("wm").unwrap().state =
            AssetState::ShiftableLoad(ShiftableLoadState {
                started: true,
                elapsed_min: 5.0,
                actual_power_kw: 2.0,
            });
        assert_eq!(
            handle.cancel_if_cancellable("wm").await,
            CancelOutcome::NotCancellable
        );
        assert!(
            sim.lock().await.find_asset("wm").is_some(),
            "the running load stays"
        );
    }

    #[tokio::test]
    async fn cancel_if_cancellable_reports_an_unknown_asset() {
        let (handle, _sim) = handle_with(&[]);
        assert_eq!(
            handle.cancel_if_cancellable("nope").await,
            CancelOutcome::NoSuchAsset
        );
    }

    #[tokio::test]
    async fn reset_asset_applies_the_values_and_reports_an_unknown_id() {
        let (handle, sim) = handle_with(&[AssetParams::Battery(BatteryParams::default())]);
        let values = HashMap::from([("soc".to_string(), 0.25)]);
        assert!(
            handle
                .reset_asset(crate::ids::ASSET_BATTERY, values.clone())
                .await
        );
        let guard = sim.lock().await;
        let (entry, cfg) = guard.find_asset(crate::ids::ASSET_BATTERY).unwrap();
        assert_eq!(cfg.state_values(&entry.state).get("soc"), Some(&0.25));
        drop(guard);
        assert!(!handle.reset_asset("nope", values).await);
    }

    #[tokio::test]
    async fn update_asset_config_applies_the_values_and_reports_an_unknown_id() {
        let (handle, sim) = handle_with(&[AssetParams::Battery(BatteryParams::default())]);
        let values = HashMap::from([("capacity_kwh".to_string(), 42.0)]);
        assert!(
            handle
                .update_asset_config(crate::ids::ASSET_BATTERY, values.clone())
                .await
        );
        let guard = sim.lock().await;
        let (entry, cfg) = guard.find_asset(crate::ids::ASSET_BATTERY).unwrap();
        assert_eq!(
            cfg.state_values(&entry.state).get("capacity_kwh"),
            Some(&42.0)
        );
        drop(guard);
        assert!(!handle.update_asset_config("nope", values).await);
    }
}
