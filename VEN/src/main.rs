mod assets;
mod common;
mod config;
mod controller;
mod entities;
mod ids;
mod tasks;
mod models;
mod planner_events;
mod profile;
mod routes;
mod simulator;
mod state;
mod vtn;

use config::Config;
use entities::asset::PlanTrigger;
use metrics_exporter_prometheus::PrometheusBuilder;
use planner_events::{PlannerEvent, PlannerEventTx};
use profile::{PlannerObjective, Profile};
use simulator::SimState;
use state::AppState;
use std::sync::{atomic::AtomicBool, Arc};
use tokio::sync::{Mutex, RwLock};
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
    pub active_objective: Arc<RwLock<PlannerObjective>>,
    pub planner_event_tx: PlannerEventTx,
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

    // Latch for DeviceDeviation: prevents RateChange (sent every 2s) from
    // overwriting DeviceDeviation in the watch channel before the planner reads it.
    let deviation_pending = Arc::new(AtomicBool::new(false));

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
        Profile::try_load(path).await?
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

    // Initialize simulator state — asset configs always rebuilt from profile so that
    // profile changes (k_loss, thermal_mass, etc.) take effect on every restart.
    let sim_state = {
        let sim = simulator::persist::load_with_profile(&data_dir, &profile).await;
        Arc::new(Mutex::new(sim))
    };

    // Validate absorber configuration at startup
    {
        let sim_guard = sim_state.lock().await;
        controller::absorber::validate_startup(&profile, &sim_guard)?;
    }

    // Spawn background loops
    tasks::spawn_program_poll(state.clone(), vtn.clone(), cfg.poll_programs_secs);
    tasks::spawn_event_poll(
        state.clone(),
        vtn.clone(),
        cfg.poll_events_secs,
        trigger_tx.clone(),
    );
    tasks::spawn_report_poll(state.clone(), vtn.clone(), cfg.poll_reports_secs);

    // Plan F: planner_event_tx must exist before spawn_sim_tick (correction SSE)
    let (planner_event_tx_inner, _) = tokio::sync::broadcast::channel::<PlannerEvent>(128);
    let planner_event_tx: PlannerEventTx = Arc::new(planner_event_tx_inner);

    tasks::spawn_sim_tick(
        state.clone(),
        sim_state.clone(),
        profile.clone(),
        cfg.ven_name.clone(),
        vtn.clone(),
        trigger_tx.clone(),
        data_dir.clone(),
        planner_event_tx.clone(),
        deviation_pending.clone(),
    );
    tasks::spawn_obligation_check(
        state.clone(),
        sim_state.clone(),
        vtn.clone(),
        cfg.ven_name.clone(),
    );
    let active_objective = Arc::new(RwLock::new(profile.planner.objective));
    tasks::spawn_planning(
        state.clone(),
        profile.clone(),
        vtn.clone(),
        cfg.ven_name.clone(),
        trigger_rx,
        sim_state.clone(),
        active_objective.clone(),
        planner_event_tx.clone(),
        deviation_pending.clone(),
    );
    if let Some(path) = cfg.persist_path.clone() {
        tasks::spawn_state_persist(state.clone(), path);
    }

    // Build HTTP app and serve
    let ctx = AppCtx {
        state,
        vtn,
        metrics_handle: Arc::new(metrics_handle),
        trigger_tx,
        profile,
        sim: sim_state.clone(),
        active_objective,
        planner_event_tx,
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
