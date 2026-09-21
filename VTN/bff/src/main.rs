mod cache;
mod config;
mod db;
mod error;
mod fleet;
mod fleet_store;
mod recorder;
mod routes;
mod vtn_client;

use axum::{
    extract::{Request, State},
    http::{HeaderName, Method},
    middleware::{self, Next},
    response::Response,
    routing::{delete, get, put},
    Router,
};
use metrics::{counter, histogram};
use metrics_exporter_prometheus::PrometheusBuilder;
use std::sync::Arc;
use std::time::Instant;
use tower_http::cors::{Any, CorsLayer};
use tower_http::request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer};
use tower_http::trace::TraceLayer;
use tracing::info;

use cache::TtlCache;
use config::Config;
use vtn_client::VtnClient;

#[derive(Clone)]
pub struct AppCtx {
    /// The one VTN credential. 3.1's scope model lets a single business-layer
    /// client hold every scope the BFF needs, so the 3.0 dual-credential split
    /// (any-business + ven-manager) is gone.
    pub business: VtnClient,
    pub cache: Arc<TtlCache>,
    pub config: Arc<Config>,
    pub metrics_handle: Arc<metrics_exporter_prometheus::PrometheusHandle>,
    pub recorder_status: recorder::SharedRecorderStatus,
    /// The fleet's live state, as the VENs last published it.
    pub fleet: fleet::FleetState,
    pub fleet_status: Arc<tokio::sync::RwLock<fleet::FleetIngestStatus>>,
    /// The fleet's stored history, once its connection is up.
    pub fleet_store: fleet_store::SharedPool,
    /// The queue in front of it, for the count of samples it had to drop.
    pub fleet_writer: Option<fleet_store::TelemetryWriter>,
}

async fn metrics_middleware(State(_ctx): State<AppCtx>, req: Request, next: Next) -> Response {
    let method = req.method().to_string();
    let path = req.uri().path().to_string();
    let start = Instant::now();

    let response = next.run(req).await;

    let status = response.status().as_u16().to_string();
    let duration = start.elapsed().as_secs_f64();

    counter!("http_requests_total", "method" => method.clone(), "path" => path.clone(), "status" => status).increment(1);
    histogram!("http_request_duration_seconds", "method" => method, "path" => path)
        .record(duration);

    response
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .json()
        .with_env_filter(std::env::var("RUST_LOG").unwrap_or_else(|_| "info".into()))
        .init();

    let metrics_handle = PrometheusBuilder::new().install_recorder()?;

    let cfg = Config::from_env()?;
    info!("starting BFF on {}", cfg.listen_addr);

    let business = VtnClient::new(
        cfg.vtn_base_url.clone(),
        cfg.bl_client_id.clone(),
        cfg.bl_client_secret.clone(),
    );

    let recorder_status: recorder::SharedRecorderStatus = Arc::new(Default::default());

    // The live fleet feed (fleet-monitor phase 0 §7). Gated on
    // FLEET_MQTT_HOST exactly as the publishing side is: unset means this
    // deployment has no fleet feed, which is configuration rather than fault.
    let fleet_state: fleet::FleetState = Arc::new(tokio::sync::RwLock::new(Default::default()));
    let fleet_status: Arc<tokio::sync::RwLock<fleet::FleetIngestStatus>> =
        Arc::new(tokio::sync::RwLock::new(Default::default()));
    // The history behind that feed (D-7). Same gate as the recorder: with no
    // DATABASE_URL the live view still works, there is simply nothing to look
    // back at.
    let (telemetry_writer, fleet_store_pool) = match cfg.database_url.clone() {
        Some(url) => {
            let (writer, pool) = fleet_store::spawn(url);
            (Some(writer), pool)
        }
        None => (None, Arc::new(tokio::sync::RwLock::new(None))),
    };

    match fleet::FleetMqttConfig::from_env() {
        Some(cfg) => {
            tracing::info!(
                broker = %cfg.broker_host,
                port = cfg.broker_port,
                topic = %cfg.subscription(),
                "fleet ingest: subscribing"
            );
            fleet_status.write().await.enabled = true;
            fleet::spawn_ingest(
                cfg,
                fleet_state.clone(),
                fleet_status.clone(),
                telemetry_writer.clone(),
            );
        }
        None => tracing::info!("fleet ingest: no FLEET_MQTT_HOST, not subscribing"),
    }

    let ctx = AppCtx {
        business: business.clone(),
        cache: Arc::new(TtlCache::new()),
        config: Arc::new(cfg.clone()),
        metrics_handle: Arc::new(metrics_handle),
        recorder_status: recorder_status.clone(),
        fleet: fleet_state.clone(),
        fleet_status: fleet_status.clone(),
        fleet_store: fleet_store_pool,
        fleet_writer: telemetry_writer.clone(),
    };

    // Phase 1 (A-2): VTN recorder, gated on DATABASE_URL being set. Spawning
    // (rather than connecting inline here) means a DB/DNS problem — even at
    // startup — can never fail or delay the rest of the BFF: connection is
    // retried with backoff inside the spawned task (see recorder.rs, the
    // 2026-08-10 fix for the incident where a one-shot connect failure
    // permanently disabled the recorder for 9 days).
    if let Some(database_url) = cfg.database_url.clone() {
        recorder::spawn_recorder(
            database_url,
            business,
            cfg.recorder_poll_secs,
            recorder_status,
        );
    } else {
        info!("DATABASE_URL not set — VTN recorder disabled");
    }

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods([Method::GET, Method::POST, Method::PUT, Method::DELETE])
        .allow_headers(Any);

    let x_request_id = HeaderName::from_static("x-request-id");

    let app = Router::new()
        .route("/api/health", get(routes::health::health))
        .route(
            "/api/programs",
            get(routes::programs::get_programs).post(routes::programs::create_program),
        )
        .route(
            "/api/programs/:id",
            put(routes::programs::update_program).delete(routes::programs::delete_program),
        )
        .route(
            "/api/events",
            get(routes::events::get_events).post(routes::events::create_event),
        )
        .route(
            "/api/events/:id",
            put(routes::events::update_event).delete(routes::events::delete_event),
        )
        .route("/api/vens", get(routes::vens::get_vens))
        .route("/api/vens/:id", delete(routes::vens::delete_ven))
        .route("/api/reports", get(routes::reports::get_reports))
        // The fleet's live state (phase 0 §7). Historical series arrive with
        // the telemetry store; this is what a dashboard opens with.
        .route("/api/fleet/power", get(routes::fleet::fleet_power))
        .route("/api/reports/:id", delete(routes::reports::delete_report))
        .route("/api/metrics", get(routes::metrics::get_metrics))
        .route_layer(middleware::from_fn_with_state(
            ctx.clone(),
            metrics_middleware,
        ))
        .with_state(ctx)
        .layer(PropagateRequestIdLayer::new(x_request_id.clone()))
        .layer(TraceLayer::new_for_http())
        .layer(SetRequestIdLayer::new(x_request_id, MakeRequestUuid))
        .layer(cors);

    let listener = tokio::net::TcpListener::bind(&cfg.listen_addr).await?;
    info!("BFF listening on {}", cfg.listen_addr);
    axum::serve(listener, app).await?;
    Ok(())
}
