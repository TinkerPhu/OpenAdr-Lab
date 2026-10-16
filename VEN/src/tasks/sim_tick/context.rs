//! Pre-lock tick context resolution — split out of `tick.rs` to keep it
//! under the tasks/ file-size cap. Bundles every state/port read that must
//! happen before `sim.lock()` (weather, real-measurement MQTT feeds,
//! inject/plan/capacity/dispatch/tariff snapshots, arbiter gates) into one
//! struct so `tick_once` stays a thin orchestrator.

use crate::controller::comms_loss::CommsLossState;
use crate::controller::{MeasurementPort, WeatherForecastPort};
use crate::entities::sim_inject::SimInjectState;
use crate::profile::comms_loss::CommsLossConfig;
use crate::state::AppState;

pub(crate) struct TickContext {
    pub inject: SimInjectState,
    /// Whether a one-shot inject field was present this tick and should be
    /// cleared post-lock — derived from `inject` here (pre-lock) so `tick.rs`
    /// doesn't need its own local bindings just to smuggle two bools out of
    /// the lock block.
    pub pv_clear: bool,
    pub base_clear: bool,
    /// This tick's external feeds per asset (W5b): each array's weather estimate and series,
    /// the measured asset's reading, each base load's learned heuristic. Resolved once here so
    /// the previews (pre-lock) and `SimState::tick` (in-lock) receive the identical values.
    pub feeds: crate::simulator::AssetTickFeeds,
    /// site-capacity-seam-unification / R-72: the site's genuine physical/
    /// interconnection rating (`profile.grid.max_import_kw`/`max_export_kw`),
    /// resolved once at startup and threaded down alongside every other
    /// tick input — same value the planning path already receives, now also
    /// reaching `compute_site_headroom`/`compute_site_capacity_curve`.
    pub grid_max_import_kw: f64,
    pub grid_max_export_kw: f64,
    pub plan_snap: Option<crate::entities::plan::Plan>,
    /// The priority-resolved capacity schedule (limits, subscriptions, reservations) —
    /// every tick reader (arbiter limit pass, PV generation limit, grid asset limits)
    /// asks it at `now`, through `entities::capacity::effective_capacity_limit`.
    pub capacity_schedule: Vec<crate::entities::capacity::CapacitySnapshot>,
    pub dispatch_windows: Vec<crate::entities::capacity::DispatchWindow>,
    pub alert_windows: Vec<crate::entities::capacity::AlertWindow>,
    pub rates_snap: Vec<crate::entities::tariff_snapshot::TariffSnapshot>,
    /// Each EV's opportunistic-charging switch this tick.
    pub overlay_enabled: super::arbiter_glue::OverlayEnabled,
    pub deviation_arbiter_enabled: bool,
    pub incumbent_lever: Option<String>,
    /// Last tick's storage bridges (R-104), so each carried storage command drops its own.
    pub bridge_kw_by_asset: std::collections::HashMap<String, f64>,
    /// GB-47 limit-enforcement gate and that pass's own last-tick lever.
    pub limit_enforcement_enabled: bool,
    pub limit_incumbent_lever: Option<String>,
    /// The departure of the session governing each EV now, by asset id — read fresh here
    /// (pre-lock, async) so a departure time is a live scheduling fact, not a planning
    /// result; each EV gets its own (`TickInputs::ev_departure_by_asset`).
    pub ev_departure_by_asset: std::collections::HashMap<String, chrono::DateTime<chrono::Utc>>,
    pub comms_loss: Option<CommsLossState>,
    /// The active requests, read only while the adopted plan's objective dispatches without
    /// it (the opportunistic objective); `None` under every planning objective, which reads no
    /// request state here (openspec `opportunistic-objective` D0).
    pub opportunistic: Option<crate::controller::arbiter::OpportunisticRequests>,
}

impl TickContext {
    /// The request targets the simulator hands each asset this tick: empty under every planning
    /// objective, which injects no request target (openspec `opportunistic-objective` D0).
    pub(crate) fn request_targets(&self) -> crate::controller::arbiter::RequestTargets {
        self.opportunistic
            .as_ref()
            .map(|r| r.targets.clone())
            .unwrap_or_default()
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn resolve_tick_context(
    state: &AppState,
    now: chrono::DateTime<chrono::Utc>,
    weather: &dyn WeatherForecastPort,
    asset_feeds: &crate::entities::asset_params::AssetFeedConfig,
    pv_measurement: &dyn MeasurementPort,
    base_load_measurement: &dyn MeasurementPort,
    comms_loss_config: Option<CommsLossConfig>,
    grid_max_import_kw: f64,
    grid_max_export_kw: f64,
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
    let snow = state.pv_snow_state().await;
    let (weather_pv_kw, weather_pv_forecast) =
        super::feeds::resolve_weather_pv_for_tick(weather, &asset_feeds.pv_weather, snow, now)
            .await;
    let measured_kw = super::feeds::resolve_measurements_now(
        (pv_measurement, asset_feeds.measured_pv.as_deref()),
        (
            base_load_measurement,
            asset_feeds.measured_base_load.as_deref(),
        ),
        now,
    )
    .await;
    let feeds = crate::simulator::AssetTickFeeds {
        weather_pv_kw,
        weather_pv_forecast,
        measured_kw,
        heuristic: state.asset_heuristics().await,
    };
    let (overlay_enabled, ev_sessions_now) =
        super::arbiter_glue::resolve_overlay_enabled(state, now).await;
    let opportunistic = match plan_snap.as_ref() {
        Some(plan) if !plan.objective.dispatches_from_plan() => {
            Some(crate::controller::arbiter::opportunistic_requests(
                now,
                &ev_sessions_now,
                &state.heater_targets().await,
                &state.shiftable_loads().await,
            ))
        }
        _ => None,
    };

    TickContext {
        inject,
        pv_clear,
        base_clear,
        feeds,
        grid_max_import_kw,
        grid_max_export_kw,
        plan_snap,
        capacity_schedule: state.planned_capacity_limits().await,
        dispatch_windows: state.dispatch_windows().await,
        alert_windows: state.alert_windows().await,
        rates_snap: state.planned_tariffs().await,
        overlay_enabled,
        deviation_arbiter_enabled: state.deviation_arbiter_enabled().await,
        incumbent_lever: state.arbiter_active_lever().await,
        bridge_kw_by_asset: state.arbiter_bridge_kw_by_asset().await,
        limit_enforcement_enabled: state.limit_enforcement_enabled().await,
        limit_incumbent_lever: state.limit_active_lever().await,
        ev_departure_by_asset: ev_sessions_now
            .into_iter()
            .map(|(id, s)| (id, s.departure_time))
            .collect(),
        comms_loss,
        opportunistic,
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
            &Default::default(),
            &crate::controller::NoopMeasurementPort,
            &crate::controller::NoopMeasurementPort,
            None,
            100.0,
            100.0,
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
            &Default::default(),
            &crate::controller::NoopMeasurementPort,
            &crate::controller::NoopMeasurementPort,
            Some(cfg),
            100.0,
            100.0,
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
            &Default::default(),
            &crate::controller::NoopMeasurementPort,
            &crate::controller::NoopMeasurementPort,
            Some(cfg),
            100.0,
            100.0,
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
        assert_eq!(
            ctx.feeds.heuristic_kw(crate::ids::ASSET_BASE_LOAD, now()),
            None
        );
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
            ctx.feeds.heuristic_kw(crate::ids::ASSET_BASE_LOAD, now()),
            Some(heuristic.sample_kw(now()))
        );
    }
}
