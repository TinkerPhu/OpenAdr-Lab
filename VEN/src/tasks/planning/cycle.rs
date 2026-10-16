//! One plan-cycle's worth of work, extracted out of `mod.rs`'s `spawn_planning`
//! loop to keep that file under the `tasks/` file-size cap (R-40 debt note
//! flagged this file for exactly this split "when next touched" — R-50's
//! weather wiring is what touched it).

use chrono::{DateTime, Utc};
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::info;

use crate::controller::{HistoryPort, SolverPort, WeatherForecastPort};
use crate::entities::asset::PlanTriggerSignal;
use crate::entities::asset_params::AssetParams;
use crate::entities::planner_params::{PlannerObjective, PlannerParams};
use crate::planner_events::{PlannerEvent, PlannerEventTx};
use crate::simulator::plan_context::{apply_pending_pv_inject, clone_sim_snapshot};
use crate::simulator::SimState;
use crate::state::AppState;

use crate::services::forecast::finish_plan_cycle;
use crate::tasks::progress_ticker::spawn_progress_ticker;

/// Run exactly one plan cycle: gather live state, build the solve request
/// (including R-50's weather-sourced PV forecast, resolved inside
/// `services::planning::build_solve_request`), solve, adopt if warranted, and
/// publish post-cycle outputs. No return value — callers only need the side effects.
#[allow(clippy::too_many_arguments)]
pub(super) async fn run_plan_cycle(
    state: &AppState,
    sim: &Arc<Mutex<SimState>>,
    planner: &PlannerParams,
    grid_max_import_kw: f64,
    grid_max_export_kw: f64,
    asset_params: &[AssetParams],
    solver: &Arc<dyn SolverPort>,
    active_objective: &crate::app_ctx::ActiveObjective,
    event_tx: &PlannerEventTx,
    notifier: &crate::services::notify::Notifier,
    weather: &Arc<dyn WeatherForecastPort>,
    weather_pv_params: &crate::entities::asset_params::PvWeatherParams,
    // The kind and its cause together: two parameters for one fact invited
    // them to disagree, and the reason string is derived rather than passed.
    signal: &PlanTriggerSignal,
    wall_now: DateTime<Utc>,
    now: DateTime<Utc>,
    history: Option<Arc<dyn HistoryPort>>,
    telemetry: &dyn crate::controller::telemetry_port::TelemetryPort,
) {
    let trigger = signal.trigger.clone();
    let reason = format!("{trigger:?}");
    let (trigger_reason, trigger_event_ids) = (reason.as_str(), signal.event_ids.as_slice());
    // One read of the world, before anything is solved against it.
    let st = super::cycle_state::read_cycle_state(state, active_objective).await;
    // Clone SimState snapshot so the Mutex is released immediately.
    // MILP solving takes 18-60s on Node1 ARM64; holding the lock would
    // block sim ticks and /capability reads for the entire duration.
    let mut sim_snap = clone_sim_snapshot(sim, trigger_reason).await;

    // Patch the clone when pv_irradiance inject is pending and the tick hasn't
    // applied it yet (no-op when the tick ran first — see fn docs).
    apply_pending_pv_inject(&mut sim_snap, &st.inject_snap, now);

    // The plan in force: the heater anchor pins against it, and the adoption
    // gate needs the same value again after the solve.
    let current_plan = state.active_plan().await;
    let obj = st.obj;
    let num_slots = planner.plan_horizon_h as usize * 3600 / planner.plan_step_s as usize;
    let (mut plan, solver_ms) = announced_solve(event_tx, obj, num_slots, now, async {
        let solve_req = super::assemble::assemble_solve_request(
            super::assemble::SolveAssembly {
                state,
                sim_snap: &sim_snap,
                planner,
                asset_params,
                grid_max_import_kw,
                grid_max_export_kw,
                history: history.as_ref(),
                now,
                trigger: trigger.clone(),
                current_plan: current_plan.as_ref(),
            },
            st,
        )
        .await;
        plan_for(obj, solver, solve_req, planner, now, &trigger).await
    })
    .await;
    // created_at records true wall-clock time so gate decay (elapsed_s) measures
    // real plan age. horizon.start_time = now (aligned) is the slot grid origin.
    plan.created_at = wall_now;
    info!(
        solver_ms,
        trigger = %trigger_reason,
        slots = plan.slots.len(),
        objective_eur = plan.objective_eur,
        "planner: solve complete"
    );

    let cycle = crate::services::PlanningService::adopt_if_warranted(
        plan,
        &trigger,
        trigger_reason,
        trigger_event_ids,
        planner.plan_adoption_threshold_eur,
        planner.plan_adoption_decay_s,
        planner.gate_switch_penalty_eur,
        &crate::services::planning::heater_stage_sizes_kw(asset_params),
        solver_ms,
        obj,
        state,
        event_tx,
        wall_now, // gate decay measures real plan age; aligned `now` can lag replan_s
    )
    .await;
    if cycle.adopted {
        // The fleet's view of the plan in force (openspec `opportunistic-objective` 6.3);
        // serialised and sent from a task of its own, so the cycle never waits on the broker.
        telemetry.publish_plan(Arc::new(cycle.plan.clone()));
    }

    // Post-cycle outputs: WP4.3 notifications + WP3.6 forecasts.
    let prev = current_plan.as_ref();
    finish_plan_cycle(
        state,
        &crate::simulator::SimHandle::new(sim.clone()),
        notifier,
        wall_now,
        prev,
        &cycle,
        weather,
        weather_pv_params,
        history,
        (grid_max_import_kw, grid_max_export_kw),
    )
    .await;

    info!(
        trigger = %trigger_reason,
        slot_count = cycle.plan.slots.len(),
        adopted = cycle.adopted,
        "plan cycle complete"
    );
}

/// The solve as SSE clients see it: `SolvingStarted`, then a progress tick every second until
/// `work` (assembly and solve) is done. Returns the plan and how long `work` took [ms].
async fn announced_solve(
    event_tx: &PlannerEventTx,
    objective: PlannerObjective,
    num_slots: usize,
    now: DateTime<Utc>,
    work: impl std::future::Future<Output = crate::entities::plan::Plan>,
) -> (crate::entities::plan::Plan, u64) {
    let _ = event_tx.send(PlannerEvent::SolvingStarted {
        objective,
        num_slots,
        triggered_at: now,
    });
    let (ticker_task, cancel_tx) = spawn_progress_ticker(event_tx.clone());
    let solve_start = std::time::Instant::now();
    let plan = work.await;
    let solver_ms = solve_start.elapsed().as_millis() as u64;
    let _ = cancel_tx.send(());
    ticker_task.await.ok();
    (plan, solver_ms)
}

/// The one guard of the planning cycle (openspec `opportunistic-objective` D0): every planning
/// objective solves exactly as before; the opportunistic one is not solved.
async fn plan_for(
    objective: PlannerObjective,
    solver: &Arc<dyn SolverPort>,
    solve_req: crate::controller::SolveRequest,
    planner: &PlannerParams,
    now: DateTime<Utc>,
    trigger: &crate::entities::asset::PlanTrigger,
) -> crate::entities::plan::Plan {
    if objective.dispatches_from_plan() {
        crate::services::PlanningService::solve_plan(solver, solve_req).await
    } else {
        crate::controller::milp_planner::plan_without_forecast(
            planner,
            now,
            trigger.clone(),
            objective,
        )
    }
}
