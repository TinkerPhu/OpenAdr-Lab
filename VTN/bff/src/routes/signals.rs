//! The signal workbench's two routes. Both are thin: the facts and the timing come from
//! `signal_workbench`, which gets them from `lab-core`.

use axum::{extract::State, http::StatusCode, Json};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::json;

use crate::signal_workbench::{self, WireSignalFacts};
use crate::AppCtx;

type ApiError = (StatusCode, Json<serde_json::Value>);

fn bad_request(message: impl Into<String>) -> ApiError {
    (
        StatusCode::BAD_REQUEST,
        Json(json!({"error": message.into()})),
    )
}

/// `GET /api/signals/registry` — what a number of each payload type means on the wire.
pub async fn registry() -> Json<Vec<WireSignalFacts>> {
    Json(signal_workbench::registry())
}

#[derive(Debug, Deserialize)]
pub struct PreviewRequest {
    /// The event as it would be posted to `/api/events`, or as `/api/events` lists it.
    pub event: serde_json::Value,
    /// With `pattern`, the declared intervals timed but not repeated; otherwise the
    /// expansion over `[from, to)`, which are then required.
    #[serde(default)]
    pub pattern: bool,
    pub from: Option<DateTime<Utc>>,
    pub to: Option<DateTime<Utc>>,
}

/// `POST /api/signals/preview` — a draft or stored event resolved by the rule receivers use.
pub async fn preview(
    State(ctx): State<AppCtx>,
    Json(req): Json<PreviewRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let event = signal_workbench::draft_event(&req.event).map_err(bad_request)?;
    let reader = crate::fleet_signals::payload_reader(&ctx.business).await;

    if req.pattern {
        let pattern = signal_workbench::pattern(&event, &reader).map_err(bad_request)?;
        return Ok(Json(json!({ "pattern": pattern })));
    }

    let (Some(from), Some(to)) = (req.from, req.to) else {
        return Err(bad_request(
            "`from` and `to` are required unless `pattern` is true",
        ));
    };
    if to <= from {
        return Err(bad_request("`to` must be after `from`"));
    }
    Ok(Json(json!(signal_workbench::preview(
        &event, &reader, from, to
    ))))
}
