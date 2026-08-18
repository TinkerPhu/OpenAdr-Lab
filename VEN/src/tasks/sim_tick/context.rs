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
    /// BL-40: the site's learned base-load heuristic, sampled at this tick's
    /// `now`, when one has been learned for `ids::ASSET_BASE_LOAD` (cold
    /// start / never-learned yet is `None`). Resolved once here so both
    /// `peek_base_load_kw` (pre-lock) and `SimState::tick` (in-lock) receive
    /// the identical value — see design.md D1.
    pub base_load_heuristic_kw_now: Option<f64>,
    pub plan_snap: Option<crate::entities::plan::Plan>,
    pub capacity_snap: crate::entities::capacity::OadrCapacityState,
    pub dispatch_windows: Vec<crate::entities::capacity::DispatchWindow>,
    pub alert_windows: Vec<crate::entities::capacity::AlertWindow>,
    pub rates_snap: Vec<crate::entities::tariff_snapshot::TariffSnapshot>,
    pub overlay_enabled: bool,
    pub deviation_arbiter_enabled: bool,
    pub incumbent_lever: Option<String>,
    /// Live device-session state for the site-headroom forecast
    /// (`simulator::forecast::build_forecast_frames`) — read fresh here
    /// (pre-lock, async) rather than from the plan, since a shiftable
    /// load's window/duration is a live scheduling fact, not a planning
    /// result.
    pub ev_session: Option<crate::entities::device_session::EvSession>,
    pub shiftable_loads: Vec<crate::entities::device_session::ShiftableLoad>,
    pub shiftable_runtimes: Vec<crate::entities::device_session::ShiftableLoadRuntime>,
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
    let base_load_heuristic_kw_now = state
        .asset_heuristics()
        .await
        .get(crate::ids::ASSET_BASE_LOAD)
        .map(|h| h.sample_kw(now));

    TickContext {
        inject,
        weather_pv_kw_now,
        pv_measured_kw_now,
        base_load_measured_kw_now,
        base_load_heuristic_kw_now,
        plan_snap: state.active_plan().await,
        capacity_snap: state.capacity_state().await,
        dispatch_windows: state.dispatch_windows().await,
        alert_windows: state.alert_windows().await,
        rates_snap: state.planned_tariffs().await,
        overlay_enabled: super::arbiter_glue::resolve_overlay_enabled(state, now).await,
        deviation_arbiter_enabled: state.deviation_arbiter_enabled().await,
        incumbent_lever: state.arbiter_active_lever().await,
        ev_session: state.ev_session().await,
        shiftable_loads: state.shiftable_loads().await,
        shiftable_runtimes: state.shiftable_runtimes().await,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entities::design_vocabulary::AssetHeuristics;
    use chrono::TimeZone;
    use std::collections::HashMap;

    fn now() -> chrono::DateTime<chrono::Utc> {
        chrono::Utc.with_ymd_and_hms(2026, 7, 14, 12, 0, 0).unwrap()
    }

    async fn resolve(state: &AppState) -> TickContext {
        resolve_tick_context(
            state,
            now(),
            &crate::controller::NoopWeatherPort,
            None,
            &crate::controller::NoopMeasurementPort,
            false,
            &crate::controller::NoopMeasurementPort,
            false,
        )
        .await
    }

    #[tokio::test]
    async fn base_load_heuristic_kw_now_is_none_without_a_learned_heuristic() {
        let state = AppState::new();
        let ctx = resolve(&state).await;
        assert_eq!(ctx.base_load_heuristic_kw_now, None);
    }

    #[tokio::test]
    async fn base_load_heuristic_kw_now_matches_sample_kw_when_heuristic_present() {
        let state = AppState::new();
        let heuristic = AssetHeuristics {
            asset_id: crate::ids::ASSET_BASE_LOAD.to_string(),
            daytime_profile_kw: [vec![0.7; 24], vec![0.9; 24]],
            seasonal_factor: 1.05,
            last_updated: Some(now()),
            recent_mean_abs_error_kw: None,
        };
        let mut map = HashMap::new();
        map.insert(crate::ids::ASSET_BASE_LOAD.to_string(), heuristic.clone());
        state.set_asset_heuristics(map).await;

        let ctx = resolve(&state).await;
        assert_eq!(
            ctx.base_load_heuristic_kw_now,
            Some(heuristic.sample_kw(now()))
        );
    }
}
