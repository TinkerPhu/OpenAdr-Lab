// Simulator tick loop body, extracted to keep sim_tick/mod.rs under 200 lines.

use std::sync::Arc;
use tokio::sync::Mutex;

use crate::controller::{
    HistoryPort, MeasurementPort, SimulatorPort, VtnPort, WeatherForecastPort,
};
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
    persist_counter: u64,
    persist_every_ticks: u64,
    report_counter: u64,
    report_every_ticks: u64,
    tick_s: u64,
    weather: Arc<dyn WeatherForecastPort>,
    weather_pv_params: Option<PvForecastParams>,
    pv_co2_g_kwh: f64,
    pv_measurement: Arc<dyn MeasurementPort>,
    pv_measurement_enabled: bool,
    base_load_measurement: Arc<dyn MeasurementPort>,
    base_load_measurement_enabled: bool,
    notifier: crate::services::notify::Notifier,
    history: Option<Arc<dyn HistoryPort>>,
) -> (u64, u64) {
    let now = chrono::Utc::now();
    let dt_s = tick_s as f64;

    let _events = state.events().await;
    let ctx = super::context::resolve_tick_context(
        &state,
        now,
        weather.as_ref(),
        weather_pv_params.as_ref(),
        pv_measurement.as_ref(),
        pv_measurement_enabled,
        base_load_measurement.as_ref(),
        base_load_measurement_enabled,
    )
    .await;

    let (
        tick_sensor,
        tick_sim_snap,
        tick_envelope,
        tick_forecast,
        tick_capacity_curves,
        cleared_fields,
        pv_clear,
        base_clear,
        absorbed_kwh_by_asset,
        (new_active_lever, arbiter_net_kw, arbiter_dev_kw),
    ) = {
        let mut sim_guard = sim.lock().await;

        let cleared_fields = super::helpers::apply_sim_injections(&ctx.inject, &mut sim_guard);
        let pv_clear = ctx.inject.pv_irradiance.is_some();
        let base_clear = ctx.inject.base_load_kw.is_some();

        let pre_snap = sim_guard
            .snapshot()
            .expect("SimState::snapshot is infallible");

        // `pre_snap` predates this tick's physics; peek_* preview `now` so the arbiter never sees a stale input.
        let live_pv_kw = sim_guard.peek_pv_kw(
            now,
            dt_s,
            ctx.inject.pv_irradiance,
            ctx.inject.pv_irradiance_alpha,
            ctx.weather_pv_kw_now,
            ctx.pv_measured_kw_now,
        );
        let live_base_load_kw = sim_guard.peek_base_load_kw(
            now,
            dt_s,
            ctx.inject.base_load_kw,
            ctx.inject.base_load_alpha,
            ctx.base_load_measured_kw_now,
            ctx.base_load_heuristic_kw_now,
        );

        let outcome = super::helpers::build_tick_setpoints(
            &pre_snap,
            ctx.plan_snap.as_ref(),
            &ctx.inject,
            ctx.overlay_enabled,
            now,
            &ctx.dispatch_windows,
            &ctx.alert_windows,
            live_pv_kw,
            live_base_load_kw,
            ctx.deviation_arbiter_enabled,
            ctx.incumbent_lever.as_deref(),
        );

        let resolved_pv_generation_limit = super::helpers::resolve_pv_limit(
            ctx.plan_snap.as_ref(),
            &ctx.capacity_snap,
            &ctx.inject,
            now,
            outcome.pv_generation_limit_tighten_kw,
        );

        let (heater_emergency_curtail, heater_emergency_absorb) =
            super::arbiter_glue::resolve_heater_emergency_mode(
                &ctx.inject,
                outcome.heater_emergency_mode,
            );

        let absorbed_kwh_by_asset = outcome.absorbed_kwh_by_asset.clone();
        let new_active_lever = outcome.active_lever.map(|s| s.to_string());
        let (arbiter_net_kw, arbiter_dev_kw) = (outcome.net_kw, outcome.dev_kw);

        sim_guard.tick(
            dt_s,
            outcome.setpoints,
            now,
            ctx.inject.pv_irradiance,
            ctx.inject.pv_irradiance_alpha,
            ctx.inject.ambient_temp_c,
            ctx.inject.heater_temp_min_c,
            ctx.inject.heater_temp_max_c,
            ctx.inject.base_load_kw,
            ctx.inject.base_load_alpha,
            ctx.inject.ev_plugged,
            ctx.inject.ev_soc_target,
            ctx.weather_pv_kw_now,
            heater_emergency_curtail,
            heater_emergency_absorb,
            resolved_pv_generation_limit.limit_kw,
            resolved_pv_generation_limit.source,
            ctx.pv_measured_kw_now,
            ctx.base_load_measured_kw_now,
            ctx.base_load_heuristic_kw_now,
        );

        let (tick_sensor, tick_sim_snap, tick_envelope, tick_forecast, tick_capacity_curves) =
            super::finalize::finalize_tick_outputs(
                &mut sim_guard,
                &ctx.capacity_snap,
                ctx.plan_snap.as_ref(),
                ctx.ev_session.as_ref(),
                &ctx.shiftable_loads,
                &ctx.shiftable_runtimes,
                now,
            );

        (
            tick_sensor,
            tick_sim_snap,
            tick_envelope,
            tick_forecast,
            tick_capacity_curves,
            cleared_fields,
            pv_clear,
            base_clear,
            absorbed_kwh_by_asset,
            (new_active_lever, arbiter_net_kw, arbiter_dev_kw),
        )
    };

    let arbiter_summary = (new_active_lever, arbiter_net_kw, arbiter_dev_kw);
    super::arbiter_glue::record_arbiter_outcome(&state, &notifier, arbiter_summary, now).await;
    super::arbiter_glue::apply_residual_escalation(
        &state,
        &trigger_tx,
        &absorbed_kwh_by_asset,
        now,
    )
    .await;

    super::post_lock::clear_inject_fields(&state, cleared_fields, pv_clear, base_clear).await;

    let snap_for_reports = tick_sim_snap.clone();
    let _sim_snapshot = super::publish::publish_sim_tick_result(
        tick_sensor,
        tick_sim_snap,
        tick_envelope,
        tick_forecast,
        tick_capacity_curves,
        ctx.plan_snap.as_ref(),
        &state,
        &trigger_tx,
        &ctx.rates_snap,
        dt_s,
        now,
        pv_co2_g_kwh,
    )
    .await;

    super::post_lock::run_periodic_reports_and_persist(
        report_counter,
        report_every_ticks,
        persist_counter,
        persist_every_ticks,
        &state,
        &snap_for_reports,
        vtn.as_ref(),
        &ven_name,
        now,
        history,
        &sim,
        &data_dir,
    )
    .await
}

#[cfg(test)]
#[path = "tick_tests.rs"]
mod tick_tests;
