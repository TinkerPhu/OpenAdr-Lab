mod config;
mod models;
mod state;
mod vtn;

use axum::{
    extract::{Path, Query, State},
    http::Method,
    response::IntoResponse,
    routing::{get, post, put},
    Json, Router,
};
use chrono::Utc;
use config::Config;
use models::{SensorInput, SensorSnapshot};
use serde::Deserialize;
use state::AppState;
use tower_http::cors::{Any, CorsLayer};
use tracing::{error, info};
use uuid::Uuid;
use vtn::VtnClient;

#[derive(Clone)]
struct AppCtx {
    state: AppState,
    vtn: VtnClient,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(std::env::var("RUST_LOG").unwrap_or_else(|_| "info".into()))
        .init();

    let cfg = Config::from_env()?;
    info!("starting ven {} listening on {}", cfg.ven_name, cfg.listen_addr);

    let state = AppState::new();

    // Optional: load persisted state
    if let Some(path) = cfg.persist_path.clone() {
        if let Ok(s) = tokio::fs::read_to_string(&path).await {
            if let Err(e) = state.load_from_json(&s).await {
                error!("failed to load persisted state: {e:#}");
            } else {
                info!("loaded persisted state from {path}");
            }
        }
    }

    let vtn = VtnClient::new(cfg.vtn_base_url.clone(), cfg.client_id.clone(), cfg.client_secret.clone(), cfg.ven_name.clone());

    // Poll programs
    {
        let state = state.clone();
        let vtn = vtn.clone();
        let secs = cfg.poll_programs_secs;
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(secs));
            loop {
                interval.tick().await;
                match vtn.fetch_programs().await {
                    Ok(programs) => state.set_programs(programs).await,
                    Err(e) => error!("program poll failed: {e:#}"),
                }
            }
        });
    }

    // Poll events
    {
        let state = state.clone();
        let vtn = vtn.clone();
        let secs = cfg.poll_events_secs;
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(secs));
            loop {
                interval.tick().await;
                match vtn.fetch_events().await {
                    Ok(events) => state.set_events(events, 500).await,
                    Err(e) => error!("event poll failed: {e:#}"),
                }
            }
        });
    }

    // Poll reports
    {
        let state = state.clone();
        let vtn = vtn.clone();
        let secs = cfg.poll_reports_secs;
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(secs));
            loop {
                interval.tick().await;
                match vtn.fetch_reports().await {
                    Ok(reports) => state.set_reports(reports).await,
                    Err(e) => error!("report poll failed: {e:#}"),
                }
            }
        });
    }

    // Fake sensor sampler (replace with MQTT/Modbus/etc.)
    {
        let state = state.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(10));
            loop {
                interval.tick().await;

                // Example: fake power value
                let mut snap = state.sensor().await;
                snap.ts = Utc::now();
                snap.power_w = Some((snap.ts.timestamp() % 100) as f64); // placeholder
                snap.raw = serde_json::json!({"source": "fake"});
                state.update_sensor(snap).await;
            }
        });
    }

    // Optional persistence task
    if let Some(path) = cfg.persist_path.clone() {
        let state = state.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(15));
            loop {
                interval.tick().await;
                match state.to_json().await {
                    Ok(json) => {
                        if let Err(e) = tokio::fs::write(&path, json).await {
                            error!("persist write failed: {e:#}");
                        }
                    }
                    Err(e) => error!("persist serialization failed: {e:#}"),
                }
            }
        });
    }

    // HTTP API
    let ctx = AppCtx { state, vtn };

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods([Method::GET, Method::POST, Method::PUT])
        .allow_headers(Any);

    let app = Router::new()
        .route("/health", get(health))
        .route("/events", get(get_events))
        .route("/programs", get(get_programs))
        .route("/sensors", get(get_sensors).post(post_sensors))
        .route("/reports", get(get_reports).post(post_reports))
        .route("/reports/:id", put(put_report))
        .with_state(ctx)
        .layer(cors);

    let listener = tokio::net::TcpListener::bind(cfg.listen_addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

async fn health() -> &'static str {
    "ok"
}

#[derive(Deserialize)]
struct EventsQuery {
    limit: Option<usize>,
}

async fn get_events(
    State(ctx): State<AppCtx>,
    Query(q): Query<EventsQuery>,
) -> impl IntoResponse {
    let mut events = ctx.state.events().await;
    let limit = q.limit.unwrap_or(100);
    if events.len() > limit {
        events.truncate(limit);
    }
    Json(events)
}

async fn get_programs(State(ctx): State<AppCtx>) -> impl IntoResponse {
    Json(ctx.state.programs().await)
}

async fn get_sensors(State(ctx): State<AppCtx>) -> impl IntoResponse {
    Json(ctx.state.sensor().await)
}

async fn post_sensors(
    State(ctx): State<AppCtx>,
    Json(input): Json<SensorInput>,
) -> impl IntoResponse {
    let snap = SensorSnapshot {
        id: Uuid::new_v4(),
        ts: Utc::now(),
        temperature_c: input.temperature_c,
        power_w: input.power_w,
        voltage_v: input.voltage_v,
        raw: input.raw.unwrap_or(serde_json::json!({})),
    };
    ctx.state.update_sensor(snap.clone()).await;
    Json(snap)
}

async fn get_reports(State(ctx): State<AppCtx>) -> impl IntoResponse {
    Json(ctx.state.reports().await)
}

async fn post_reports(
    State(ctx): State<AppCtx>,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    match ctx.vtn.upsert_report(body).await {
        Ok(result) => (axum::http::StatusCode::CREATED, Json(result)).into_response(),
        Err(e) => {
            error!("report submission failed: {e:#}");
            (
                axum::http::StatusCode::BAD_GATEWAY,
                Json(serde_json::json!({"error": format!("{e:#}")})),
            )
                .into_response()
        }
    }
}

async fn put_report(
    State(ctx): State<AppCtx>,
    Path(id): Path<String>,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    match ctx.vtn.update_report(&id, body).await {
        Ok(result) => (axum::http::StatusCode::OK, Json(result)).into_response(),
        Err(e) => {
            error!("report update failed: {e:#}");
            (
                axum::http::StatusCode::BAD_GATEWAY,
                Json(serde_json::json!({"error": format!("{e:#}")})),
            )
                .into_response()
        }
    }
}
