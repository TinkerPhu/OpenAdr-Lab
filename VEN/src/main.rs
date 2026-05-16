mod assets;
mod common;
mod config;
mod controller;
mod entities;
mod ids;
mod services;
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
use profile::Profile;
use std::collections::HashMap;
use crate::assets::ControlDescriptor;
use simulator::SimState;

use crate::entities::asset_params::AssetParams;
use crate::entities::planner_params::{
    AbsorberAssetParams, AbsorberParams, PlannerObjective, PlannerParams, SimulatorParams,
};
use state::AppState;
use std::sync::{atomic::AtomicBool, Arc};
use tokio::sync::{Mutex, RwLock};
use tracing::{error, info, warn};
use vtn::VtnClient;
use crate::controller::VtnPort;

#[derive(Clone)]
pub struct AppCtx {
    pub state: AppState,
    pub vtn: VtnClient,
    pub metrics_handle: Arc<metrics_exporter_prometheus::PrometheusHandle>,
    pub trigger_tx: Arc<tokio::sync::watch::Sender<PlanTrigger>>,
    /// Pre-computed simulator schema (asset → control descriptors).
    /// Built once at startup from `profile`; route handlers access it without
    /// touching the raw `Profile` type or acquiring any lock.
    pub sim_schema: Arc<HashMap<String, Vec<ControlDescriptor>>>,
    pub sim: Arc<Mutex<SimState>>,
    pub active_objective: Arc<RwLock<PlannerObjective>>,
    pub planner_event_tx: PlannerEventTx,
}

fn build_domain_params(
    profile: &Profile,
) -> (SimulatorParams, PlannerParams, AbsorberParams, Vec<AssetParams>) {
    let sim_params = SimulatorParams {
        tick_s: profile.simulator.tick_s,
        persist_every_s: profile.simulator.persist_every_s,
        report_interval_s: profile.simulator.report_interval_s,
    };
    let planner_params = PlannerParams {
        plan_step_s: profile.planner.plan_step_s,
        plan_horizon_h: profile.planner.plan_horizon_h,
        replan_interval_s: profile.planner.replan_interval_s,
        deviation_threshold_kw: profile.planner.deviation_threshold_kw,
        deviation_trigger_ticks: profile.planner.deviation_trigger_ticks,
        correction_min_kw: profile.planner.correction_min_kw,
        w_energy: profile.planner.w_energy,
        w_ghg: profile.planner.w_ghg,
        w_grid: profile.planner.w_grid,
        c_bat_wear_eur_kwh: profile.planner.c_bat_wear_eur_kwh,
        c_ev_startup_eur: profile.planner.c_ev_startup_eur,
        c_bat_startup_eur: profile.planner.c_bat_startup_eur,
        c_ev_ramp_eur_kw: profile.planner.c_ev_ramp_eur_kw,
        c_bat_ramp_eur_kw: profile.planner.c_bat_ramp_eur_kw,
        c_bat_ev_coexist_eur_kwh: profile.planner.c_bat_ev_coexist_eur_kwh,
        w_viol: profile.planner.w_viol,
        pen_imp_eur_kwh: profile.planner.pen_imp_eur_kwh,
        pen_exp_eur_kwh: profile.planner.pen_exp_eur_kwh,
        v_ev_extra_eur_kwh: profile.planner.v_ev_extra_eur_kwh,
        w_tier_penalty_eur: profile.planner.w_tier_penalty_eur,
        objective: profile.planner.objective,
        plan_adoption_threshold_eur: profile.planner.plan_adoption_threshold_eur,
        plan_adoption_decay_s: profile.planner.plan_adoption_decay_s,
        phase2_epsilon_eur: profile.planner.phase2_epsilon_eur,
    };
    let absorber_params = AbsorberParams {
        enabled: profile.absorber.enabled,
        dead_band_kw: profile.absorber.dead_band_kw,
        dead_band_clearing_ticks: profile.absorber.dead_band_clearing_ticks,
        deviation_trigger_ticks: profile.planner.deviation_trigger_ticks,
        assets: profile
            .absorber
            .assets
            .iter()
            .map(|a| AbsorberAssetParams {
                id: a.id.clone(),
                priority: a.priority,
                min_state_linger_s: a.min_state_linger_s,
                ev_departure_guard_s: a.ev_departure_guard_s,
            })
            .collect(),
    };
    let asset_params = profile.asset_params();
    (sim_params, planner_params, absorber_params, asset_params)
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
    let (sim_params, planner_params, absorber_params, asset_params) = build_domain_params(&profile);
    let grid_max_import_kw = profile.grid.max_import_kw;
    let grid_max_export_kw = profile.grid.max_export_kw;

    let vtn = VtnClient::new(
        cfg.vtn_base_url.clone(),
        cfg.client_id.clone(),
        cfg.client_secret.clone(),
        cfg.ven_name.clone(),
    );
    let vtn_port: Arc<dyn VtnPort> = Arc::new(vtn.clone());

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
        let sim = simulator::persist::load_with_params(&data_dir, &sim_params, &asset_params).await;
        Arc::new(Mutex::new(sim))
    };

    // Validate absorber configuration at startup
    {
        let sim_guard = sim_state.lock().await;
        let sim_snap = sim_guard.to_sim_snapshot();
        controller::absorber::validate_startup(&absorber_params, &sim_snap)?;
    }

    // Spawn background loops
    tasks::spawn_program_poll(state.clone(), vtn_port.clone(), cfg.poll_programs_secs);
    tasks::spawn_event_poll(
        state.clone(),
        vtn_port.clone(),
        cfg.poll_events_secs,
        trigger_tx.clone(),
    );
    tasks::spawn_report_poll(state.clone(), vtn_port.clone(), cfg.poll_reports_secs);

    // Plan F: planner_event_tx must exist before spawn_sim_tick (correction SSE)
    let (planner_event_tx_inner, _) = tokio::sync::broadcast::channel::<PlannerEvent>(128);
    let planner_event_tx: PlannerEventTx = Arc::new(planner_event_tx_inner);

    tasks::spawn_sim_tick(
        state.clone(),
        sim_state.clone(),
        sim_params,
        absorber_params,
        cfg.ven_name.clone(),
        vtn_port.clone(),
        trigger_tx.clone(),
        data_dir.clone(),
        planner_event_tx.clone(),
        deviation_pending.clone(),
    );
    tasks::spawn_obligation_check(
        state.clone(),
        sim_state.clone(),
        vtn_port.clone(),
        cfg.ven_name.clone(),
    );
    let active_objective = Arc::new(RwLock::new(planner_params.objective));
    tasks::spawn_planning(
        state.clone(),
        planner_params.clone(),
        grid_max_import_kw,
        grid_max_export_kw,
        asset_params.clone(),
        vtn_port.clone(),
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
    let sim_schema = Arc::new(simulator::schema_from_params(&asset_params));
    let ctx = AppCtx {
        state,
        vtn,
        metrics_handle: Arc::new(metrics_handle),
        trigger_tx,
        sim_schema,
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
