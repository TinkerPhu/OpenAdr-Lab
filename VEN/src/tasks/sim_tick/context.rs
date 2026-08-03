//! Pre-lock tick context resolution — split out of `tick.rs` to keep it
//! under the tasks/ file-size cap. Bundles every state/port read that must
//! happen before `sim.lock()` (weather, real-measurement MQTT feeds,
//! inject/plan/capacity/dispatch/tariff snapshots, arbiter gates) into one
//! struct so `tick_once` stays a thin orchestrator.

use crate::controller::{MeasurementPort, WeatherForecastPort};
use crate::entities::asset_params::PvForecastParams;
use crate::entities::sim_inject::SimInjectState;
use crate::state::AppState;

pub(crate) struct TickContext {
    pub inject: SimInjectState,
    pub weather_pv_kw_now: Option<f64>,
    pub pv_measured_kw_now: Option<f64>,
    pub base_load_measured_kw_now: Option<f64>,
    pub plan_snap: Option<crate::entities::plan::Plan>,
    pub capacity_snap: crate::entities::capacity::OadrCapacityState,
    pub dispatch_windows: Vec<crate::entities::capacity::DispatchWindow>,
    pub alert_windows: Vec<crate::entities::capacity::AlertWindow>,
    pub rates_snap: Vec<crate::entities::tariff_snapshot::TariffSnapshot>,
    pub overlay_enabled: bool,
    pub deviation_arbiter_enabled: bool,
    pub incumbent_lever: Option<String>,
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn resolve_tick_context(
    state: &AppState,
    now: chrono::DateTime<chrono::Utc>,
    weather: &dyn WeatherForecastPort,
    weather_pv_params: Option<&PvForecastParams>,
    pv_measurement: &dyn MeasurementPort,
    pv_measurement_enabled: bool,
    base_load_measurement: &dyn MeasurementPort,
    base_load_measurement_enabled: bool,
) -> TickContext {
    let inject = state.inject_state().await;

    let weather_pv_kw_now =
        super::arbiter_glue::resolve_weather_pv_kw_now(weather, weather_pv_params, now).await;
    let (pv_measured_kw_now, base_load_measured_kw_now) =
        super::arbiter_glue::resolve_measurements_now(
            pv_measurement,
            pv_measurement_enabled,
            base_load_measurement,
            base_load_measurement_enabled,
            now,
        )
        .await;

    TickContext {
        inject,
        weather_pv_kw_now,
        pv_measured_kw_now,
        base_load_measured_kw_now,
        plan_snap: state.active_plan().await,
        capacity_snap: state.capacity_state().await,
        dispatch_windows: state.dispatch_windows().await,
        alert_windows: state.alert_windows().await,
        rates_snap: state.planned_tariffs().await,
        overlay_enabled: super::arbiter_glue::resolve_overlay_enabled(state).await,
        deviation_arbiter_enabled: state.deviation_arbiter_enabled().await,
        incumbent_lever: state.arbiter_active_lever().await,
    }
}
