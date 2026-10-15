use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use serde::Deserialize;
use ts_rs::TS;

use crate::app_ctx::SimRead;
use crate::state::AppState;

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
///
/// W4: every EV's queue, each session naming its EV (`asset_id`), EVs in id order.
pub async fn get_ev_session(State(state): State<AppState>) -> impl IntoResponse {
    let sessions: Vec<_> = state
        .ev_sessions()
        .await
        .values()
        .flat_map(|q| q.iter().cloned())
        .collect();
    Json(sessions).into_response()
}

/// Whether `asset_id` is an EV: the asset that takes charge sessions, by its own declaration
/// (`RequestKind::ChargeSession`, R-128), not by its id.
async fn is_ev(sim_read: &SimRead, asset_id: &str) -> bool {
    sim_read.request_slices().await.iter().any(|s| {
        s.id == asset_id
            && s.request_kind == Some(crate::entities::asset_params::RequestKind::ChargeSession)
    })
}

/// GET /assets/:asset_id/ev-settings — that EV's overlay settings (W4); 404 for an asset that
/// is not an EV.
pub async fn get_ev_settings(
    State(sim_read): State<SimRead>,
    State(state): State<AppState>,
    Path(asset_id): Path<String>,
) -> impl IntoResponse {
    if !is_ev(&sim_read, &asset_id).await {
        return crate::routes::error::unknown_asset(&asset_id);
    }
    Json(state.ev_settings(&asset_id).await).into_response()
}

/// PUT /assets/:asset_id/ev-settings body.
#[derive(Deserialize, TS)]
#[ts(optional_fields = nullable)]
pub struct UpdateEvSettingsBody {
    pub opportunistic_charging_enabled: bool,
}

/// PUT /assets/:asset_id/ev-settings — update that EV's toggle for opportunistic PV charging
/// (W4); 404 for an asset that is not an EV.
pub async fn put_ev_settings(
    State(sim_read): State<SimRead>,
    State(state): State<AppState>,
    Path(asset_id): Path<String>,
    Json(body): Json<UpdateEvSettingsBody>,
) -> impl IntoResponse {
    if !is_ev(&sim_read, &asset_id).await {
        return crate::routes::error::unknown_asset(&asset_id);
    }
    let current = state.ev_settings(&asset_id).await;
    let updated = crate::state::EvSettings {
        opportunistic_charging_enabled: body.opportunistic_charging_enabled,
        paused_by_active_session: current.paused_by_active_session,
    };
    state.set_ev_settings(&asset_id, updated.clone()).await;
    Json(updated).into_response()
}

/// GET /ev-usage-sim — read-only diagnostics for `ev-usage-simulation` and
/// `ev-usage-forecast` (`ui-transparency`): which usage class the profile
/// declared, whether charge planning is engaged, and the next scheduled
/// leave/return, or `204 No Content` when the EV has no usage schedule
/// configured at all, matching `/ev-session`'s existing convention.
pub async fn get_ev_usage_sim(State(sim_read): State<SimRead>) -> impl IntoResponse {
    match sim_read.usage_schedule_view(chrono::Utc::now()).await {
        Some(view) => Json(view).into_response(),
        None => StatusCode::NO_CONTENT.into_response(),
    }
}
