mod cache;
mod config;
mod error;
mod routes;
mod vtn_client;

use axum::{
    http::Method,
    routing::{delete, get, post, put},
    Router,
};
use std::sync::Arc;
use tower_http::cors::{Any, CorsLayer};
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
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(std::env::var("RUST_LOG").unwrap_or_else(|_| "info".into()))
        .init();

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
    };

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods([Method::GET, Method::POST, Method::PUT, Method::DELETE])
        .allow_headers(Any);

    let app = Router::new()
        .route("/api/health", get(routes::health::health))
        .route("/api/programs", get(routes::programs::get_programs).post(routes::programs::create_program))
        .route("/api/programs/:id", put(routes::programs::update_program).delete(routes::programs::delete_program))
        .route("/api/events", get(routes::events::get_events).post(routes::events::create_event))
        .route("/api/events/:id", put(routes::events::update_event).delete(routes::events::delete_event))
        .route("/api/vens", get(routes::vens::get_vens))
        .route("/api/vens/:id", delete(routes::vens::delete_ven))
        .with_state(ctx)
        .layer(cors);

    let listener = tokio::net::TcpListener::bind(&cfg.listen_addr).await?;
    info!("BFF listening on {}", cfg.listen_addr);
    axum::serve(listener, app).await?;
    Ok(())
}
