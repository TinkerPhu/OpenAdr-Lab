use chrono::{DateTime, Utc};
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

/// Align a UTC timestamp down to the nearest `step_s`-second boundary.
///
/// All plan replans that fire within the same `step_s` window produce an identical
/// `now_aligned`, giving the acceptance gate slot-comparable plans and making successive
/// plans differ by exactly an integer multiple of `step_s` — never an arbitrary offset.
/// Uses `rem_euclid` so pre-epoch timestamps (negative unix) are handled correctly.
fn align_to_step(raw: DateTime<Utc>, step_s: u64) -> DateTime<Utc> {
    let ts = raw.timestamp();
    let step = step_s as i64;
    DateTime::<Utc>::from_timestamp(ts - ts.rem_euclid(step), 0)
        .expect("step-aligned timestamp is always valid")
}

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
            let wall_now = Utc::now();
            // Align to the nearest step boundary so all replans within the same window
            // share identical slot grids (gate stability, warm-start prerequisite).
            // wall_now is kept separately for Plan.created_at (gate decay uses real age).
            let now = align_to_step(wall_now, planner.plan_step_s);
            let rates = state.planned_tariffs().await;
            let capacity = state.capacity_state().await;
            let trigger = wake_trigger.clone();
            let trigger_reason = format!("{:?}", trigger);

            // Hard triggers (user action, state change) must not be constrained by the anchor
            // from a previous Periodic cycle — clear it so the next solve is fully free.
            if !matches!(trigger, PlanTrigger::Periodic) {
                state.set_anchor_until(None).await;
            }

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
            let n_slots: usize = planner.plan_zones.iter().map(|z| z.slots).sum();
            let cum_s: Vec<i64> = {
                let mut v = Vec::with_capacity(n_slots + 1);
                v.push(0i64);
                for zone in &planner.plan_zones {
                    for _ in 0..zone.slots {
                        v.push(v.last().unwrap() + zone.step_s as i64);
                    }
                }
                v
            };
            let lambda_sw = asset_params
                .iter()
                .find_map(|p| match p {
                    AssetParams::Heater(h) => Some(h.switching_penalty_eur),
                    _ => None,
                })
                .unwrap_or(0.0);

            // Read current plan and anchor_until before the blocking solve so the heater
            // tier binaries are pinned to the last adopted plan within the anchor window.
            let anchor_until = state.anchor_until().await;
            let current_plan = state.active_plan().await;
            let heater_anchor = crate::services::planning::build_heater_anchor(
                current_plan.as_ref(),
                anchor_until,
                now,
                n_slots,
            );

            // Average import tariff over the planning horizon — used to auto-compute
            // terminal energy reward coefficients for storage assets.
            let avg_imp_eur_kwh = {
                let total: f64 = (0..n_slots)
                    .map(|t| {
                        let slot_t = now + chrono::Duration::seconds(cum_s[t]);
                        tariff_ts
                            .import_eur_kwh
                            .interpolate_at(slot_t)
                            .unwrap_or(0.25)
                    })
                    .sum();
                if n_slots > 0 {
                    total / n_slots as f64
                } else {
                    0.25
                }
            };

            // Resolve c_terminal_eur_kwh per asset type. Battery and heater get
            // different formulas; EV gets 0.0 (deadline constraint handles incentive).
            // Profile override (Some(x)) takes precedence over auto-computed value.
            let heater_c_terminal_eur_kwh = asset_params
                .iter()
                .find_map(|p| match p {
                    AssetParams::Heater(h) => Some(
                        h.c_terminal_eur_kwh
                            .unwrap_or(avg_imp_eur_kwh + planner.c_ctrl_imp_malus_eur_kwh),
                    ),
                    _ => None,
                })
                .unwrap_or(0.0);
            let battery_c_terminal_eur_kwh = asset_params
                .iter()
                .find_map(|p| match p {
                    AssetParams::Battery(b) => Some(
                        b.c_terminal_eur_kwh
                            .unwrap_or(avg_imp_eur_kwh * b.round_trip_efficiency),
                    ),
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
                        &cum_s,
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
                        if matches!(cfg, crate::assets::AssetConfig::Heater(_)) {
                            heater_anchor.clone()
                        } else {
                            vec![]
                        },
                    )
                })
                .collect();

            let mut plan = tokio::task::spawn_blocking(move || {
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
            // created_at records true wall-clock time so gate decay (elapsed_s) measures
            // real plan age. horizon.start_time = now (aligned) is the slot grid origin.
            plan.created_at = wall_now;
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
                planner.gate_switch_penalty_eur,
                solver_ms,
                obj,
                &state,
                &event_tx,
                // wall_now: gate decay measures real plan age (created_at is wall time).
                // Aligned `now` would give elapsed_s ≤ 0 when the step grid doesn't
                // advance between replans (e.g. step_s=600, replan=300).
                wall_now,
            )
            .await;

            // Refresh site envelope immediately after each plan cycle.
            {
                let sim_snap = sim.lock().await.to_sim_snapshot();
                let env = controller::envelope::compute_envelope(&sim_snap, wall_now);
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
                    wall_now,
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
    use super::align_to_step;
    use chrono::{TimeZone, Utc};
    use std::sync::Arc;
    use tokio::sync::{broadcast, watch, Mutex, RwLock};

    use crate::entities::asset::PlanTrigger;
    use crate::entities::planner_params::{PlannerObjective, PlannerParams};
    use crate::planner_events::PlannerEvent;
    use crate::services::test_support::mock_vtn::MockVtn;
    use crate::simulator::SimState;
    use crate::state::AppState;

    use super::spawn_planning;

    #[test]
    fn test_align_to_step_rounds_down() {
        let make = |h: u32, m: u32, s: u32| Utc.with_ymd_and_hms(2026, 4, 11, h, m, s).unwrap();

        // step_s = 600 (10 min)
        assert_eq!(align_to_step(make(14, 23, 47), 600), make(14, 20, 0));
        assert_eq!(align_to_step(make(14, 20, 0), 600), make(14, 20, 0)); // already aligned
        assert_eq!(align_to_step(make(14, 29, 59), 600), make(14, 20, 0));

        // step_s = 300 (5 min)
        assert_eq!(align_to_step(make(14, 23, 47), 300), make(14, 20, 0));
        assert_eq!(align_to_step(make(14, 25, 0), 300), make(14, 25, 0)); // already aligned
        assert_eq!(align_to_step(make(14, 24, 59), 300), make(14, 20, 0));

        // result timestamp is always an exact multiple of step_s from epoch
        for step_s in [300u64, 600, 900, 1800] {
            let raw = make(14, 23, 47);
            let aligned = align_to_step(raw, step_s);
            assert_eq!(
                aligned.timestamp() % step_s as i64,
                0,
                "step_s={step_s}: aligned timestamp not a multiple of step_s"
            );
        }
    }

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
