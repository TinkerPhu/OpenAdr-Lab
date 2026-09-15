// Per-tick external feed resolution (weather PV, real-measurement MQTT),
// read pre-lock by `context::resolve_tick_context`.

use chrono::{DateTime, Utc};

/// Weather-sourced PV for this tick: the value at this exact instant, plus one
/// value per remaining plan slot for the site-headroom / capacity forecast.
///
/// Both come from a SINGLE `latest()` fetch and a SINGLE
/// `weather_pv_forecast_series` evaluation — that series runs solar-position
/// and transposition physics over every forecast sample plus a snow
/// trajectory, and the tick loop runs once a second, so resolving the instant
/// and the slot grid separately would double that work on every tick of every
/// VEN. Same staleness gating and same translation the planner's own PV input
/// uses (R-50), reached through the one shared entry point rather than
/// re-derived, so a plan and the headroom drawn against it never disagree.
pub(crate) async fn resolve_weather_pv_kw_for_tick(
    weather: &dyn crate::controller::WeatherForecastPort,
    weather_pv_params: Option<&crate::entities::asset_params::PvForecastParams>,
    now: DateTime<Utc>,
) -> (
    Option<f64>,
    Option<Vec<crate::entities::solar::WeatherPvForecastSlot>>,
) {
    let Some(params) = weather_pv_params else {
        return (None, None);
    };
    let Some(forecast) = weather.latest().await else {
        return (None, None);
    };
    if !forecast.is_fresh(now, crate::services::planning::WEATHER_STALENESS_THRESHOLD) {
        return (None, None);
    }
    let series = crate::entities::solar::weather_pv_forecast_series(params, &forecast);
    let now_kw = crate::entities::solar::weather_pv_kw_for_slots(&series, &[now])
        .first()
        .copied();
    // pv-competence-consolidation section 5: the per-slot series (formerly a
    // separate `slots_kw` return value, pre-sampled onto plan-slot
    // boundaries by this function) is superseded entirely by the raw series
    // below, threaded onto PvInverter each tick (TickOverrides.pv_weather_forecast)
    // so PvInverter::max_effort_schedule/forecast()/simulate_forward can
    // sample it themselves at whatever future timestamps they need, rather
    // than a site-level caller pre-sampling it onto boundaries the asset
    // doesn't own.
    (now_kw, Some(series))
}

/// Real-measurement MQTT feed value for this exact instant (real-measurement-mqtt).
/// `enabled` is the profile-level gate (`measurements.pv_enabled` /
/// `.base_load_enabled`) — the second gate alongside the port itself only
/// existing when the corresponding env var was set at startup.
async fn resolve_measured_kw_now(
    port: &dyn crate::controller::MeasurementPort,
    enabled: bool,
    now: DateTime<Utc>,
) -> Option<f64> {
    if !enabled {
        return None;
    }
    let latest = port.latest_kw().await;
    crate::entities::measurement::resolve_measured_kw(
        latest,
        now,
        crate::entities::measurement::MEASUREMENT_STALENESS_THRESHOLD,
    )
}

/// Both signals' measured readings for this instant, `(pv, base_load)` —
/// bundles the two `resolve_measured_kw_now` calls into one await site to
/// keep `tick_once` under the tasks/ file-size cap.
pub(crate) async fn resolve_measurements_now(
    pv_port: &dyn crate::controller::MeasurementPort,
    pv_enabled: bool,
    base_load_port: &dyn crate::controller::MeasurementPort,
    base_load_enabled: bool,
    now: DateTime<Utc>,
) -> (Option<f64>, Option<f64>) {
    let pv = resolve_measured_kw_now(pv_port, pv_enabled, now).await;
    let base_load = resolve_measured_kw_now(base_load_port, base_load_enabled, now).await;
    (pv, base_load)
}
