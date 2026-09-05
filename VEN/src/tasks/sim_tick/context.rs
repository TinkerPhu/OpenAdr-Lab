//! Pre-lock tick context resolution — split out of `tick.rs` to keep it
//! under the tasks/ file-size cap. Bundles every state/port read that must
//! happen before `sim.lock()` (weather, real-measurement MQTT feeds,
//! inject/plan/capacity/dispatch/tariff snapshots, arbiter gates) into one
//! struct so `tick_once` stays a thin orchestrator.

use crate::controller::{MeasurementPort, WeatherForecastPort};
use crate::entities::asset_params::PvForecastParams;
use crate::entities::sim_inject::SimInjectState;
use crate::profile::comms_loss::CommsLossConfig;
use crate::state::AppState;

/// R-59: resolved comms-loss curtailment state for this tick — `active` is
/// the debounced "VTN has been unreachable long enough" verdict, computed
/// once here so both the PV resolver and the EV/heater/battery clamp read
/// the identical value. `None` overall (not just `active: false`) means the
/// profile has no `comms_loss:` section at all (opt-out fast path).
#[derive(Debug, Clone, Copy)]
pub(crate) struct CommsLossState {
    pub active: bool,
    pub max_power_pct: f64,
}

pub(crate) struct TickContext {
    pub inject: SimInjectState,
    /// Whether a one-shot inject field was present this tick and should be
    /// cleared post-lock — derived from `inject` here (pre-lock) so `tick.rs`
    /// doesn't need its own local bindings just to smuggle two bools out of
    /// the lock block.
    pub pv_clear: bool,
    pub base_clear: bool,
    pub weather_pv_kw_now: Option<f64>,
    /// Weather-sourced PV ceiling for each REMAINING plan slot, aligned to
    /// `plan_snap`'s own future-slot start times (not a uniform grid — the
    /// horizon is multi-zone). Feeds the site-headroom / capacity forecast
    /// through the same resolution the planner uses for `p_pv_kw`. `None`
    /// when there is no plan, no weather config, or the feed is stale — the
    /// forecast then falls back to the sin model, exactly as the planner does.
    pub weather_pv_kw_slots: Option<Vec<f64>>,
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
    pub comms_loss: Option<CommsLossState>,
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
    comms_loss_config: Option<CommsLossConfig>,
) -> TickContext {
    let inject = state.inject_state().await;
    let pv_clear = inject.pv_irradiance.is_some();
    let base_clear = inject.base_load_kw.is_some();
    let comms_loss = match comms_loss_config {
        Some(cfg) => {
            let vtn_status = state.vtn_connection_status().await;
            Some(CommsLossState {
                active: vtn_status.comms_lost_for(now, cfg.debounce_s),
                max_power_pct: cfg.max_power_pct,
            })
        }
        None => None,
    };

    let plan_snap = state.active_plan().await;
    // Resolved pre-lock (the port fetch is async) against the plan's own
    // remaining slot starts, so `build_forecast_frames` can stay sync inside
    // the lock and still see per-slot weather. One fetch + one series
    // evaluation serves both the instant and the slot grid.
    let slot_starts: Vec<_> = plan_snap
        .as_ref()
        .map(|plan| {
            plan.all_slots()
                .filter(|s| s.start >= now)
                .map(|s| s.start)
                .collect()
        })
        .unwrap_or_default();
    let (weather_pv_kw_now, weather_pv_kw_slots) =
        super::arbiter_glue::resolve_weather_pv_kw_for_tick(
            weather,
            weather_pv_params,
            now,
            &slot_starts,
        )
        .await;
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
        pv_clear,
        base_clear,
        weather_pv_kw_now,
        weather_pv_kw_slots,
        pv_measured_kw_now,
        base_load_measured_kw_now,
        base_load_heuristic_kw_now,
        plan_snap,
        capacity_snap: state.capacity_state().await,
        dispatch_windows: state.dispatch_windows().await,
        alert_windows: state.alert_windows().await,
        rates_snap: state.planned_tariffs().await,
        overlay_enabled: super::arbiter_glue::resolve_overlay_enabled(state, now).await,
        deviation_arbiter_enabled: state.deviation_arbiter_enabled().await,
        incumbent_lever: state.arbiter_active_lever().await,
        ev_session: state.ev_session().await,
        shiftable_loads: state.shiftable_loads().await,
        comms_loss,
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
            None,
        )
        .await
    }

    #[tokio::test]
    async fn comms_loss_is_none_when_profile_has_no_comms_loss_section() {
        let state = AppState::new();
        let ctx = resolve(&state).await;
        assert!(ctx.comms_loss.is_none());
    }

    #[tokio::test]
    async fn comms_loss_active_false_when_configured_but_vtn_connected() {
        let state = AppState::new();
        let cfg = CommsLossConfig {
            max_power_pct: 0.7,
            debounce_s: 60,
        };
        let ctx = resolve_tick_context(
            &state,
            now(),
            &crate::controller::NoopWeatherPort,
            None,
            &crate::controller::NoopMeasurementPort,
            false,
            &crate::controller::NoopMeasurementPort,
            false,
            Some(cfg),
        )
        .await;
        let cl = ctx
            .comms_loss
            .expect("comms_loss must be Some when configured");
        assert!(
            !cl.active,
            "VTN is connected by default — must not be active"
        );
        assert_eq!(cl.max_power_pct, 0.7);
    }

    #[tokio::test]
    async fn comms_loss_active_true_once_debounce_elapses() {
        let state = AppState::new();
        let t0 = now();
        state.record_vtn_poll_success(t0).await;
        state
            .record_vtn_poll_failure(t0, "boom".to_string(), 5.0)
            .await;
        let cfg = CommsLossConfig {
            max_power_pct: 0.7,
            debounce_s: 60,
        };
        let later = t0 + chrono::Duration::seconds(61);
        let ctx = resolve_tick_context(
            &state,
            later,
            &crate::controller::NoopWeatherPort,
            None,
            &crate::controller::NoopMeasurementPort,
            false,
            &crate::controller::NoopMeasurementPort,
            false,
            Some(cfg),
        )
        .await;
        let cl = ctx
            .comms_loss
            .expect("comms_loss must be Some when configured");
        assert!(cl.active, "past debounce window — must be active");
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
            daytime_profile_kw: std::array::from_fn(|bucket| vec![0.7 + 0.02 * bucket as f64; 24]),
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
