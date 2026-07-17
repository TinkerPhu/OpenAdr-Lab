pub mod assets;
pub mod debug;
pub mod events;
pub mod hems;
pub mod notifications;
pub mod reports;
pub mod sim;
pub mod system;
pub mod timeline;
pub mod trace;

use axum::{
    http::Method,
    routing::{delete, get, post, put},
    Router,
};
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;

use crate::AppCtx;

pub fn build_router(ctx: AppCtx) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods([Method::GET, Method::POST, Method::PUT, Method::DELETE])
        .allow_headers(Any);

    Router::new()
        .route("/health", get(system::health))
        .route("/events", get(events::get_events))
        .route("/programs", get(events::get_programs))
        .route(
            "/sensors",
            get(events::get_sensors).post(events::post_sensors),
        )
        .route(
            "/reports",
            get(reports::get_reports).post(reports::post_reports),
        )
        .route("/reports/:id", put(reports::put_report))
        .route("/sim", get(sim::get_sim))
        .route("/sim/schema", get(sim::get_sim_schema))
        .route("/sim/reset/:asset_id", post(sim::post_sim_reset))
        .route("/sim/config/battery", put(sim::put_sim_config_battery))
        .route(
            "/sim/inject",
            get(sim::get_sim_inject).post(sim::post_sim_inject),
        )
        .route("/sim/inject/reset", post(sim::post_sim_inject_reset))
        .route("/plan/trigger", post(sim::post_plan_trigger))
        .route(
            "/debug/heuristics/preload",
            post(debug::post_heuristics_preload),
        )
        .route("/trace/events", get(trace::get_trace_events))
        .route("/trace/history", get(trace::get_trace_history))
        .route("/metrics", get(system::get_metrics))
        // HEMS Stage 1–3 routes
        .route("/plan", get(hems::get_plan))
        .route("/plan/objective", put(hems::put_plan_objective))
        .route("/plan/events", get(hems::get_plan_events))
        .route("/notifications", get(notifications::get_notifications))
        .route(
            "/notifications/history",
            get(notifications::get_notifications_history),
        )
        .route(
            "/notifications/events",
            get(notifications::get_notification_events),
        )
        .route("/tariffs", get(hems::get_tariffs))
        // Timeline routes (speckit 005) — /all must precede /:asset_id
        .route("/timeline/all", get(timeline::get_timeline_all))
        .route("/timeline/:asset_id", get(timeline::get_timeline))
        // Asset forecast + history + capability endpoints (speckit 007 / Phase A)
        .route("/forecast/:asset_id", get(assets::get_asset_forecast))
        // Persistent history routes (Phase 1, WP1.4) — literal segments must
        // precede /:asset_id so they aren't shadowed by the live-history route.
        .route("/history/ticks", get(hems::get_history_ticks))
        .route("/history/grid", get(hems::get_history_grid))
        .route("/history/events", get(hems::get_history_events))
        .route("/history/reports", get(hems::get_history_reports))
        .route("/history/plans", get(hems::get_history_plans))
        .route("/history/:asset_id", get(assets::get_asset_history))
        .route("/capability/:asset_id", get(assets::get_asset_capability))
        .route(
            "/assets/:asset_id/comfort_curve",
            get(hems::get_comfort_curve)
                .post(hems::post_comfort_curve)
                .delete(hems::delete_comfort_curve),
        )
        // HEMS Stage 2 routes
        .route("/capacity", get(hems::get_capacity))
        .route("/signals", get(hems::get_signals))
        .route("/obligations", get(hems::get_obligations))
        // HEMS Stage 4 routes
        .route("/ledger", get(hems::get_ledger))
        // WP3.6 (BL-15): per-asset forecasts from the latest plan cycle
        .route("/forecast", get(hems::get_forecast))
        // HEMS Stage 5 routes
        .route(
            "/user-requests",
            get(hems::get_requests).post(hems::post_requests),
        )
        .route("/user-requests/:id", delete(hems::delete_request))
        .route("/flexibility", get(hems::get_flexibility))
        .route(
            "/ev-session",
            get(hems::get_ev_session)
                .post(hems::post_ev_session)
                .delete(hems::delete_ev_session),
        )
        .route(
            "/ev-settings",
            get(hems::get_ev_settings).put(hems::put_ev_settings),
        )
        .route(
            "/heater-target",
            get(hems::get_heater_target)
                .post(hems::post_heater_target)
                .delete(hems::delete_heater_target),
        )
        .route(
            "/shiftable-loads",
            get(hems::get_shiftable_loads).post(hems::post_shiftable_load),
        )
        .route("/shiftable-loads/:id", delete(hems::delete_shiftable_load))
        .route(
            "/baseline-override",
            get(hems::get_baseline_override)
                .post(hems::post_baseline_override)
                .delete(hems::delete_baseline_override),
        )
        .with_state(ctx)
        .layer(TraceLayer::new_for_http())
        .layer(cors)
}
