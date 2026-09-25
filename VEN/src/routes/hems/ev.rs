use axum::{extract::State, http::StatusCode, response::IntoResponse, Json};
use serde::{Deserialize, Serialize};

use crate::AppCtx;

/// GET /ev-session — returns the currently active EvSession (204 if none), read-only.
///
/// Kept after BL-41 (which removed the write-side `/ev-session` CRUD, superseded by
/// `/user-requests`) because a VTN-issued CHARGE_STATE_SETPOINT event creates an
/// EvSession directly (`tasks/poll_signals.rs`) with no linked UserRequest — so it is
/// invisible to `GET /user-requests`. This is the only observable surface for that case.
pub async fn get_ev_session(State(ctx): State<AppCtx>) -> impl IntoResponse {
    match ctx.state.ev_session().await {
        Some(s) => Json(s).into_response(),
        None => StatusCode::NO_CONTENT.into_response(),
    }
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

/// `GET /ev-usage-sim` response body.
#[derive(Serialize)]
pub struct EvUsageSimState {
    pub plan_ahead: bool,
    pub next_trip: Option<NextTrip>,
}

/// GET /ev-usage-sim — read-only diagnostics for `ev-usage-simulation`
/// (`ui-transparency`): whether plan-ahead is active and the next scheduled
/// leave/return, or `204 No Content` when the EV has no usage-sim configured
/// at all, matching `/ev-session`'s existing convention.
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
                (0..7).find_map(|offset| {
                    crate::assets::ev_schedule::daily_trip(
                        usage_sim,
                        now.date_naive() + chrono::Duration::days(offset),
                        ev.usage_sim_seed_tag,
                    )
                    .filter(|trip| trip.leave_at > now)
                })
            },
        );

    Json(EvUsageSimState {
        plan_ahead: usage_sim.plan_ahead,
        next_trip: next_trip.map(|trip| NextTrip {
            leave_at: trip.leave_at,
            return_at: trip.return_at,
            expected_soc_drop_pct: trip.soc_drop_pct,
        }),
    })
    .into_response()
}
