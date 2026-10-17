// Per-tick external feed resolution (weather PV, real-measurement MQTT),
// read pre-lock by `context::resolve_tick_context`.

use chrono::{DateTime, Utc};
use std::collections::HashMap;

/// Weather-sourced PV for this tick, per array (W5b): each array's value at this exact
/// instant, and the series behind it (from that array's own geometry and rating).
///
/// One `latest()` fetch for every array and one `weather_pv_forecast_series` evaluation per
/// array — that series runs solar-position and transposition physics over every forecast
/// sample plus a snow trajectory, and the tick loop runs once a second. The staleness gate
/// and the translation are `entities::solar`'s one `fresh_weather_pv_series`; the planner
/// reads each array's forecast, which this series feeds, so a plan and the headroom drawn
/// against it never disagree. The raw series is threaded onto the array each tick
/// (`TickOverrides.pv_weather_forecast`) so it can sample it at whatever future timestamps
/// it needs.
pub(crate) async fn resolve_weather_pv_for_tick(
    weather: &dyn crate::controller::WeatherForecastPort,
    pv_weather: &crate::entities::asset_params::PvWeatherParams,
    initial_snow: crate::entities::pv_snow::PvSnowState,
    now: DateTime<Utc>,
) -> (
    HashMap<String, f64>,
    HashMap<String, Vec<crate::entities::solar::WeatherPvForecastSlot>>,
) {
    let (mut kw_now, mut series_by_asset) = (HashMap::new(), HashMap::new());
    if pv_weather.is_empty() {
        return (kw_now, series_by_asset);
    }
    let forecast = weather.latest().await;
    for (asset_id, params) in pv_weather {
        let Some(series) = crate::entities::solar::fresh_weather_pv_series(
            Some(params),
            forecast.as_ref(),
            now,
            crate::services::planning::WEATHER_STALENESS_THRESHOLD,
            initial_snow,
        ) else {
            continue;
        };
        if let Some(&kw) = crate::entities::solar::weather_pv_kw_for_slots(&series, &[now]).first()
        {
            kw_now.insert(asset_id.clone(), kw);
        }
        series_by_asset.insert(asset_id.clone(), series);
    }
    (kw_now, series_by_asset)
}

/// Real-measurement MQTT feed value for this exact instant (real-measurement-mqtt), for the
/// asset the feed measures (`None`: the profile enables it for no asset — the second gate
/// alongside the port itself only existing when the corresponding env var was set).
async fn resolve_measured_kw_now(
    port: &dyn crate::controller::MeasurementPort,
    asset_id: Option<&str>,
    now: DateTime<Utc>,
) -> Option<(String, f64)> {
    let asset_id = asset_id?;
    let latest = port.latest_kw().await;
    let kw = crate::entities::measurement::resolve_measured_kw(
        latest,
        now,
        crate::entities::measurement::MEASUREMENT_STALENESS_THRESHOLD,
    )?;
    Some((asset_id.to_string(), kw))
}

/// Both feeds' measured readings for this instant, by the asset each measures —
/// `(port, measured asset)` for PV and for base load.
pub(crate) async fn resolve_measurements_now(
    pv: (&dyn crate::controller::MeasurementPort, Option<&str>),
    base_load: (&dyn crate::controller::MeasurementPort, Option<&str>),
    now: DateTime<Utc>,
) -> HashMap<String, f64> {
    let pv = resolve_measured_kw_now(pv.0, pv.1, now).await;
    let base_load = resolve_measured_kw_now(base_load.0, base_load.1, now).await;
    pv.into_iter().chain(base_load).collect()
}

/// This tick's previewed power of every physics-driven asset, twice (W5b: per asset): each
/// PV array under the generation limit in force (what the tick will produce) and under the
/// limit without the arbiter's own tightening (what a released correction would face —
/// `ArbiterTick::live_released_kw`, R-88), each base load once (the two are equal for it).
/// One preview per array, so the two can only differ in the limit.
pub(crate) fn live_previews(
    sim: &crate::simulator::SimState,
    ctx: &super::context::TickContext,
    pre_snap: &crate::controller::SimSnapshot,
    now: DateTime<Utc>,
    dt_s: f64,
) -> (
    crate::controller::arbiter::LiveKwByAsset,
    crate::controller::arbiter::LiveKwByAsset,
) {
    let released_limit = crate::controller::comms_loss::pv_generation_limit(
        pre_snap,
        ctx.plan_snap.as_ref(),
        &ctx.capacity_schedule,
        &ctx.inject,
        now,
        None,
        ctx.comms_loss,
    );
    let (inject, feeds) = (&ctx.inject, &ctx.feeds);
    let (mut live, mut released) = (HashMap::new(), HashMap::new());
    for (entry, _) in sim.iter_assets() {
        let id = entry.id.as_str();
        let (pv_override, tau_s) = (inject.pv_irradiance, inject.pv_tau_s);
        if let Some(preview) = sim.peek_pv(id, now, dt_s, pv_override, tau_s, feeds) {
            live.insert(id.to_string(), preview.in_force_kw());
            released.insert(
                id.to_string(),
                preview.under_limit_kw(released_limit.limit_kw),
            );
        } else if let Some(kw) = sim.peek_base_load_kw(
            id,
            now,
            dt_s,
            inject.base_load_kw,
            inject.base_load_alpha,
            feeds,
        ) {
            live.insert(id.to_string(), kw);
            released.insert(id.to_string(), kw);
        }
    }
    (live, released)
}
