use chrono::Utc;
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};
use tracing::{debug, error, info, warn};

use crate::controller;
use crate::controller::VtnPort;
use crate::entities::asset::PlanTrigger;
use crate::entities::asset_params::AssetParams;
use crate::entities::planner_params::{PlannerObjective, PlannerParams};
use crate::planner_events::{PlannerEvent, PlannerEventTx};
use crate::simulator::SimState;
use crate::state::AppState;

#[allow(clippy::too_many_arguments)]
pub(crate) fn spawn_planning(
    state: AppState,
    planner: PlannerParams,
    grid_max_import_kw: f64,
    grid_max_export_kw: f64,
    asset_params: Vec<AssetParams>,
    vtn: Arc<dyn VtnPort>,
    ven_name: String,
    mut trigger_rx: tokio::sync::watch::Receiver<PlanTrigger>,
    sim: Arc<Mutex<SimState>>,
    active_objective: Arc<RwLock<PlannerObjective>>,
    event_tx: PlannerEventTx,
) -> tokio::task::JoinHandle<()> {
    let replan_s = planner.replan_interval_s;
    let initial_delay_s = planner.planning_initial_delay_s;
    tokio::spawn(async move {
        // Initial delay: let event poll populate rates before first plan
        tokio::time::sleep(std::time::Duration::from_secs(initial_delay_s)).await;
        // First cycle is always Periodic; subsequent cycles are set by the select! below.
        // Using a local variable instead of borrow()ing the watch channel prevents stale
        // retained values (e.g. AssetStateChange set once and never cleared) from
        // mis-classifying every subsequent timeout-driven cycle as a hard trigger and
        // bypassing the plan acceptance gate.
        let mut wake_trigger = PlanTrigger::Periodic;
        loop {
            let now = Utc::now();
            let rates = state.planned_tariffs().await;
            let capacity = state.capacity_state().await;
            let trigger = wake_trigger.clone();
            let trigger_reason = format!("{:?}", trigger);

            info!(trigger = %trigger_reason, "planner loop: starting plan cycle");

            let tariff_ts =
                crate::entities::tariff_snapshot::TariffTimeSeries::from_snapshots(&rates);
            let ev_sess = state.ev_session().await;
            let heat_tgt = state.heater_target().await;
            let shift_loads = state.shiftable_loads().await;
            let bl_override = state.baseline_override().await;
            let obj = *active_objective.read().await;
            // Read inject state BEFORE cloning the sim. The pv_irradiance inject is a
            // one-shot: the sim tick applies it to pv.irradiance_offset and then clears
            // inject.pv_irradiance. If we read inject_state after the clone, the tick
            // can race in between: it clears the inject flag but we already have a stale
            // clone with offset=0. Reading first guarantees we always capture the pending
            // value before the tick has a chance to clear it.
            let inject_snap = state.inject_state().await;
            let pv_forecast_override = inject_snap.pv_plan_kw;
            // Clone SimState snapshot so the Mutex is released immediately.
            // MILP solving takes 18-60s on Pi4 ARM64; holding the lock would
            // block sim ticks and /capability reads for the entire duration.
            let lock_start = std::time::Instant::now();
            let mut sim_snap = sim.lock().await.clone();
            let lock_ms = lock_start.elapsed().as_millis();
            if lock_ms > 500 {
                warn!(lock_wait_ms = lock_ms, trigger = %trigger_reason, "planner: sim lock wait was long");
            } else {
                debug!(lock_wait_ms = lock_ms, "planner: sim lock acquired");
            }

            // Patch the clone when pv_irradiance inject is pending and the tick hasn't
            // applied it yet. When the tick runs first, the clone already has the correct
            // offset and this block is a no-op (inject_snap.pv_irradiance is None).
            if let Some(forced) = inject_snap.pv_irradiance {
                use crate::assets::{AssetConfig, PvInverter};
                let natural = PvInverter::natural_irradiance_at(now);
                if let Some((_, AssetConfig::Pv(pv))) =
                    sim_snap.find_asset_mut(crate::ids::ASSET_PV)
                {
                    pv.irradiance_offset = forced - natural;
                    pv.pv_alpha = inject_snap.pv_irradiance_alpha;
                }
            }

            // ── Emit solving_started ──────────────────────────────────────
            let num_slots = planner.plan_horizon_h as usize * 3600 / planner.plan_step_s as usize;
            let _ = event_tx.send(PlannerEvent::SolvingStarted {
                objective: obj,
                num_slots,
                triggered_at: now,
            });

            // ── Spawn 1 s progress ticker ─────────────────────────────────
            let (cancel_tx, cancel_rx) = tokio::sync::oneshot::channel::<()>();
            let progress_tx = event_tx.clone();
            let ticker_task = tokio::spawn(async move {
                let start = std::time::Instant::now();
                let mut ticker = tokio::time::interval(std::time::Duration::from_secs(1));
                ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
                let mut iteration: u32 = 0;
                let mut cancel_rx = cancel_rx;
                loop {
                    tokio::select! {
                        _ = ticker.tick() => {
                            iteration += 1;
                            let _ = progress_tx.send(PlannerEvent::SolvingProgress {
                                elapsed_ms: start.elapsed().as_millis() as u64,
                                iteration,
                            });
                        }
                        _ = &mut cancel_rx => break,
                    }
                }
            });

            // ── Run blocking HiGHS solve off the async runtime ────────────
            let solve_start = std::time::Instant::now();
            let planner_clone = planner.clone();
            let asset_params_clone = asset_params.clone();
            let trigger_for_planner = trigger.clone();
            let snap = sim_snap.to_sim_snapshot();

            // Build per-asset MILP contexts from live simulator state.
            // This happens before spawn_blocking so asset states are captured at this instant.
            let step_s = planner.plan_step_s;
            let n_slots = (planner.plan_horizon_h as f64 * 3600.0 / step_s as f64) as usize;
            let lambda_sw = asset_params
                .iter()
                .find_map(|p| match p {
                    AssetParams::Heater(h) => Some(h.switching_penalty_eur),
                    _ => None,
                })
                .unwrap_or(0.0);

            // Average import tariff over the planning horizon — used to auto-compute
            // terminal energy reward coefficients for storage assets.
            let avg_imp_eur_kwh = {
                let total: f64 = (0..n_slots)
                    .map(|t| {
                        let slot_t =
                            now + chrono::Duration::seconds(t as i64 * step_s as i64);
                        tariff_ts
                            .import_eur_kwh
                            .interpolate_at(slot_t)
                            .unwrap_or(0.25)
                    })
                    .sum();
                if n_slots > 0 { total / n_slots as f64 } else { 0.25 }
            };

            // Resolve c_terminal_eur_kwh per asset type. Battery and heater get
            // different formulas; EV gets 0.0 (deadline constraint handles incentive).
            // Profile override (Some(x)) takes precedence over auto-computed value.
            let heater_c_terminal_eur_kwh = asset_params
                .iter()
                .find_map(|p| match p {
                    AssetParams::Heater(h) => Some(h.c_terminal_eur_kwh.unwrap_or_else(|| {
                        avg_imp_eur_kwh + planner.c_ctrl_imp_malus_eur_kwh
                    })),
                    _ => None,
                })
                .unwrap_or(0.0);
            let battery_c_terminal_eur_kwh = asset_params
                .iter()
                .find_map(|p| match p {
                    AssetParams::Battery(b) => Some(b.c_terminal_eur_kwh.unwrap_or_else(|| {
                        avg_imp_eur_kwh * b.round_trip_efficiency
                    })),
                    _ => None,
                })
                .unwrap_or(0.0);

            let asset_contexts: Vec<
                Box<dyn controller::milp_planner::asset_port::AssetMilpContext>,
            > = sim_snap
                .iter_assets()
                .filter_map(|(entry, cfg)| {
                    // Use per-asset c_terminal: heater and battery get their own coefficient;
                    // EV gets 0.0 (deadline constraint handles the charging incentive).
                    let c_terminal = match cfg {
                        crate::assets::AssetConfig::Heater(_) => heater_c_terminal_eur_kwh,
                        crate::assets::AssetConfig::Battery(_) => battery_c_terminal_eur_kwh,
                        _ => 0.0,
                    };
                    cfg.build_milp_context(
                        &entry.state,
                        n_slots,
                        step_s,
                        now,
                        ev_sess.as_ref(),
                        heat_tgt.as_ref(),
                        asset_params
                            .iter()
                            .find_map(|p| match p {
                                AssetParams::Ev(e) => Some(e.min_charge_kw),
                                _ => None,
                            })
                            .unwrap_or(0.0),
                        planner.v_ev_extra_eur_kwh,
                        planner.v_ev_core_eur_kwh,
                        lambda_sw,
                        c_terminal,
                    )
                })
                .collect();

            let plan = tokio::task::spawn_blocking(move || {
                controller::milp_planner::run_planner(
                    asset_contexts,
                    &snap,
                    &tariff_ts,
                    &capacity,
                    &planner_clone,
                    grid_max_import_kw,
                    grid_max_export_kw,
                    &asset_params_clone,
                    now,
                    trigger_for_planner,
                    ev_sess.as_ref(),
                    heat_tgt.as_ref(),
                    &shift_loads,
                    bl_override.as_ref(),
                    Some(obj),
                    pv_forecast_override,
                )
            })
            .await
            .expect("planner task panicked");
            let solver_ms = solve_start.elapsed().as_millis() as u64;
            info!(
                solver_ms,
                trigger = %trigger_reason,
                slots = plan.slots.len(),
                objective_eur = plan.objective_eur,
                "planner: solve complete"
            );

            // ── Cancel ticker, delegate adoption + events to PlanningService ──
            let _ = cancel_tx.send(());
            ticker_task.await.ok();

            let cycle = crate::services::PlanningService::adopt_if_warranted(
                plan,
                &trigger,
                &trigger_reason,
                planner.plan_adoption_threshold_eur,
                planner.plan_adoption_decay_s,
                solver_ms,
                obj,
                &state,
                &event_tx,
                now,
            )
            .await;

            // Refresh site envelope immediately after each plan cycle.
            {
                let sim_snap = sim.lock().await.to_sim_snapshot();
                let env = controller::envelope::compute_envelope(&sim_snap, now);
                state.set_site_envelope(env).await;
            }

            info!(
                trigger = %trigger_reason,
                slot_count = cycle.plan.slots.len(),
                adopted = cycle.adopted,
                "plan cycle complete"
            );

            // Event-driven status report on PlanCycle (T050)
            {
                let sim_snap = sim.lock().await.to_sim_snapshot();
                let report_opt = controller::reporter::build_status_report(
                    &cycle.plan_cycle_event,
                    &sim_snap,
                    &ven_name,
                    None,
                    now,
                );
                if let Some(report) = report_opt {
                    if let Err(e) = vtn.upsert_report(report).await {
                        error!("status report (plan cycle) submission failed: {e:#}");
                    }
                }
            }

            // Wait for next trigger OR periodic timeout.
            // Record what woke us: timeout → Periodic, channel change → that trigger.
            // This ensures the acceptance gate sees Periodic for routine replans
            // and is only bypassed for genuine event-driven triggers.
            wake_trigger = tokio::select! {
                _ = tokio::time::sleep(std::time::Duration::from_secs(replan_s)) => PlanTrigger::Periodic,
                _ = trigger_rx.changed() => trigger_rx.borrow_and_update().clone(),
            };
        }
    })
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use tokio::sync::{broadcast, watch, Mutex, RwLock};

    use crate::entities::asset::PlanTrigger;
    use crate::entities::planner_params::{PlannerObjective, PlannerParams};
    use crate::planner_events::PlannerEvent;
    use crate::services::test_support::mock_vtn::MockVtn;
    use crate::simulator::SimState;
    use crate::state::AppState;

    use super::spawn_planning;

    fn minimal_sim() -> Arc<Mutex<SimState>> {
        let s: SimState = serde_json::from_value(serde_json::json!({
            "asset_configs": [],
            "assets": [],
            "grid": {
                "net_power_w": 0.0, "import_w": 0.0, "export_w": 0.0,
                "voltage_v": 0.0, "import_kwh": 0.0, "export_kwh": 0.0
            },
            "last_tick": chrono::Utc::now().to_rfc3339()
        }))
        .expect("minimal SimState must deserialize");
        Arc::new(Mutex::new(s))
    }

    #[tokio::test]
    async fn spawn_planning_constructs_without_panic() {
        let (trigger_tx, trigger_rx) = watch::channel(PlanTrigger::Periodic);
        let (event_bcast_tx, _) = broadcast::channel::<PlannerEvent>(1);
        let event_tx = Arc::new(event_bcast_tx);
        let vtn = Arc::new(MockVtn::new());
        let sim = minimal_sim();
        let active_objective = Arc::new(RwLock::new(PlannerObjective::default()));

        let handle = spawn_planning(
            AppState::new(),
            PlannerParams::default(),
            10.0,
            10.0,
            vec![],
            vtn,
            "test-ven".to_string(),
            trigger_rx,
            sim,
            active_objective,
            event_tx,
        );
        handle.abort();
        let _ = trigger_tx; // keep alive until abort
                            // passes if no panic during construction and abort
    }
}
