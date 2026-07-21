//! One plan-cycle's worth of work, extracted out of `mod.rs`'s `spawn_planning`
//! loop to keep that file under the `tasks/` file-size cap (R-40 debt note
//! flagged this file for exactly this split "when next touched" — R-50's
//! weather wiring is what touched it).

use chrono::{DateTime, Utc};
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};
use tracing::info;

use crate::controller::{SolverPort, WeatherForecastPort};
use crate::entities::asset::PlanTrigger;
use crate::entities::asset_params::{AssetParams, PvForecastParams};
use crate::entities::planner_params::{PlannerObjective, PlannerParams};
use crate::planner_events::{PlannerEvent, PlannerEventTx};
use crate::services::planning::PlanCycleInputs;
use crate::simulator::plan_context::{
    apply_pending_pv_inject, build_asset_contexts, clone_sim_snapshot,
};
use crate::simulator::SimState;
use crate::state::AppState;

use crate::services::forecast::finish_plan_cycle;
use crate::tasks::progress_ticker::spawn_progress_ticker;

/// Run exactly one plan cycle: gather live state, build the solve request
/// (including R-50's weather-sourced PV forecast, resolved inside
/// `services::planning::build_solve_request`), solve, adopt if warranted,
/// and publish post-cycle outputs. No return value — callers only need the
/// side effects (state updates, notifications, forecasts).
#[allow(clippy::too_many_arguments)]
pub(super) async fn run_plan_cycle(
    state: &AppState,
    sim: &Arc<Mutex<SimState>>,
    planner: &PlannerParams,
    grid_max_import_kw: f64,
    grid_max_export_kw: f64,
    asset_params: &[AssetParams],
    solver: &Arc<dyn SolverPort>,
    active_objective: &Arc<RwLock<PlannerObjective>>,
    event_tx: &PlannerEventTx,
    notifier: &crate::services::notify::Notifier,
    weather: &Arc<dyn WeatherForecastPort>,
    weather_pv_params: Option<&PvForecastParams>,
    trigger: PlanTrigger,
    trigger_reason: &str,
    wall_now: DateTime<Utc>,
    now: DateTime<Utc>,
) {
    let rates = state.planned_tariffs().await;
    let capacity = state.capacity_state().await;
    let alert_windows = state.alert_windows().await;
    let simple_windows = state.simple_windows().await;

    let ev_sess = state.ev_session().await;
    let heat_tgt = state.heater_target().await;
    let shift_loads = state.shiftable_loads().await;
    let bl_override = state.baseline_override().await;
    // WP5.2 (BL-14): resolved here (async) and threaded down as a
    // plain owned value — build_milp_inputs et al. are sync/pure.
    let asset_heuristics = state.asset_heuristics().await;
    let obj = *active_objective.read().await;
    // Read inject state BEFORE cloning the sim: the one-shot pv_irradiance
    // inject is cleared by the sim tick after applying it — reading after the
    // clone can race that clear and lose the pending value (stale offset=0).
    let inject_snap = state.inject_state().await;
    let pv_forecast_override = inject_snap.pv_plan_kw;
    // Clone SimState snapshot so the Mutex is released immediately.
    // MILP solving takes 18-60s on Pi4 ARM64; holding the lock would
    // block sim ticks and /capability reads for the entire duration.
    let mut sim_snap = clone_sim_snapshot(sim, trigger_reason).await;

    // Patch the clone when pv_irradiance inject is pending and the tick hasn't
    // applied it yet (no-op when the tick ran first — see fn docs).
    apply_pending_pv_inject(&mut sim_snap, &inject_snap, now);

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
        planner,
        asset_params,
        current_plan.as_ref(),
        anchor_until,
        now,
    );

    // Build per-asset MILP contexts from live simulator state.
    // This happens before spawn_blocking so asset states are captured at this instant.
    let asset_contexts = build_asset_contexts(
        &sim_snap,
        n_slots,
        &cum_s,
        now,
        ev_sess.as_ref(),
        heat_tgt.as_ref(),
        asset_params,
        planner,
        lambda_sw,
        heater_c_terminal_eur_kwh,
        battery_c_terminal_eur_kwh,
        &heater_anchor,
    );

    // R-50: services::planning::build_solve_request resolves the
    // weather-sourced PV forecast internally (staleness/config gate in
    // entities::solar::resolve_weather_pv_kw).
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
        asset_params.to_vec(),
        now,
        trigger.clone(),
        ev_sess,
        heat_tgt,
        shift_loads,
        bl_override,
        Some(obj),
        pv_forecast_override,
        asset_heuristics,
        weather,
        weather_pv_params,
        wall_now,
        &cum_s,
        n_slots,
    )
    .await;
    let mut plan = crate::services::PlanningService::solve_plan(solver, solve_req).await;
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
        trigger_reason,
        planner.plan_adoption_threshold_eur,
        planner.plan_adoption_decay_s,
        planner.gate_switch_penalty_eur,
        solver_ms,
        obj,
        state,
        event_tx,
        wall_now, // gate decay measures real plan age; aligned `now` can lag replan_s
    )
    .await;

    // Post-cycle outputs: WP4.3 notifications + WP3.6 forecasts.
    let prev = current_plan.as_ref();
    finish_plan_cycle(
        state,
        sim,
        notifier,
        wall_now,
        prev,
        &cycle,
        weather,
        weather_pv_params,
    )
    .await;

    info!(
        trigger = %trigger_reason,
        slot_count = cycle.plan.slots.len(),
        adopted = cycle.adopted,
        "plan cycle complete"
    );
}
