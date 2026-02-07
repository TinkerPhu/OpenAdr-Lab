mod cache;
mod config;
mod error;
mod routes;
mod vtn_client;

use axum::{http::Method, routing::get, Router};
use std::sync::Arc;
use tower_http::cors::{Any, CorsLayer};
use tracing::info;

use cache::TtlCache;
use config::Config;
use vtn_client::VtnClient;

#[derive(Clone)]
pub struct AppCtx {
    pub vtn: VtnClient,
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

    let vtn = VtnClient::new(
        cfg.vtn_base_url.clone(),
        cfg.client_id.clone(),
        cfg.client_secret.clone(),
    );

    let ctx = AppCtx {
        vtn,
        cache: Arc::new(TtlCache::new()),
        config: Arc::new(cfg.clone()),
    };

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods([Method::GET])
        .allow_headers(Any);

    let app = Router::new()
        .route("/api/health", get(routes::health::health))
        .route("/api/programs", get(routes::programs::get_programs))
        .route("/api/events", get(routes::events::get_events))
        .route("/api/vens", get(routes::vens::get_vens))
        .with_state(ctx)
        .layer(cors);

    let listener = tokio::net::TcpListener::bind(&cfg.listen_addr).await?;
    info!("BFF listening on {}", cfg.listen_addr);
    axum::serve(listener, app).await?;
    Ok(())
}
