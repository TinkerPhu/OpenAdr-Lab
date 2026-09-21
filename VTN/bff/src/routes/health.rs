use axum::{extract::State, Json};
use chrono::Utc;
use serde_json::json;

use crate::AppCtx;

pub async fn health(State(ctx): State<AppCtx>) -> Json<serde_json::Value> {
    let (reachable, auth_ok) = ctx.business.check_health().await;
    let recorder = ctx.recorder_status.read().await.clone();
    let fleet = ctx.fleet_status.read().await.clone();
    // How many VENs have told us anything, and how many say they are up. Both
    // matter: a VEN whose last-will fired is still *known*, and a fleet view
    // that counted only "known" would show it as fine.
    let (known, online) = {
        let state = ctx.fleet.read().await;
        let online = state
            .values()
            .filter(|v| v.state.as_deref() != Some("offline"))
            .count();
        (state.len(), online)
    };

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
        },
        "fleet": {
            "enabled": fleet.enabled,
            "connected": fleet.connected,
            "lastMessageAt": fleet.last_message_at,
            "lastError": fleet.last_error,
            "vensKnown": known,
            "vensOnline": online,
            // The history behind the feed. `storeConnected` is false until the
            // first successful connection, and `samplesDropped` counts what the
            // writer's queue could not take -- a number that should stay at
            // zero and means something specific if it does not.
            "storeEnabled": ctx.fleet_writer.is_some(),
            "storeConnected": ctx.fleet_store.read().await.is_some(),
            "samplesDropped": ctx.fleet_writer.as_ref().map(|w| w.dropped()).unwrap_or(0),
        }
    }))
}
