use axum::{extract::State, Json};
use chrono::Utc;
use serde_json::json;

use crate::AppCtx;

pub async fn health(State(ctx): State<AppCtx>) -> Json<serde_json::Value> {
    let (reachable, auth_ok) = ctx.business.check_health().await;

    Json(json!({
        "time": Utc::now().to_rfc3339(),
        "bff": {
            "ok": true,
            "version": env!("CARGO_PKG_VERSION"),
        },
        "vtn": {
            "reachable": reachable,
            "authOk": auth_ok,
        }
    }))
}
