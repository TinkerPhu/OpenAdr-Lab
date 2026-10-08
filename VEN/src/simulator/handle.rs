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
use crate::controller::sim_read_port::SimReadPort;
use crate::controller::sim_roster_port::{CancelOutcome, SimRosterPort};
use crate::entities::asset::{AssetCapabilityView, AssetTraceRow, ComfortRate};
use crate::entities::asset_params::AssetRequestSlice;
use crate::entities::capacity_curve::CapacityCurves;
use crate::entities::device_session::ShiftableLoad;
use crate::entities::ev_usage::EvUsageSimState;
use crate::entities::plan::{Plan, SiteFlexibilityEnvelope};
use crate::entities::timeline::TimelineSnapshot;
use crate::entities::DomainError;
use chrono::Duration;
use chrono::{DateTime, Utc};
use lab_core::time_series::{Interpolation, TimeSeries};
use tracing::{debug, warn};

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

    async fn reset_asset(
        &self,
        asset_id: &str,
        values: HashMap<String, f64>,
    ) -> Result<(), DomainError> {
        let mut sim = self.sim.lock().await;
        let (entry, cfg) =
            sim.find_asset_mut(asset_id)
                .ok_or_else(|| DomainError::AssetNotFound {
                    asset_id: asset_id.to_string(),
                })?;
        cfg.validate_values(asset_id, &values)?;
        cfg.reset(&mut entry.state, values);
        Ok(())
    }

    async fn update_asset_config(
        &self,
        asset_id: &str,
        values: HashMap<String, f64>,
    ) -> Result<(), DomainError> {
        let mut sim = self.sim.lock().await;
        let (_entry, cfg) =
            sim.find_asset_mut(asset_id)
                .ok_or_else(|| DomainError::AssetNotFound {
                    asset_id: asset_id.to_string(),
                })?;
        cfg.validate_values(asset_id, &values)?;
        cfg.update_config(values);
        Ok(())
    }
}

#[async_trait]
impl SimReadPort for SimHandle {
    async fn timeline_snapshot(&self, now: DateTime<Utc>) -> TimelineSnapshot {
        self.sim.lock().await.to_timeline_snapshot(now)
    }

    async fn asset_forecast(
        &self,
        asset_id: &str,
        timespan: Duration,
        now: DateTime<Utc>,
    ) -> Option<TimeSeries> {
        let sim = self.sim.lock().await;
        let (entry, cfg) = sim.find_asset(asset_id)?;
        Some(cfg.forecast(&entry.state, timespan, now))
    }

    async fn asset_capability(&self, asset_id: &str) -> Option<AssetCapabilityView> {
        let sim = self.sim.lock().await;
        let (entry, cfg) = sim.find_asset(asset_id)?;
        let capability = cfg.capability(&entry.state);
        let floor = cfg.flexibility_floor(&entry.state);
        Some(AssetCapabilityView {
            is_fixed: capability.is_fixed(&floor),
            key_features: cfg.key_features(&entry.state),
            capability,
            floor,
        })
    }

    async fn asset_history(
        &self,
        asset_id: &str,
        timespan: Duration,
        now: DateTime<Utc>,
    ) -> Option<TimeSeries> {
        let sim = self.sim.lock().await;
        let entry = sim.asset(asset_id)?;
        let points = entry.history.slice(timespan, now);
        // A LOCF boundary point at now-timespan, so consumers always get a sample anchored at
        // the start of the requested window.
        let boundary_ts = now - timespan;
        let boundary_power_kw = entry.history.power_at(boundary_ts).unwrap_or(0.0);
        let mut samples = vec![(boundary_ts, boundary_power_kw)];
        samples.extend(points.iter().map(|p| (p.ts, p.power_kw)));
        Some(TimeSeries {
            samples,
            interpolation: Interpolation::Linear,
        })
    }

    async fn asset_trace(
        &self,
        asset_id: &str,
        window: Duration,
        now: DateTime<Utc>,
    ) -> Vec<AssetTraceRow> {
        let lock_start = std::time::Instant::now();
        let sim = self.sim.lock().await;
        let lock_wait_ms = lock_start.elapsed().as_millis();
        if lock_wait_ms > 100 {
            warn!(
                lock_wait_ms,
                asset = %asset_id,
                "asset trace: sim mutex wait was long (planner may be running)"
            );
        } else {
            debug!(lock_wait_ms, asset = %asset_id, "asset trace: sim mutex acquired");
        }
        let Some((entry, cfg)) = sim.find_asset(asset_id) else {
            return Vec::new();
        };
        entry
            .history
            .slice(window, now)
            .into_iter()
            .map(|p| {
                let mut values = cfg.state_values(&p.state);
                values.insert("power_kw".into(), p.power_kw);
                AssetTraceRow { ts: p.ts, values }
            })
            .collect()
    }

