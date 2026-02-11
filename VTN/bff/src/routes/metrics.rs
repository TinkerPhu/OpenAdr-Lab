use axum::response::IntoResponse;

use crate::AppCtx;

pub async fn get_metrics(
    axum::extract::State(ctx): axum::extract::State<AppCtx>,
) -> impl IntoResponse {
    ctx.metrics_handle.render()
}
