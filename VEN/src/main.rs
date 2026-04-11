mod assets;
mod common;
mod config;
mod controller;
mod entities;
mod loops;
mod models;
mod profile;
mod routes;
mod simulator;
mod state;
mod vtn;

use config::Config;
use entities::asset::PlanTrigger;
use metrics_exporter_prometheus::PrometheusBuilder;
use profile::Profile;
use simulator::SimState;
use state::AppState;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{error, info, warn};
use vtn::VtnClient;

#[derive(Clone)]
pub struct AppCtx {
    pub state: AppState,
    pub vtn: VtnClient,
    pub metrics_handle: Arc<metrics_exporter_prometheus::PrometheusHandle>,
    pub trigger_tx: Arc<tokio::sync::watch::Sender<PlanTrigger>>,
    pub profile: Arc<Profile>,
    pub sim: Arc<Mutex<SimState>>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .json()
        .with_env_filter(std::env::var("RUST_LOG").unwrap_or_else(|_| "info".into()))
        .init();

    let metrics_handle = PrometheusBuilder::new().install_recorder()?;

    let cfg = Config::from_env()?;
    info!(
        "starting ven {} listening on {}",
        cfg.ven_name, cfg.listen_addr
    );

    let state = AppState::new();

    // PlanTrigger watch channel — event poll and dispatcher send triggers;
    // planning loop receives them for reactive replanning.
    let (trigger_tx, trigger_rx) = tokio::sync::watch::channel(PlanTrigger::Periodic);
    let trigger_tx = Arc::new(trigger_tx);

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

    // Load simulator profile
    let profile = if let Some(ref path) = cfg.profile_path {
        Profile::load(path).await
    } else {
        warn!("PROFILE_PATH not set, using default profile");
        Profile::default()
    };
    let profile = Arc::new(profile);

    let vtn = VtnClient::new(
        cfg.vtn_base_url.clone(),
        cfg.client_id.clone(),
        cfg.client_secret.clone(),
        cfg.ven_name.clone(),
    );

    // Derive shared data_dir from persist_path
    let data_dir = cfg
        .persist_path
        .as_deref()
        .and_then(|p| std::path::Path::new(p).parent())
        .and_then(|p| p.to_str())
        .unwrap_or("/data")
        .to_string();

    // Initialize simulator state (load from disk or seed from profile)
    let sim_state = {
        let loaded = simulator::persist::load(&data_dir).await;
        let sim = loaded.unwrap_or_else(|| SimState::from_profile(&profile));
        Arc::new(Mutex::new(sim))
    };

    // Spawn background loops
    loops::spawn_program_poll(state.clone(), vtn.clone(), cfg.poll_programs_secs);
    loops::spawn_event_poll(
        state.clone(),
        vtn.clone(),
        cfg.poll_events_secs,
        trigger_tx.clone(),
    );
    loops::spawn_report_poll(state.clone(), vtn.clone(), cfg.poll_reports_secs);
    loops::spawn_sim_tick(
        state.clone(),
        sim_state.clone(),
        profile.clone(),
        cfg.ven_name.clone(),
        vtn.clone(),
        trigger_tx.clone(),
        data_dir.clone(),
    );
    loops::spawn_obligation_check(
        state.clone(),
        sim_state.clone(),
        vtn.clone(),
        cfg.ven_name.clone(),
    );
    loops::spawn_planning(
        state.clone(),
        profile.clone(),
        vtn.clone(),
        cfg.ven_name.clone(),
        trigger_rx,
        sim_state.clone(),
    );
    if let Some(path) = cfg.persist_path.clone() {
        loops::spawn_state_persist(state.clone(), path);
    }

    // Build HTTP app and serve
    let ctx = AppCtx {
        state,
        vtn,
        metrics_handle: Arc::new(metrics_handle),
        trigger_tx,
        profile,
        sim: sim_state.clone(),
    };

    let listener = tokio::net::TcpListener::bind(&cfg.listen_addr).await?;
    info!("listening on {}", cfg.listen_addr);

    axum::serve(listener, routes::build_router(ctx))
        .with_graceful_shutdown(async move {
            let _ = tokio::signal::ctrl_c().await;
            info!("shutdown signal received, persisting sim state");
            let sim_guard = sim_state.lock().await;
            if let Err(e) = simulator::persist::save(&sim_guard, &data_dir).await {
                error!("shutdown persist failed: {e:#}");
            } else {
                info!("sim state persisted on shutdown");
            }
        })
        .await?;

    Ok(())
}
