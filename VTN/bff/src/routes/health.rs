use axum::{extract::State, Json};
use chrono::Utc;
use serde_json::json;

use crate::AppCtx;

pub async fn health(State(ctx): State<AppCtx>) -> Json<serde_json::Value> {
    let (reachable, auth_ok) = ctx.business.check_health().await;
    let recorder = ctx.recorder_status.read().await.clone();

    Json(json!({
        "time": Utc::now().to_rfc3339(),
        "bff": {
            "ok": true,
            "version": env!("CARGO_PKG_VERSION"),
        },
        "vtn": {
            "reachable": reachable,
            "authOk": auth_ok,
        },
        "recorder": {
            "enabled": ctx.config.database_url.is_some(),
            "connected": recorder.connected,
            "lastPollAt": recorder.last_poll_at,
            "lastSuccessAt": recorder.last_success_at,
            "consecutiveFailures": recorder.consecutive_failures,
            "lastError": recorder.last_error,
        }
    }))
}
