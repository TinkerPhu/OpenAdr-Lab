// Extracted from VEN/src/loops.rs — simulator tick (spawn_sim_tick)

use chrono::{DateTime, Utc};
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};
use tracing::{debug, error, info, warn};

use crate::assets::AssetConfig;
use crate::controller;
use crate::entities;
use crate::entities::asset::PlanTrigger;
use crate::entities::capacity::OadrCapacityState;
use crate::entities::plan::Plan;
use crate::entities::plan::SiteFlexibilityEnvelope;
use crate::entities::tariff_snapshot::TariffSnapshot;
use crate::models::SensorSnapshot;
use crate::planner_events::{PlannerEvent, PlannerEventTx};
use crate::profile::{PlannerObjective, Profile};
use crate::simulator::SimState;
use crate::simulator::{AssetEntry, SimSnapshot};
use crate::state::{AppState, EvSettings, SimInjectState};
use crate::vtn::VtnClient;
use std::collections::HashMap;

use crate::controller::absorber::AbsorberState;

// Helper modules (split to keep sim_tick.rs small)
// Declared at tasks module level in tasks/mod.rs; called via crate::tasks::...


// Helper functions moved to sim_tick_helpers.rs and sim_tick_publish.rs

// Thin wrappers delegating to helper modules to keep this file under 200 lines.

pub(crate) fn apply_sim_injections(inject: &SimInjectState, sim: &mut SimState) -> Vec<&'static str> {
    crate::tasks::sim_tick_helpers::apply_sim_injections(inject, sim)
}

pub(crate) fn build_tick_setpoints(
    sim: &SimState,
    plan_snap: Option<&Plan>,
    capacity_snap: &OadrCapacityState,
    inject: &SimInjectState,
    overlay_enabled: bool,
    now: DateTime<Utc>,
) -> HashMap<String, f64> {
    crate::tasks::sim_tick_helpers::build_tick_setpoints(sim, plan_snap, capacity_snap, inject, overlay_enabled, now)
}

pub(crate) async fn publish_sim_tick_result(
    sensor: SensorSnapshot,
    mut sim_snap: SimSnapshot,
    envelope: SiteFlexibilityEnvelope,
    plan_snap: Option<&Plan>,
    state: &AppState,
    trigger_tx: &tokio::sync::watch::Sender<PlanTrigger>,
    rates_snap: &[TariffSnapshot],
    dt_s: f64,
    now: DateTime<Utc>,
) -> SimSnapshot {
    crate::tasks::sim_tick_publish::publish_sim_tick_result(sensor, sim_snap, envelope, plan_snap, state, trigger_tx, rates_snap, dt_s, now).await
}

pub(crate) fn finalize_tick_outputs(
    sim: &mut SimState,
    capacity_snap: &OadrCapacityState,
    now: DateTime<Utc>,
) -> (SensorSnapshot, SimSnapshot, SiteFlexibilityEnvelope) {
    crate::tasks::sim_tick_helpers::finalize_tick_outputs(sim, capacity_snap, now)
}

pub(crate) fn accumulate_deviation(
    absorber_state: &mut AbsorberState,
    residual_kw: f64,
    profile: &Profile,
    trigger_tx: &tokio::sync::watch::Sender<PlanTrigger>,
    deviation_pending: &std::sync::atomic::AtomicBool,
    now: DateTime<Utc>,
) {
    crate::tasks::sim_tick_helpers::accumulate_deviation(absorber_state, residual_kw, profile, trigger_tx, deviation_pending, now)
}

pub(crate) async fn run_measurement_reports(
    state: &AppState,
    sim: &Arc<Mutex<SimState>>,
    vtn: &VtnClient,
    ven_name: &str,
    now: DateTime<Utc>,
) {
    crate::tasks::sim_tick_publish::run_measurement_reports(state, sim, vtn, ven_name, now).await
}

pub(crate) async fn persist_sim_state(sim: &Arc<Mutex<SimState>>, data_dir: &str) {
    crate::tasks::sim_tick_publish::persist_sim_state(sim, data_dir).await
}


pub(crate) fn spawn_sim_tick(
    state: AppState,
    sim: Arc<Mutex<SimState>>,
    profile: Arc<Profile>,
    ven_name: String,
    vtn: VtnClient,
    trigger_tx: Arc<tokio::sync::watch::Sender<PlanTrigger>>,
    data_dir: String,
    event_tx: PlannerEventTx,
    deviation_pending: Arc<std::sync::atomic::AtomicBool>,
) -> tokio::task::JoinHandle<()> {
    let tick_s = profile.simulator.tick_s;
    let persist_every_s = profile.simulator.persist_every_s;
    let report_interval_s = profile.simulator.report_interval_s;
    tokio::spawn(async move {
        let mut tick_interval = tokio::time::interval(std::time::Duration::from_secs(tick_s));
        let mut persist_counter: u64 = 0;
        let persist_every_ticks = if tick_s > 0 {
            persist_every_s / tick_s
        } else {
            15
        };
        let mut report_counter: u64 = 0;
        let report_every_ticks = if tick_s > 0 && report_interval_s > 0 {
            report_interval_s / tick_s
        } else {
            0
        };

        // Plan F/G: Layer 1/2 absorber state (multi-asset deviation absorption + Tier 2 escalation)
        let mut absorber_state = AbsorberState {
            residual_ticks: 0,
            last_state_change_ts: HashMap::new(),
            settling_ticks: HashMap::new(),
            active_overlay_kw: HashMap::new(),
            correction_is_active: false,
            last_emitted_correction_kw: 0.0,
        };

        loop {
            tick_interval.tick().await;

            let (new_absorber_state, new_persist_counter, new_report_counter) =
                crate::tasks::sim_tick_tick::tick_once(
                    absorber_state,
                    state.clone(),
                    sim.clone(),
                    profile.clone(),
                    ven_name.clone(),
                    vtn.clone(),
                    trigger_tx.clone(),
                    data_dir.clone(),
                    event_tx.clone(),
                    deviation_pending.clone(),
                    persist_counter,
                    persist_every_ticks,
                    report_counter,
                    report_every_ticks,
                    tick_s,
                )
                .await;

            absorber_state = new_absorber_state;
            persist_counter = new_persist_counter;
            report_counter = new_report_counter;
        }
    })
}
