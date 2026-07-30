//! GET/PUT `/arbiter-settings` — runtime toggle for the deviation arbiter
//! (`controller::arbiter`, `openspec/changes/deviation-arbiter/`). Mirrors
//! `ev.rs`'s `/ev-settings` pattern. Default `false`; flipping it on lets a
//! user (or a test's own setup step) observe the arbiter's reactive levers
//! without changing any profile's default.

use axum::{extract::State, response::IntoResponse, Json};
use serde::{Deserialize, Serialize};

use crate::AppCtx;

#[derive(Debug, Clone, Serialize)]
pub struct ArbiterSettings {
    pub deviation_arbiter_enabled: bool,
}

/// GET /arbiter-settings — returns the current rollout-gate state.
pub async fn get_arbiter_settings(State(ctx): State<AppCtx>) -> impl IntoResponse {
    Json(ArbiterSettings {
        deviation_arbiter_enabled: ctx.state.deviation_arbiter_enabled().await,
    })
}

/// PUT /arbiter-settings body.
#[derive(Deserialize)]
pub struct UpdateArbiterSettingsBody {
    pub deviation_arbiter_enabled: bool,
}

/// PUT /arbiter-settings — update the deviation-arbiter rollout gate.
pub async fn put_arbiter_settings(
    State(ctx): State<AppCtx>,
    Json(body): Json<UpdateArbiterSettingsBody>,
) -> impl IntoResponse {
    ctx.state
        .set_deviation_arbiter_enabled(body.deviation_arbiter_enabled)
        .await;
    Json(ArbiterSettings {
        deviation_arbiter_enabled: body.deviation_arbiter_enabled,
    })
}

/// GET /arbiter-diagnostics — last tick's arbiter reasoning (projected net
/// site power, residual deviation from the plan target, and which lever, if
/// any, fired) so the reactive levers are visible outside the server process
/// (ui-transparency — no backend-only decision without an inspectable
/// surface). `net_kw`/`dev_kw`/`active_lever` are `null` before the arbiter
/// has run this process, or during the no-plan-yet startup window.
pub async fn get_arbiter_diagnostics(State(ctx): State<AppCtx>) -> impl IntoResponse {
    Json(ctx.state.arbiter_diagnostics().await)
}