    async fn default_comfort_rates(&self, asset_id: &str) -> Option<Vec<ComfortRate>> {
        let sim = self.sim.lock().await;
        sim.find_asset(asset_id)
            .map(|(_, cfg)| cfg.default_comfort_rates())
    }

    async fn usage_schedule_view(&self, now: DateTime<Utc>) -> Option<EvUsageSimState> {
        let sim = self.sim.lock().await;
        sim.find_asset(crate::ids::ASSET_EV)
            .and_then(|(_, cfg)| cfg.usage_schedule_view(now))
    }

    async fn request_slices(&self) -> Vec<AssetRequestSlice> {
        let sim = self.sim.lock().await;
        sim.iter_assets()
            .map(|(entry, cfg)| {
                // Storage-shaped assets declare their own request defaults; the rest
                // (heater, PV, base load) have none.
                let defaults = cfg
                    .as_request_resolvable()
                    .map(|r| r.request_defaults(&entry.state));
                AssetRequestSlice {
                    id: entry.id.clone(),
                    current_soc: defaults.map(|d| d.current_soc),
                    default_soc_target: defaults.map(|d| d.default_soc_target),
                    capacity_kwh: defaults.map(|d| d.capacity_kwh),
                    max_charge_kw: defaults.map(|d| d.max_charge_kw),
                    completion_policy: cfg.default_completion_policy(),
                    comfort_rates: cfg.default_comfort_rates(),
                    default_target_temp_c: cfg
                        .as_thermostat()
                        .and_then(|t| t.default_request_target_c()),
                }
            })
            .collect()
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

    #[tokio::test]
    async fn timeline_snapshot_equals_the_direct_snapshot() {
        let (handle, sim) = handle_with(&all_kinds());
        let now = Utc::now();
        let direct = sim.lock().await.to_timeline_snapshot(now);
        let via_port = handle.timeline_snapshot(now).await;
        assert_eq!(via_port.assets.len(), direct.assets.len());
        assert!(via_port.assets.contains_key(crate::ids::ASSET_BATTERY));
    }

    #[tokio::test]
    async fn asset_forecast_is_the_assets_own_series_and_none_for_an_unknown_asset() {
        let (handle, _sim) = handle_with(&all_kinds());
        let series = handle
            .asset_forecast(
                crate::ids::ASSET_BATTERY,
                Duration::seconds(300),
                Utc::now(),
            )
            .await
            .expect("the battery forecasts");
        assert!(!series.samples.is_empty());
        assert!(handle
            .asset_forecast("nope", Duration::seconds(300), Utc::now())
            .await
            .is_none());
    }

    #[tokio::test]
    async fn asset_capability_carries_the_assets_range_floor_and_key_features() {
        let (handle, sim) = handle_with(&all_kinds());
        let view = handle
            .asset_capability(crate::ids::ASSET_BATTERY)
            .await
            .expect("the battery answers");
        let guard = sim.lock().await;
        let (entry, cfg) = guard.find_asset(crate::ids::ASSET_BATTERY).unwrap();
        assert_eq!(
            view.capability.max_import_kw,
            cfg.capability(&entry.state).max_import_kw
        );
        assert_eq!(view.key_features, cfg.key_features(&entry.state));
        assert_eq!(view.is_fixed, view.capability.is_fixed(&view.floor));
        drop(guard);
        assert!(handle.asset_capability("nope").await.is_none());
    }

    #[tokio::test]
    async fn asset_history_is_anchored_at_the_window_start_and_follows_the_records() {
        let (handle, sim) = handle_with(&all_kinds());
        let now = Utc::now();
        sim.lock()
            .await
            .record_history(now - Duration::seconds(10), 10.0, -5.0);
        sim.lock()
            .await
            .record_history(now - Duration::seconds(5), 10.0, -5.0);
        let series = handle
            .asset_history(crate::ids::ASSET_BATTERY, Duration::seconds(60), now)
            .await
            .expect("the battery has a history");
        assert_eq!(
            series.samples.len(),
            3,
            "boundary point plus the two records"
        );
        assert_eq!(series.samples[0].0, now - Duration::seconds(60));
        assert!(handle
            .asset_history("nope", Duration::seconds(60), now)
            .await
            .is_none());
    }

    #[tokio::test]
    async fn asset_trace_rows_are_state_values_plus_power_and_empty_for_an_unknown_asset() {
        let (handle, sim) = handle_with(&all_kinds());
        let now = Utc::now();
        sim.lock()
            .await
            .record_history(now - Duration::seconds(5), 10.0, -5.0);
        let rows = handle
            .asset_trace(crate::ids::ASSET_BATTERY, Duration::hours(24), now)
            .await;
        assert_eq!(rows.len(), 1);
        assert!(rows[0].values.contains_key("power_kw"));
        assert!(
            rows[0].values.contains_key("soc"),
            "the battery's own state values"
        );
        assert!(handle
            .asset_trace("nope", Duration::hours(24), now)
            .await
            .is_empty());
    }

    #[tokio::test]
    async fn default_comfort_rates_are_the_assets_and_none_for_an_unknown_asset() {
        let (handle, _sim) = handle_with(&all_kinds());
        assert!(handle
            .default_comfort_rates(crate::ids::ASSET_EV)
            .await
            .is_some());
        assert!(handle.default_comfort_rates("nope").await.is_none());
    }

    #[tokio::test]
    async fn usage_schedule_view_is_none_without_a_configured_schedule() {
        let (handle, _sim) = handle_with(&all_kinds());
        assert!(handle.usage_schedule_view(Utc::now()).await.is_none());
        let (handle, _sim) = handle_with(&[]);
        assert!(
            handle.usage_schedule_view(Utc::now()).await.is_none(),
            "no EV at all"
        );
    }

    #[tokio::test]
    async fn request_slices_carry_the_heaters_declared_target_and_none_for_other_assets() {
        use crate::entities::asset_params::HeaterParams;
        let heater = AssetParams::Heater(HeaterParams {
            default_target_temp_c: Some(21.0),
            ..HeaterParams::default()
        });
        let (handle, _sim) = handle_with(&[heater, AssetParams::Battery(BatteryParams::default())]);
        for slice in handle.request_slices().await {
            let expected = (slice.id == crate::ids::ASSET_HEATER).then_some(21.0);
            assert_eq!(slice.default_target_temp_c, expected, "on '{}'", slice.id);
        }
    }

    #[tokio::test]
    async fn request_slices_give_storage_assets_their_defaults_and_the_rest_none() {
        let (handle, _sim) = handle_with(&all_kinds());
        let slices = handle.request_slices().await;
        assert_eq!(slices.len(), 5);
        for slice in &slices {
            let storage = slice.id == crate::ids::ASSET_BATTERY || slice.id == crate::ids::ASSET_EV;
            assert_eq!(
                slice.current_soc.is_some(),
                storage,
                "defaults on '{}'",
                slice.id
            );
            assert_eq!(
                slice.capacity_kwh.is_some(),
                storage,
                "capacity on '{}'",
                slice.id
            );
        }
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
        handle
            .reset_asset(crate::ids::ASSET_BATTERY, values.clone())
            .await
            .expect("a valid value is applied");
        let guard = sim.lock().await;
        let (entry, cfg) = guard.find_asset(crate::ids::ASSET_BATTERY).unwrap();
        assert_eq!(cfg.state_values(&entry.state).get("soc"), Some(&0.25));
        drop(guard);
        assert!(matches!(
            handle.reset_asset("nope", values).await,
            Err(DomainError::AssetNotFound { .. })
        ));
    }

    #[tokio::test]
    async fn update_asset_config_applies_the_values_and_reports_an_unknown_id() {
        let (handle, sim) = handle_with(&[AssetParams::Battery(BatteryParams::default())]);
        let values = HashMap::from([("capacity_kwh".to_string(), 42.0)]);
        handle
            .update_asset_config(crate::ids::ASSET_BATTERY, values.clone())
            .await
            .expect("a valid value is applied");
        let guard = sim.lock().await;
        let (entry, cfg) = guard.find_asset(crate::ids::ASSET_BATTERY).unwrap();
        assert_eq!(
            cfg.state_values(&entry.state).get("capacity_kwh"),
            Some(&42.0)
        );
        drop(guard);
        assert!(matches!(
            handle.update_asset_config("nope", values).await,
            Err(DomainError::AssetNotFound { .. })
        ));
    }

    /// R-113: the battery refuses a value outside its own limits, and nothing is applied.
    #[tokio::test]
    async fn the_battery_refuses_out_of_range_values_and_keeps_its_own() {
        let (handle, sim) = handle_with(&[AssetParams::Battery(BatteryParams::default())]);
        let battery = crate::ids::ASSET_BATTERY;
        let before = {
            let guard = sim.lock().await;
            let (entry, cfg) = guard.find_asset(battery).unwrap();
            cfg.state_values(&entry.state)
        };
        for (key, value, port_reset) in [
            ("soc", 1.5, true),
            ("min_soc", -0.1, false),
            ("capacity_kwh", 0.0, false),
        ] {
            let values = HashMap::from([(key.to_string(), value)]);
            let result = if port_reset {
                handle.reset_asset(battery, values).await
            } else {
                handle.update_asset_config(battery, values).await
            };
            match result {
                Err(DomainError::InvalidValue {
                    key: k, message, ..
                }) => {
                    assert_eq!(k, key);
                    assert!(message.contains(key), "{message}");
                }
                other => panic!("{key}={value} must be refused, got {other:?}"),
            }
        }
        let guard = sim.lock().await;
        let (entry, cfg) = guard.find_asset(battery).unwrap();
        assert_eq!(
            cfg.state_values(&entry.state),
            before,
            "nothing was applied"
        );
    }
}
