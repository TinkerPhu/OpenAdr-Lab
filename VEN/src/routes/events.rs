use axum::{
    extract::{Query, State},
    response::IntoResponse,
    Json,
};
use chrono::Utc;
use serde::Deserialize;
use uuid::Uuid;

use crate::simulator::{SensorInput, SensorSnapshot};
use crate::state::AppState;

#[derive(Deserialize)]
pub struct EventsQuery {
    pub limit: Option<usize>,
}

pub async fn get_events(
    State(state): State<AppState>,
    Query(q): Query<EventsQuery>,
) -> impl IntoResponse {
    let mut events = state.events().await;
    let limit = q.limit.unwrap_or(100);
    if events.len() > limit {
        events.truncate(limit);
    }
    Json(events)
}

pub async fn get_programs(State(state): State<AppState>) -> impl IntoResponse {
    Json(state.programs().await)
}

pub async fn get_sensors(State(state): State<AppState>) -> impl IntoResponse {
    Json(state.sensor().await)
}

pub async fn post_sensors(
    State(state): State<AppState>,
    Json(input): Json<SensorInput>,
) -> impl IntoResponse {
    let snap = SensorSnapshot {
        id: Uuid::new_v4(),
        ts: Utc::now(),
        temperature_c: input.temperature_c,
        power_w: input.power_w,
        voltage_v: input.voltage_v,
        raw: input.raw.unwrap_or_default(),
    };
    state.update_sensor(snap.clone()).await;
    Json(snap)
}
