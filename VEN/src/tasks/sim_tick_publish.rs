// Async publishers and persist helpers extracted from VEN/src/tasks/sim_tick.rs

use chrono::{DateTime, Utc};
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{error, info, debug, warn};
use crate::controller;

use crate::models::SensorSnapshot;
use crate::simulator::{SimSnapshot, SimState};
use crate::entities::plan::SiteFlexibilityEnvelope;
use crate::entities::plan::Plan;
use crate::state::AppState;
use crate::entities::asset::PlanTrigger;
use crate::entities::tariff_snapshot::TariffSnapshot;
use crate::vtn::VtnClient;

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
    // Update sensor snapshot (backward compat)
    state.update_sensor(sensor).await;

    // Update sim in app state — augmented with shiftable runtimes

    // ── Shiftable load runtime: start / complete / augment ──────

    // Start: detect shiftable loads that the current plan slot wants
    // to run but that have no runtime yet.
    if let Some(plan) = plan_snap {
        if let Some(slot) = plan.slots.iter().find(|s| s.start <= now && now < s.end) {
            let runtimes = state.shiftable_runtimes().await;
            let loads = state.shiftable_loads().await;
            for alloc in &slot.allocations {
                if sim_snap.assets.contains_key(alloc.asset_id.as_str()) {
                    continue;
                }
                let already_running = runtimes.iter().any(|r| r.asset_id == alloc.asset_id);
                if !already_running {
                    if let Some(load) = loads.iter().find(|l| l.asset_id == alloc.asset_id) {
                        let ends_at = now + chrono::Duration::minutes(load.duration_min as i64);
                        state
                            .start_shiftable(crate::entities::device_session::ShiftableLoadRuntime {
                                load_id: load.id,
                                asset_id: load.asset_id.clone(),
                                power_kw: load.power_kw,
                                started_at: now,
                                ends_at,
                            })
                            .await;
                        info!(asset_id = %load.asset_id, ends_at = %ends_at, "shiftable load started");
                    }
                }
            }
        }
    }

    // Complete: remove expired runtimes and trigger replan.
    {
        let runtimes = state.shiftable_runtimes().await;
        for rt in &runtimes {
            if now >= rt.ends_at {
                info!(asset_id = %rt.asset_id, "shiftable load completed");
                state.complete_shiftable(rt.load_id).await;
                let _ = trigger_tx.send(PlanTrigger::UserRequest);
            }
        }
    }

    // Augment SimSnapshot with running shiftable runtimes so they
    // appear in GET /sim and ledger accounting.
    {
        let runtimes = state.shiftable_runtimes().await;
        for rt in &runtimes {
            if rt.is_running(now) {
                sim_snap.assets.insert(
                    rt.asset_id.clone(),
                    crate::simulator::AssetSnapshot {
                        power_kw: rt.power_kw,
                        values: {
                            let mut m = std::collections::HashMap::new();
                            m.insert("running".into(), 1.0);
                            m.insert("ends_at_unix".into(), rt.ends_at.timestamp() as f64);
                            m
                        },
                    },
                );
            }
        }
    }

    state.update_sim(sim_snap.clone()).await;

    // Post-tick: consolidated ledger accounting
    let mut ledger = state.asset_ledger().await;
    controller::monitor::record_tick(&mut ledger, &sim_snap, rates_snap, dt_s, now);
    state.set_asset_ledger(ledger).await;

    // Refresh site envelope (computed in-lock from final sim state).
    state.set_site_envelope(envelope).await;

    sim_snap
}

pub(crate) async fn run_measurement_reports(
    state: &AppState,
    sim: &Arc<Mutex<SimState>>,
    vtn: &VtnClient,
    ven_name: &str,
    now: DateTime<Utc>,
) {
    let events = state.events().await;
    let sim_guard = sim.lock().await;
    let reports = controller::reporter::build_measurement_reports_for_active_events(
        &events,
        &*sim_guard,
        ven_name,
        now,
    );
    drop(sim_guard);
    for report in reports {
        if let Err(e) = vtn.upsert_report(report).await {
            error!("measurement report submission failed: {e:#}");
        }
    }
}

pub(crate) async fn persist_sim_state(sim: &Arc<Mutex<SimState>>, data_dir: &str) {
    let sim_clone = { sim.lock().await.clone() };
    if let Err(e) = crate::simulator::persist::save(&sim_clone, data_dir).await {
        error!("sim persist failed: {e:#}");
    }
}
