// Simulator tick loop body, extracted to keep sim_tick/mod.rs under 200 lines.

use std::sync::Arc;
use tokio::sync::Mutex;

use crate::controller::arbiter::heater_emergency_modes;
use crate::controller::{MeasurementPort, WeatherForecastPort};
use crate::entities::asset::PlanTriggerSignal;
use crate::planner_events::PlannerEventTx;
use crate::simulator::SimState;
use crate::state::AppState;

#[allow(clippy::too_many_arguments)]
pub(crate) async fn tick_once(
    state: AppState,
    sim: Arc<Mutex<SimState>>,
    ven_name: String,
    trigger_tx: Arc<tokio::sync::watch::Sender<PlanTriggerSignal>>,
    data_dir: String,
    _event_tx: PlannerEventTx,
    persist_counter: u64,
    persist_every_ticks: u64,
    tick_s: u64,
    weather: Arc<dyn WeatherForecastPort>,
    asset_feeds: Arc<crate::entities::asset_params::AssetFeedConfig>,
    pv_measurement: Arc<dyn MeasurementPort>,
    base_load_measurement: Arc<dyn MeasurementPort>,
    notifier: crate::services::notify::Notifier,
    comms_loss_config: Option<crate::profile::comms_loss::CommsLossConfig>,
    grid_max_import_kw: f64,
    grid_max_export_kw: f64,
    telemetry: Arc<dyn crate::controller::telemetry_port::TelemetryPort>,
) -> u64 {
    let now = chrono::Utc::now();
    let dt_s = tick_s as f64;

    let _events = state.events().await;
    let ctx = super::context::resolve_tick_context(
        &state,
        now,
        weather.as_ref(),
        &asset_feeds,
        pv_measurement.as_ref(),
        base_load_measurement.as_ref(),
        comms_loss_config,
        grid_max_import_kw,
        grid_max_export_kw,
    )
    .await;

    let (
        tick_sensor,
        tick_sim_snap,
        tick_site_headroom,
        tick_forecast,
        tick_capacity_curves,
        cleared_fields,
        arbiter_outcome,
    ) = {
        // ev-usage-simulation: offer the EV's next simulated leave instant to the planner in
        // advance when plan-ahead is enabled (a no-op otherwise). Before the lock below: it
        // awaits on `AppState` and reads only the EV's schedule.
        super::usage_sim_plan_ahead::sync_plan_ahead_session(&state, &sim, now).await;

        let mut sim_guard = sim.lock().await;

        let cleared_fields =
            crate::simulator::inject::apply_state_injections(&ctx.inject, &mut sim_guard);

        let pre_snap = sim_guard.to_sim_snapshot();

        // `pre_snap` predates this tick's physics; peek_* preview `now` so the arbiter never sees a stale input.
        let (live_kw, live_released_kw) =
            super::feeds::live_previews(&sim_guard, &ctx, &pre_snap, now, dt_s);

        let thermostat_setpoints_kw =
            sim_guard.thermostat_setpoints_kw(ctx.inject.heater_setpoint_c);
        let mut outcome = super::setpoints::build_tick_setpoints(
            &ctx,
            &pre_snap,
            &thermostat_setpoints_kw,
            now,
            (&live_kw, &live_released_kw),
        );

        let resolved_pv_generation_limit = crate::controller::comms_loss::pv_generation_limit(
            &pre_snap,
            ctx.plan_snap.as_ref(),
            &ctx.capacity_schedule,
            &ctx.inject,
            now,
            outcome.pv_generation_limit_tighten_kw,
            ctx.comms_loss,
        );

        let heater_emergency_by_asset =
            heater_emergency_modes(&ctx.inject, &outcome.heater_emergency_mode, &pre_snap);

        sim_guard.tick(tick_inputs(
            &ctx,
            &resolved_pv_generation_limit,
            heater_emergency_by_asset,
            (dt_s, now),
            std::mem::take(&mut outcome.setpoints),
        ));

        let (tick_sensor, tick_sim_snap, tick_site_headroom, tick_forecast, tick_capacity_curves) =
            super::finalize::finalize_tick_outputs(&mut sim_guard, &ctx, now);

        (
            tick_sensor,
            tick_sim_snap,
            tick_site_headroom,
            tick_forecast,
            tick_capacity_curves,
            cleared_fields,
            outcome,
        )
    };

    let measured_net_kw = Some(tick_sim_snap.grid.net_power_kw);
    super::arbiter_glue::record_arbiter_outcome(
        &state,
        &notifier,
        &arbiter_outcome,
        measured_net_kw,
        now,
    )
    .await;
    let residual_kwh_by_asset = arbiter_outcome.residual_kwh_by_asset(dt_s, &tick_sim_snap);
    super::arbiter_glue::apply_residual_escalation(
        &state,
        &trigger_tx,
        &residual_kwh_by_asset,
        now,
    )
    .await;

    super::post_lock::after_tick(&state, &sim, cleared_fields, ctx.pv_clear, ctx.base_clear).await;

    // Publish consumes the snapshot and hands it back: one object per tick.
    super::publish::publish_sim_tick_result(
        tick_sensor,
        tick_sim_snap,
        tick_site_headroom,
        tick_forecast,
        tick_capacity_curves,
        &state,
        &trigger_tx,
        &ctx.rates_snap,
        dt_s,
        now,
        telemetry.as_ref(),
        &ven_name,
    )
    .await;

    super::post_lock::run_periodic_persist(persist_counter, persist_every_ticks, &sim, &data_dir)
        .await
}

#[cfg(test)]
#[path = "tick_tests.rs"]
mod tick_tests;

/// This tick's simulator inputs: the sim injects, the resolved PV generation limit, each
/// heater's emergency mode, each EV's departure, the request targets and the feeds.
fn tick_inputs(
    ctx: &super::context::TickContext,
    pv_limit: &crate::controller::dispatcher::ResolvedPvGenerationLimit,
    heater_emergency_by_asset: std::collections::HashMap<String, (Option<bool>, Option<bool>)>,
    (dt_s, now): (f64, chrono::DateTime<chrono::Utc>),
    setpoints: std::collections::HashMap<String, f64>,
) -> crate::simulator::TickInputs {
    crate::simulator::TickInputs {
        pv_irradiance_override: ctx.inject.pv_irradiance,
        pv_tau_s: ctx.inject.pv_tau_s,
        pv_generation_limit_override: pv_limit.limit_kw,
        pv_curtailment_source: pv_limit.source,
        ambient_temp_c_override: ctx.inject.ambient_temp_c,
        heater_temp_min_override: ctx.inject.heater_temp_min_c,
        heater_temp_max_override: ctx.inject.heater_temp_max_c,
        heater_emergency_by_asset,
        base_load_kw_override: ctx.inject.base_load_kw,
        base_load_alpha: ctx.inject.base_load_alpha,
        ev_plugged_override: ctx.inject.ev_plugged,
        ev_soc_target_override: ctx.inject.ev_soc_target,
        ev_departure_by_asset: ctx.ev_departure_by_asset.clone(),
        request_targets: ctx.request_targets(),
        feeds: ctx.feeds.clone(),
        ..crate::simulator::TickInputs::new(dt_s, now, setpoints)
    }
}
