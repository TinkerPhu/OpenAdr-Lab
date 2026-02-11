mod cache;
mod config;
mod error;
mod routes;
mod vtn_client;

use axum::{
    extract::{Request, State},
    http::{HeaderName, Method},
    middleware::{self, Next},
    response::Response,
    routing::{delete, get, post, put},
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
    pub business: VtnClient,
    pub ven_mgr: VtnClient,
    pub cache: Arc<TtlCache>,
    pub config: Arc<Config>,
    pub metrics_handle: Arc<metrics_exporter_prometheus::PrometheusHandle>,
}

async fn metrics_middleware(
    State(_ctx): State<AppCtx>,
    req: Request,
    next: Next,
) -> Response {
    let method = req.method().to_string();
    let path = req.uri().path().to_string();
    let start = Instant::now();

    let response = next.run(req).await;

    let status = response.status().as_u16().to_string();
    let duration = start.elapsed().as_secs_f64();

    counter!("http_requests_total", "method" => method.clone(), "path" => path.clone(), "status" => status).increment(1);
    histogram!("http_request_duration_seconds", "method" => method, "path" => path).record(duration);

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
        cfg.business_client_id.clone(),
        cfg.business_client_secret.clone(),
    );

    let ven_mgr = VtnClient::new(
        cfg.vtn_base_url.clone(),
        cfg.ven_mgr_client_id.clone(),
        cfg.ven_mgr_client_secret.clone(),
    );

    let ctx = AppCtx {
        business,
        ven_mgr,
        cache: Arc::new(TtlCache::new()),
        config: Arc::new(cfg.clone()),
        metrics_handle: Arc::new(metrics_handle),
    };

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods([Method::GET, Method::POST, Method::PUT, Method::DELETE])
        .allow_headers(Any);

    let x_request_id = HeaderName::from_static("x-request-id");

    let app = Router::new()
        .route("/api/health", get(routes::health::health))
        .route("/api/programs", get(routes::programs::get_programs).post(routes::programs::create_program))
        .route("/api/programs/:id", put(routes::programs::update_program).delete(routes::programs::delete_program))
        .route("/api/events", get(routes::events::get_events).post(routes::events::create_event))
        .route("/api/events/:id", put(routes::events::update_event).delete(routes::events::delete_event))
        .route("/api/vens", get(routes::vens::get_vens))
        .route("/api/vens/:id", delete(routes::vens::delete_ven))
        .route("/api/reports", get(routes::reports::get_reports))
        .route("/api/reports/:id", delete(routes::reports::delete_report))
        .route("/api/metrics", get(routes::metrics::get_metrics))
        .route_layer(middleware::from_fn_with_state(ctx.clone(), metrics_middleware))
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
