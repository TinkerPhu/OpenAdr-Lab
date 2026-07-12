use chrono::Utc;
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};
use tracing::info;

use crate::controller::SolverPort;
use crate::entities::asset::PlanTrigger;
use crate::entities::asset_params::AssetParams;
use crate::entities::planner_params::{PlannerObjective, PlannerParams};
use crate::planner_events::{PlannerEvent, PlannerEventTx};
use crate::services::planning::PlanCycleInputs;
use crate::simulator::SimState;
use crate::state::AppState;

use super::progress_ticker::spawn_progress_ticker;
use crate::services::forecast::finish_plan_cycle;

#[allow(clippy::too_many_arguments)]
pub(crate) fn spawn_planning(
    state: AppState,
    planner: PlannerParams,
    grid_max_import_kw: f64,
    grid_max_export_kw: f64,
    asset_params: Vec<AssetParams>,
    solver: Arc<dyn SolverPort>,
    mut trigger_rx: tokio::sync::watch::Receiver<PlanTrigger>,
    sim: Arc<Mutex<SimState>>,
    active_objective: Arc<RwLock<PlannerObjective>>,
    event_tx: PlannerEventTx,
    notifier: crate::services::notify::Notifier,
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
            let now = crate::services::planning::align_to_step(wall_now, planner.plan_step_s);
            let rates = state.planned_tariffs().await;
            let capacity = state.capacity_state().await;
            let alert_windows = state.alert_windows().await;
            let simple_windows = state.simple_windows().await;
            let trigger = wake_trigger.clone();
            let trigger_reason = format!("{:?}", trigger);

            // Hard triggers (user action, state change) must not be constrained by the anchor
            // from a previous Periodic cycle — clear it so the next solve is fully free.
            if !matches!(trigger, PlanTrigger::Periodic) {
                state.set_anchor_until(None).await;
            }

            info!(trigger = %trigger_reason, "planner loop: starting plan cycle");

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
            let mut sim_snap =
                crate::services::planning::clone_sim_snapshot(&sim, &trigger_reason).await;

            // Patch the clone when pv_irradiance inject is pending and the tick hasn't
            // applied it yet. When the tick runs first, the clone already has the correct
            // offset and this is a no-op (inject_snap.pv_irradiance is None).
            crate::services::planning::apply_pending_pv_inject(&mut sim_snap, &inject_snap, now);

            // ── Emit solving_started ──────────────────────────────────────
            let num_slots = planner.plan_horizon_h as usize * 3600 / planner.plan_step_s as usize;
            let _ = event_tx.send(PlannerEvent::SolvingStarted {
                objective: obj,
                num_slots,
                triggered_at: now,
            });

            // ── Spawn 1 s progress ticker ─────────────────────────────────
            let (ticker_task, cancel_tx) = spawn_progress_ticker(event_tx.clone());

            // ── Run blocking HiGHS solve off the async runtime ────────────
            let solve_start = std::time::Instant::now();
            let snap = sim_snap.to_sim_snapshot();

            // Read before the blocking solve so heater tiers pin to the last adopted plan.
            let anchor_until = state.anchor_until().await;
            let current_plan = state.active_plan().await;
            let PlanCycleInputs {
                tariff_ts,
                n_slots,
                cum_s,
                lambda_sw,
                heater_c_terminal_eur_kwh,
                battery_c_terminal_eur_kwh,
                heater_anchor,
            } = crate::services::planning::build_plan_cycle_inputs(
                &rates,
                &planner,
                &asset_params,
                current_plan.as_ref(),
                anchor_until,
                now,
            );

            // Build per-asset MILP contexts from live simulator state.
            // This happens before spawn_blocking so asset states are captured at this instant.
            let asset_contexts = crate::services::planning::build_asset_contexts(
                &sim_snap,
                n_slots,
                &cum_s,
                now,
                ev_sess.as_ref(),
                heat_tgt.as_ref(),
                &asset_params,
                &planner,
                lambda_sw,
                heater_c_terminal_eur_kwh,
                battery_c_terminal_eur_kwh,
                &heater_anchor,
            );

            let solve_req = crate::services::planning::build_solve_request(
                asset_contexts,
                snap,
                tariff_ts,
                capacity,
                alert_windows,
                simple_windows,
                planner.clone(),
                grid_max_import_kw,
                grid_max_export_kw,
                asset_params.clone(),
                now,
                trigger.clone(),
                ev_sess,
                heat_tgt,
                shift_loads,
                bl_override,
                Some(obj),
                pv_forecast_override,
            );
            let mut plan = crate::services::PlanningService::solve_plan(&solver, solve_req).await;
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
                wall_now, // gate decay measures real plan age; aligned `now` can lag replan_s
            )
            .await;

            // Post-cycle outputs: WP4.3 notifications + WP3.6 forecasts.
            let prev = current_plan.as_ref();
            finish_plan_cycle(&state, &sim, &notifier, wall_now, prev, &cycle).await;

            info!(
                trigger = %trigger_reason,
                slot_count = cycle.plan.slots.len(),
                adopted = cycle.adopted,
                "plan cycle complete"
            );

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
    use chrono::Utc;
    use std::sync::Arc;
    use tokio::sync::{broadcast, watch, Mutex, RwLock};

    use crate::entities::asset::PlanTrigger;
    use crate::entities::planner_params::{PlannerObjective, PlannerParams};
    use crate::planner_events::PlannerEvent;
    use crate::services::test_support::mock_solver_port::MockSolverPort;
    use crate::simulator::SimState;
    use crate::state::AppState;

    use super::spawn_planning;

    /// Minimal `Plan` for test construction — no cycle in
    /// `spawn_planning_constructs_without_panic` ever runs to completion (the
    /// task is aborted right after spawn), so this value is never consumed.
    fn minimal_plan() -> crate::entities::plan::Plan {
        serde_json::from_value(serde_json::json!({
            "id": uuid::Uuid::new_v4().to_string(),
            "created_at": Utc::now().to_rfc3339(),
            "trigger": "PERIODIC",
            "horizon": {
                "start_time": "2026-01-01T00:00:00Z",
                "end_time": "2026-01-02T00:00:00Z",
                "step_size_s": 900,
                "num_steps": 96,
                "far_horizon": "2026-01-02T00:00:00Z"
            },
            "slots": [],
            "summary": {
                "total_cost_eur": 0.0,
                "total_co2_g": 0.0,
                "total_import_kwh": 0.0,
                "total_export_kwh": 0.0
            },
            "envelopes": [],
            "warnings": [],
            "objective_eur": 0.0,
            "friction_eur": 0.0
        }))
        .expect("minimal test Plan must deserialize")
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
        let solver = Arc::new(MockSolverPort::returning(minimal_plan()));
        let sim = minimal_sim();
        let active_objective = Arc::new(RwLock::new(PlannerObjective::default()));

        let handle = spawn_planning(
            AppState::new(),
            PlannerParams::default(),
            10.0,
            10.0,
            vec![],
            solver,
            trigger_rx,
            sim,
            active_objective,
            event_tx,
            crate::services::notify::Notifier::new(None),
        );
        handle.abort();
        let _ = trigger_tx; // keep alive until abort
                            // passes if no panic during construction and abort
    }
}
