use axum::{extract::State, http::StatusCode, response::IntoResponse, Json};
use serde::{Deserialize, Serialize};

use crate::AppCtx;

/// GET /ev-session — every queued EV charging session, in window order, read-only.
///
/// Returns the whole queue rather than one session: an EV may now hold several, and
/// a surface that showed only the current one would make the rest invisible
/// (`ui-transparency`). An empty queue is an empty list, not 204 — "no sessions" is
/// an answer, and a caller rendering a list should not have to special-case it.
///
/// Kept after BL-41 (which removed the write-side `/ev-session` CRUD, superseded by
/// `/user-requests`) because a session need not have a linked `UserRequest`: the
/// simulated usage schedule creates its own, so they are invisible to
/// `GET /user-requests`. This is the only observable surface for those.
pub async fn get_ev_session(State(ctx): State<AppCtx>) -> impl IntoResponse {
    let sessions: Vec<_> = ctx.state.ev_sessions().await.iter().cloned().collect();
    Json(sessions).into_response()
}

/// GET /ev-settings — returns the current EV overlay settings.
pub async fn get_ev_settings(State(ctx): State<AppCtx>) -> impl IntoResponse {
    Json(ctx.state.ev_settings().await)
}

/// PUT /ev-settings body.
#[derive(Deserialize)]
pub struct UpdateEvSettingsBody {
    pub opportunistic_charging_enabled: bool,
}

/// PUT /ev-settings — update the user toggle for opportunistic PV charging.
pub async fn put_ev_settings(
    State(ctx): State<AppCtx>,
    Json(body): Json<UpdateEvSettingsBody>,
) -> impl IntoResponse {
    let current = ctx.state.ev_settings().await;
    let updated = crate::state::EvSettings {
        opportunistic_charging_enabled: body.opportunistic_charging_enabled,
        paused_by_active_session: current.paused_by_active_session,
    };
    ctx.state.set_ev_settings(updated.clone()).await;
    Json(updated)
}

/// One upcoming simulated trip, for `GET /ev-usage-sim`.
#[derive(Serialize)]
pub struct NextTrip {
    pub leave_at: chrono::DateTime<chrono::Utc>,
    pub return_at: chrono::DateTime<chrono::Utc>,
    pub expected_soc_drop_pct: f64,
}

/// `GET /ev-usage-sim` response body. Serves both usage classes — `usage_sim`
/// and `usage_forecast` share one schedule shape, so they share one diagnostics
/// response, distinguished by `mode` rather than by a sibling route.
#[derive(Serialize)]
pub struct EvUsageSimState {
    pub mode: crate::entities::asset_params::EvUsageMode,
    pub engage_charge_planning: bool,
    pub next_trip: Option<NextTrip>,
}

/// GET /ev-usage-sim — read-only diagnostics for `ev-usage-simulation` and
/// `ev-usage-forecast` (`ui-transparency`): which usage class the profile
/// declared, whether charge planning is engaged, and the next scheduled
/// leave/return, or `204 No Content` when the EV has no usage schedule
/// configured at all, matching `/ev-session`'s existing convention.
pub async fn get_ev_usage_sim(State(ctx): State<AppCtx>) -> impl IntoResponse {
    let sim = ctx.sim.lock().await;
    let Some((_, cfg)) = sim.find_asset(crate::ids::ASSET_EV) else {
        return StatusCode::NO_CONTENT.into_response();
    };
    let Some(ev) = cfg.as_any().downcast_ref::<crate::assets::ev::EvCharger>() else {
        return StatusCode::NO_CONTENT.into_response();
    };
    let Some(usage_sim) = &ev.usage_sim else {
        return StatusCode::NO_CONTENT.into_response();
    };

    let now = chrono::Utc::now();
    // Look up to a week ahead for display purposes — independent of the
    // planner's own (shorter) horizon, which only gates plan-ahead's write.
    let next_trip =
        crate::assets::ev_schedule::active_trip_at(usage_sim, ev.usage_sim_seed_tag, now).or_else(
            || {
                crate::assets::ev_schedule::next_trip_after(
                    usage_sim,
                    ev.usage_sim_seed_tag,
                    now,
                    now + chrono::Duration::days(7),
                )
            },
        );

    Json(EvUsageSimState {
        mode: usage_sim.mode,
        engage_charge_planning: usage_sim.engage_charge_planning,
        next_trip: next_trip.map(|trip| NextTrip {
            leave_at: trip.leave_at,
            return_at: trip.return_at,
            expected_soc_drop_pct: trip.soc_drop_pct,
        }),
    })
    .into_response()
}
