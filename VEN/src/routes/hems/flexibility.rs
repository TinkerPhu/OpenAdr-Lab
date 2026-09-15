//! `GET /flexibility*` — the Site Headroom and capacity-curve reads (split out
//! of `sessions.rs`, file-size cap).

use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use tracing::debug;

use crate::controller::capacity_headroom::compute_site_capacity_curves_at;
use crate::entities::capacity_curve::CapacityCurves;
use crate::AppCtx;

/// GET /flexibility — returns the live site-level flexibility envelope (Phase E).
///
/// Updated every dispatcher tick (~1s) and after every planner cycle.
/// Returns 204 No Content until the first dispatcher tick completes.
pub async fn get_flexibility(State(ctx): State<AppCtx>) -> impl IntoResponse {
    match ctx.state.site_envelope().await {
        Some(env) => Json(env).into_response(),
        None => StatusCode::NO_CONTENT.into_response(),
    }
}

/// GET /flexibility/history — BL-43: the site-headroom ring
/// (`AppState::flexibility_history`), oldest first, for the "Site Headroom"
/// diagram. Distinct from `GET /flexibility`, which is a single live snapshot.
/// Always 200 — an empty array before the first dispatcher tick.
pub async fn get_flexibility_history(State(ctx): State<AppCtx>) -> impl IntoResponse {
    Json(ctx.state.flexibility_history().await)
}

/// GET /flexibility/forecast — forward-looking per-slot headroom trajectory,
/// re-derived fresh every dispatcher tick from the active plan's own setpoint
/// schedule plus each asset's real current state (see
/// `SiteFlexibilityForecastSlot`'s doc comment — its `up_kw`/`down_kw` are
/// ABSOLUTE achievable power now, not a delta from planned dispatch,
/// `unified-capacity-envelope-engine` Spec E) — distinct from both
/// `GET /flexibility` (instant-only) and `GET /flexibility/history` (the past
/// ring). Always 200 — an empty array when there's no active plan.
pub async fn get_flexibility_forecast(State(ctx): State<AppCtx>) -> impl IntoResponse {
    Json(ctx.state.site_headroom_forecast().await)
}

/// Query for `GET /flexibility/capacity`. `start` (RFC 3339) anchors the
/// curves at a future plan slot instead of now; malformed → 400 (axum's own
/// `Query` rejection).
#[derive(Debug, Deserialize)]
pub struct CapacityCurvesQuery {
    pub start: Option<DateTime<Utc>>,
}

/// GET /flexibility/capacity — sustained-commitment power/duration/energy
/// capacity curves (both directions in one response). Without `start`, the
/// curves re-derived every dispatcher tick from the current asset state (see
/// `controller::capacity_headroom`'s module doc for why this is a distinct
/// computation from `GET /flexibility/forecast` above, not an extension of
/// it — they're fixed-axis slices of the same underlying
/// `(t1, t2, direction, tier)` domain). With a future `start` and an active
/// plan, computed on demand from the plan slot `start` snaps down to
/// (`compute_site_capacity_curves_at`); anywhere that resolves to now, the
/// per-tick curves. The response's `start` is the instant actually used.
/// 204 before the first dispatcher tick.
pub async fn get_capacity_curves(
    State(ctx): State<AppCtx>,
    Query(query): Query<CapacityCurvesQuery>,
) -> impl IntoResponse {
    let Some((import, export)) = ctx.state.capacity_curves().await else {
        return StatusCode::NO_CONTENT.into_response();
    };
    let per_tick = CapacityCurves {
        start: import.start,
        import,
        export,
    };
    let at_start = match (query.start, ctx.state.active_plan().await) {
        (Some(start), Some(plan)) => {
            let started = std::time::Instant::now();
            // Computed synchronously under the lock, dropped before any
            // `.await` -- same pattern as `services::forecast::finish_plan_cycle`.
            let curves = {
                let sim = ctx.sim.lock().await;
                compute_site_capacity_curves_at(
                    &sim,
                    &plan,
                    start,
                    Utc::now(),
                    ctx.grid_max_import_kw,
                    ctx.grid_max_export_kw,
                )
            };
            debug!(
                elapsed_us = started.elapsed().as_micros() as u64,
                "capacity curves at future start"
            );
            curves
        }
        _ => None,
    };
    Json(at_start.unwrap_or(per_tick)).into_response()
}
