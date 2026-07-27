// Simulator tick loop body, extracted to keep sim_tick/mod.rs under 200 lines.

use std::sync::Arc;
use tokio::sync::Mutex;

use crate::controller::SimulatorPort;
use crate::controller::VtnPort;
use crate::controller::WeatherForecastPort;
use crate::entities::asset::PlanTrigger;
use crate::entities::asset_params::PvForecastParams;
use crate::planner_events::PlannerEventTx;
use crate::simulator::SimState;
use crate::state::AppState;

#[allow(clippy::too_many_arguments)]
pub(crate) async fn tick_once(
    state: AppState,
    sim: Arc<Mutex<SimState>>,
    ven_name: String,
    vtn: Arc<dyn VtnPort>,
    trigger_tx: Arc<tokio::sync::watch::Sender<PlanTrigger>>,
    data_dir: String,
    _event_tx: PlannerEventTx,
    mut persist_counter: u64,
    persist_every_ticks: u64,
    mut report_counter: u64,
    report_every_ticks: u64,
    tick_s: u64,
    weather: Arc<dyn WeatherForecastPort>,
    weather_pv_params: Option<PvForecastParams>,
) -> (u64, u64) {
    let now = chrono::Utc::now();
    let dt_s = tick_s as f64;

    // PHASE 0: Snapshot — events, inject, plan, capacity, tariffs, overlay flag
    let _events = state.events().await;
    let inject = state.inject_state().await;

    // Weather-sourced PV power for this instant (must happen before the sync lock).
    let weather_pv_kw_now = super::arbiter_glue::resolve_weather_pv_kw_now(
        weather.as_ref(),
        weather_pv_params.as_ref(),
        now,
    )
    .await;

    // Pre-tick: snapshot plan/capacity/tariffs for dispatcher
    let plan_snap = state.active_plan().await;
    let capacity_snap = state.capacity_state().await;
    let dispatch_windows = state.dispatch_windows().await;
    let alert_windows = state.alert_windows().await;
    let rates_snap = state.planned_tariffs().await;

    let overlay_enabled = super::arbiter_glue::resolve_overlay_enabled(&state).await;

    // Deviation-arbiter rollout gate + hysteresis state (read pre-lock).
    let deviation_arbiter_enabled = state.deviation_arbiter_enabled().await;
    let incumbent_lever = state.arbiter_active_lever().await;

    // Lock sim for physics only — no .await inside the block.
    let (
        tick_sensor,
        tick_sim_snap,
        tick_envelope,
        cleared_fields,
        pv_clear,
        base_clear,
        absorbed_kwh_by_asset,
        new_active_lever,
    ) = {
        let mut sim_guard = sim.lock().await;

        // PHASE 1: Apply Behaviour A one-shot injections; collect fields to clear.
        let cleared_fields = super::helpers::apply_sim_injections(&inject, &mut sim_guard);
        let pv_clear = inject.pv_irradiance.is_some();
        let base_clear = inject.base_load_kw.is_some();

        // PHASE 2: Build setpoints (dispatcher from MILP plan + capacity) then
        // run the deviation arbiter's reactive adjustment layer on top.
        let pre_snap = sim_guard
            .snapshot()
            .expect("SimState::snapshot is infallible");

        // `pre_snap` predates this tick's physics; peek_pv_kw/peek_base_load_kw
        // preview `now`'s values so the arbiter never sees a one-tick-stale input.
        let live_pv_kw = sim_guard.peek_pv_kw(
            now,
            dt_s,
            inject.pv_irradiance,
            inject.pv_irradiance_alpha,
            weather_pv_kw_now,
        );
        let live_base_load_kw =
            sim_guard.peek_base_load_kw(now, dt_s, inject.base_load_kw, inject.base_load_alpha);

        let outcome = super::helpers::build_tick_setpoints(
            &pre_snap,
            plan_snap.as_ref(),
            &capacity_snap,
            &inject,
            overlay_enabled,
            now,
            &dispatch_windows,
            &alert_windows,
            live_pv_kw,
            live_base_load_kw,
            deviation_arbiter_enabled,
            incumbent_lever.as_deref(),
        );

        let effective_capacity_for_pv = super::helpers::effective_capacity(&capacity_snap, &inject);
        let resolved_pv_export_limit = crate::controller::dispatcher::resolve_pv_export_limit_kw(
            plan_snap.as_ref(),
            &effective_capacity_for_pv,
            now,
            outcome.pv_export_limit_tighten_kw,
        );

        let (heater_emergency_curtail, heater_emergency_absorb) =
            super::arbiter_glue::resolve_heater_emergency_mode(
                &inject,
                outcome.heater_emergency_mode,
            );

        let absorbed_kwh_by_asset = outcome.absorbed_kwh_by_asset.clone();
        let new_active_lever = outcome.active_lever.map(|s| s.to_string());

        // PHASE 3: Simulator tick — apply setpoints → update device states.
        sim_guard.tick(
            dt_s,
            outcome.setpoints,
            now,
            inject.pv_irradiance,
            inject.pv_irradiance_alpha,
            inject.ambient_temp_c,
            inject.heater_temp_min_c,
            inject.heater_temp_max_c,
            inject.base_load_kw,
            inject.base_load_alpha,
            inject.ev_plugged,
            inject.ev_soc_target,
            weather_pv_kw_now,
            heater_emergency_curtail,
            heater_emergency_absorb,
            resolved_pv_export_limit.limit_kw,
            resolved_pv_export_limit.source,
        );

        // PHASE 4 (in-lock): extract snapshots and mutate history/grid/envelope.
        let (tick_sensor, tick_sim_snap, tick_envelope) =
            super::helpers::finalize_tick_outputs(&mut sim_guard, &capacity_snap, now);

        (
            tick_sensor,
            tick_sim_snap,
            tick_envelope,
            cleared_fields,
            pv_clear,
            base_clear,
            absorbed_kwh_by_asset,
            new_active_lever,
        )
    };

    // PHASE 3.5 (post-lock): arbiter hysteresis + residual escalation (§5.5).
    state.set_arbiter_active_lever(new_active_lever).await;
    super::arbiter_glue::apply_residual_escalation(
        &state,
        &trigger_tx,
        &absorbed_kwh_by_asset,
        now,
    )
    .await;

    // PHASE 1 (post-lock): clear one-shot inject fields.
    for field in cleared_fields {
        state.clear_inject_field(field).await;
    }
    if pv_clear {
        state.clear_inject_field("pv_irradiance").await;
    }
    if base_clear {
        state.clear_inject_field("base_load_kw").await;
    }

    // PHASE 5 (post-lock): async state publishes — sensor, shiftable, ledger, envelope.
    let snap_for_reports = tick_sim_snap.clone();
    let _sim_snapshot = super::publish::publish_sim_tick_result(
        tick_sensor,
        tick_sim_snap,
        tick_envelope,
        plan_snap.as_ref(),
        &state,
        &trigger_tx,
        &rates_snap,
        dt_s,
        now,
    )
    .await;

    // PHASE 6: Periodic measurement reports (T049)
    if report_every_ticks > 0 {
        report_counter += 1;
        if report_counter >= report_every_ticks {
            report_counter = 0;
            super::publish::run_measurement_reports(
                &state,
                &snap_for_reports,
                vtn.as_ref(),
                &ven_name,
                now,
            )
            .await;
        }
    }

    // PHASE 7: Periodic persist
    persist_counter += 1;
    if persist_counter >= persist_every_ticks {
        persist_counter = 0;
        super::publish::persist_sim_state(&sim, &data_dir).await;
    }

    (persist_counter, report_counter)
}

#[cfg(test)]
#[path = "tick_tests.rs"]
mod tick_tests;
