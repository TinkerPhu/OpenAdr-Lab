use axum::{extract::State, http::StatusCode, response::IntoResponse, Json};
use serde::Deserialize;

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

/// GET /ev-usage-sim — read-only diagnostics for `ev-usage-simulation` and
/// `ev-usage-forecast` (`ui-transparency`): which usage class the profile
/// declared, whether charge planning is engaged, and the next scheduled
/// leave/return, or `204 No Content` when the EV has no usage schedule
/// configured at all, matching `/ev-session`'s existing convention.
pub async fn get_ev_usage_sim(State(ctx): State<AppCtx>) -> impl IntoResponse {
    match ctx.sim_read.usage_schedule_view(chrono::Utc::now()).await {
        Some(view) => Json(view).into_response(),
        None => StatusCode::NO_CONTENT.into_response(),
    }
}
