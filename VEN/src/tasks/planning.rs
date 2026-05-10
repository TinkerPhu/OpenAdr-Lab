use chrono::Utc;
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};
use tracing::{debug, error, info, warn};

use crate::controller;
use crate::entities::asset::PlanTrigger;
use crate::planner_events::{PlannerEvent, PlannerEventTx};
use crate::profile::{PlannerObjective, Profile};
use crate::simulator::SimState;
use crate::state::AppState;
use crate::vtn::VtnClient;

pub(crate) fn spawn_planning(
    state: AppState,
    profile: Arc<Profile>,
    vtn: VtnClient,
    ven_name: String,
    mut trigger_rx: tokio::sync::watch::Receiver<PlanTrigger>,
    sim: Arc<Mutex<SimState>>,
    active_objective: Arc<RwLock<PlannerObjective>>,
    event_tx: PlannerEventTx,
    deviation_pending: Arc<std::sync::atomic::AtomicBool>,
) -> tokio::task::JoinHandle<()> {
    let replan_s = profile.planner.replan_interval_s;
    tokio::spawn(async move {
        // Initial delay: let event poll populate rates before first plan
        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
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
            // If DeviceDeviation was latched (fired while we were solving and possibly
            // overwritten by a subsequent trigger), honour it over wake_trigger.
            let trigger = if deviation_pending.swap(false, std::sync::atomic::Ordering::AcqRel) {
                PlanTrigger::DeviceDeviation
            } else {
                wake_trigger.clone()
            };
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
                if let Some((_, cfg)) = sim_snap.find_asset_mut(crate::ids::ASSET_PV) {
                    if let AssetConfig::Pv(pv) = cfg {
                        pv.irradiance_offset = forced - natural;
                        pv.pv_alpha = inject_snap.pv_irradiance_alpha;
                    }
                }
            }

            // ── Emit solving_started ──────────────────────────────────────
            let num_slots = profile.planner.plan_horizon_h as usize * 3600
                / profile.planner.plan_step_s as usize;
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
            let profile_clone = profile.clone(); // Arc<Profile>, cheap
            let trigger_for_planner = trigger.clone(); // keep `trigger` for acceptance gate below
            let snap = sim_snap.to_sim_snapshot();

            // Build per-asset MILP contexts from live simulator state.
            // This happens before spawn_blocking so asset states are captured at this instant.
            let step_s = profile.planner.plan_step_s;
            let n_slots = (profile.planner.plan_horizon_h as f64 * 3600.0 / step_s as f64) as usize;
            let lambda_sw = profile
                .heater_config()
                .map(|h| h.effective_switching_penalty())
                .unwrap_or(0.0);
            let asset_contexts: Vec<Box<dyn controller::milp_planner::asset_port::AssetMilpContext>> =
                sim_snap
                    .iter_assets()
                    .filter_map(|(entry, cfg)| {
                        cfg.build_milp_context(
                            &entry.state,
                            n_slots,
                            step_s,
                            now,
                            ev_sess.as_ref(),
                            heat_tgt.as_ref(),
                            profile.ev_config().map(|e| e.min_charge_kw).unwrap_or(0.0),
                            profile.planner.v_ev_extra_eur_kwh,
                            lambda_sw,
                        )
                    })
                    .collect();

            let plan = tokio::task::spawn_blocking(move || {
                controller::milp_planner::run_planner(
                    asset_contexts,
                    &snap,
                    &tariff_ts,
                    &capacity,
                    &profile_clone,
                    now,
                    trigger_for_planner,
                    ev_sess.as_ref(),
                    heat_tgt.as_ref(),
                    &shift_loads,
                    bl_override.as_ref(),
                    Some(obj),
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

            // ── Cancel ticker, emit plan_ready ────────────────────────────
            let _ = cancel_tx.send(());
            ticker_task.await.ok();
            let _ = event_tx.send(PlannerEvent::PlanReady {
                plan_id: plan.id,
                objective: obj,
                solver_ms,
                objective_eur: plan.objective_eur,
                friction_eur: plan.friction_eur,
                slot_count: plan.slots.len(),
                trigger: trigger_reason.clone(),
            });
            // Acceptance gate: for Periodic replans, only adopt the new plan when it
            // improves the total cost (Phase 1 + Phase 2 friction) by more than the
            // effective threshold. Using total cost prevents a fragmented plan with low
            // Phase 1 cost but high switching friction from displacing a smooth plan.
            // The threshold decays linearly with plan age so that changing circumstances
            // are never permanently blocked — after plan_adoption_decay_s seconds, any
            // new plan is accepted. Hard triggers always force adoption.
            let threshold = profile.planner.plan_adoption_threshold_eur;
            let decay_s = profile.planner.plan_adoption_decay_s;
            let is_hard_trigger = !matches!(trigger, PlanTrigger::Periodic);
            debug!(
                trigger = %trigger_reason,
                is_hard_trigger,
                objective_eur = plan.objective_eur,
                friction_eur = plan.friction_eur,
                total_eur = plan.objective_eur + plan.friction_eur,
                threshold_eur = threshold,
                "acceptance gate eval"
            );

            let adopt = if is_hard_trigger || threshold == 0.0 {
                true
            } else if let Some(ref current) = state.active_plan().await {
                let elapsed_s = (now - current.created_at).num_seconds().max(0) as f64;
                let decay_factor = if decay_s > 0.0 {
                    (1.0 - elapsed_s / decay_s).max(0.0)
                } else {
                    1.0
                };
                let effective_threshold = threshold * decay_factor;
                // When decay window has fully elapsed, refresh unconditionally so the
                // rolling 24h window never becomes stale regardless of cost delta.
                let fully_decayed = decay_s > 0.0 && elapsed_s >= decay_s;
                let current_total = current.objective_eur + current.friction_eur;
                let new_total = plan.objective_eur + plan.friction_eur;
                let improvement = current_total - new_total;
                if fully_decayed || improvement > effective_threshold {
                    true
                } else {
                    info!(
                        improvement_eur = improvement,
                        effective_threshold_eur = effective_threshold,
                        threshold_eur = threshold,
                        elapsed_s = elapsed_s,
                        decay_factor = decay_factor,
                        fully_decayed,
                        current_total_eur = current_total,
                        new_total_eur = new_total,
                        "periodic plan rejected: improvement below threshold"
                    );
                    false
                }
            } else {
                true // no existing plan → always adopt
            };

            let slot_count = plan.slots.len();
            if adopt {
                info!(trigger = %trigger_reason, slot_count, "planner: plan adopted");
                // Replan supersedes any active correction
                let _ = event_tx.send(PlannerEvent::CorrectionCleared {
                    ts: now,
                    reason: "superseded_by_replan".to_string(),
                });
                state.set_active_plan(Some(plan)).await;
            } else {
                info!(trigger = %trigger_reason, slot_count, "planner: plan NOT adopted (periodic below threshold)");
            }

            // Refresh site envelope immediately after each plan cycle.
            {
                let sim_snap = sim.lock().await.to_sim_snapshot();
                let env = controller::envelope::compute_envelope(&sim_snap, now);
                state.set_site_envelope(env).await;
            }

            info!(trigger = %trigger_reason, slot_count, "plan cycle complete");

            // Emit PlanCycle controller event (T029)
            let plan_cycle_event = controller::trace::ControllerEvent::PlanCycle {
                ts: now,
                trigger_reason,
                total_slots: slot_count,
            };
            state.push_controller_event(plan_cycle_event.clone()).await;

            // Event-driven status report on PlanCycle (T050)
            {
                let sim_snap = sim.lock().await.clone();
                let report_opt = controller::reporter::build_status_report(
                    &plan_cycle_event,
                    &sim_snap,
                    &ven_name,
                    None, // no single program_id in planning loop context
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
                _ = trigger_rx.changed() => trigger_rx.borrow().clone(),
            };
        }
    })
}
