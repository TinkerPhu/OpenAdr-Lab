//! GET/PUT `/arbiter-settings` — runtime toggles for the arbiter's two passes
//! (`controller::arbiter`): deviation correction (`deviation_arbiter_enabled`,
//! default off) and limit enforcement (`limit_enforcement_enabled`, default
//! on — switched off only to measure the planner alone). Mirrors `ev.rs`'s
//! `/ev-settings` pattern.

use axum::{extract::State, response::IntoResponse, Json};
use serde::{Deserialize, Serialize};

use crate::AppCtx;

#[derive(Debug, Clone, Serialize)]
pub struct ArbiterSettings {
    pub deviation_arbiter_enabled: bool,
    pub limit_enforcement_enabled: bool,
}

async fn arbiter_settings(ctx: &AppCtx) -> ArbiterSettings {
    ArbiterSettings {
        deviation_arbiter_enabled: ctx.state.deviation_arbiter_enabled().await,
        limit_enforcement_enabled: ctx.state.limit_enforcement_enabled().await,
    }
}

/// GET /arbiter-settings — both passes' current toggle state.
pub async fn get_arbiter_settings(State(ctx): State<AppCtx>) -> impl IntoResponse {
    Json(arbiter_settings(&ctx).await)
}

/// PUT /arbiter-settings body — each toggle is optional, so one switch can
/// change without restating the other.
#[derive(Deserialize)]
pub struct UpdateArbiterSettingsBody {
    pub deviation_arbiter_enabled: Option<bool>,
    pub limit_enforcement_enabled: Option<bool>,
}

/// PUT /arbiter-settings — update either or both toggles; returns the result.
pub async fn put_arbiter_settings(
    State(ctx): State<AppCtx>,
    Json(body): Json<UpdateArbiterSettingsBody>,
) -> impl IntoResponse {
    if let Some(enabled) = body.deviation_arbiter_enabled {
        ctx.state.set_deviation_arbiter_enabled(enabled).await;
    }
    if let Some(enabled) = body.limit_enforcement_enabled {
        ctx.state.set_limit_enforcement_enabled(enabled).await;
    }
    Json(arbiter_settings(&ctx).await)
}

/// GET /arbiter-diagnostics — last tick's arbiter reasoning (projected net
/// site power, residual deviation from the plan target, and which lever, if
/// any, fired; the limit pass's target, excess, adjustments and unresolved
/// excess; the measured net power the tick came to) so the reactive levers are visible outside the server process
/// (ui-transparency — no backend-only decision without an inspectable
/// surface). `net_kw`/`dev_kw`/`active_lever` are `null` before the arbiter
/// has run this process, or during the no-plan-yet startup window.
pub async fn get_arbiter_diagnostics(State(ctx): State<AppCtx>) -> impl IntoResponse {
    Json(ctx.state.arbiter_diagnostics().await)
}
