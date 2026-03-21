use axum::{extract::State, response::IntoResponse};

use crate::AppCtx;

pub async fn health() -> &'static str {
    "ok"
}

pub async fn get_metrics(State(ctx): State<AppCtx>) -> impl IntoResponse {
    ctx.metrics_handle.render()
}
