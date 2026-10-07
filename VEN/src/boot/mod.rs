//! Composition root, in four stages: load configuration, build the ports,
//! spawn the background loops, serve.
//!
//! `main` used to be one 488-line `async fn` at 97.6 % of this crate's
//! file-size cap, growing by roughly twenty lines per new adapter — so the
//! next port had nowhere to go, and nothing in it could be looked at in
//! isolation. The stages below are the structure that function already had
//! implicitly, in its comment headers.

pub mod background;
pub mod ports;
pub mod serve;

use std::sync::Arc;
use tokio::sync::{watch, Mutex, RwLock};
use tracing::{error, info, warn};

use crate::config::Config;
use crate::domain_params::build_domain_params;
use crate::entities::asset::{PlanTrigger, PlanTriggerSignal};
use crate::entities::asset_params::{AssetParams, PvForecastParams};
use crate::entities::planner_params::{PlannerObjective, PlannerParams, SimulatorParams};
use crate::planner_events::{PlannerEvent, PlannerEventTx};
use crate::profile::{comms_loss::CommsLossConfig, Profile};
use crate::services;
use crate::services::heuristics::HeuristicsConfig;
use crate::simulator::SimState;
use crate::state::AppState;
use crate::tasks;
use ports::Ports;

/// Everything the spawn and serve stages need, assembled once.
///
/// A named struct rather than thirty locals in `main`: these values *are* the
/// composition root's state, two stages read them, and the alternative was
/// the tuple-of-seventeen-clones pattern the task spawns used to open with.
pub struct World {
    pub state: AppState,
    pub ports: Ports,
    pub sim: Arc<Mutex<SimState>>,
    pub notifier: services::notify::Notifier,

    pub ven_name: String,
    pub listen_addr: String,
    pub data_dir: String,
    pub persist_path: Option<String>,

    pub sim_params: SimulatorParams,
    pub planner_params: PlannerParams,
    pub asset_params: Vec<AssetParams>,

    pub grid_max_import_kw: f64,
    pub grid_max_export_kw: f64,
    /// BL-17: PV embodied carbon is a per-asset profile scalar resolved once
    /// at startup, same pattern as `min_ev_charge_kw` in
    /// `simulator/plan_context.rs`.
    pub pv_co2_g_kwh: f64,
    pub weather_pv_params: Option<PvForecastParams>,
    pub pv_measurement_enabled: bool,
    pub base_load_measurement_enabled: bool,
    pub comms_loss: Option<CommsLossConfig>,
    pub history_retention_days: u32,
    /// Resolved once from the profile for the daily learner job.
    pub heuristics_config: HeuristicsConfig,

    pub poll: tasks::poll_config::ResolvedPollConfig,
    pub poll_jitter_s: u64,

    pub trigger_tx: Arc<watch::Sender<PlanTriggerSignal>>,
    pub planner_event_tx: PlannerEventTx,
    pub active_objective: Arc<RwLock<PlannerObjective>>,
    pub metrics_handle: Arc<metrics_exporter_prometheus::PrometheusHandle>,
}

pub fn init_tracing() {
    tracing_subscriber::fmt()
        .json()
        .with_env_filter(std::env::var("RUST_LOG").unwrap_or_else(|_| "info".into()))
        .init();
}

/// Load and validate the profile. An invalid profile is fatal: the VEN would
/// otherwise run with physics nobody intended.
pub async fn load_profile(cfg: &Config) -> anyhow::Result<Arc<Profile>> {
    let profile = match cfg.profile_path {
        Some(ref path) => Profile::try_load(path).await?,
        None => {
            warn!("PROFILE_PATH not set, using default profile");
            Profile::default()
        }
    };
    if let Err(violations) = profile.validate() {
        for v in &violations {
            eprintln!("profile error: {v}");
        }
        std::process::exit(1);
    }
    Ok(Arc::new(profile))
}

/// Restore the persisted `AppState`, if there is one. Best-effort: a
/// corrupt or missing file leaves the fresh state in place.
pub async fn restore_persisted_state(cfg: &Config, state: &AppState) {
    let Some(path) = cfg.persist_path.clone() else {
        return;
    };
    let Ok(s) = tokio::fs::read_to_string(&path).await else {
        return;
    };
    match state.load_from_json(&s).await {
        Ok(()) => info!("loaded persisted state from {path}"),
        Err(e) => error!("failed to load persisted state: {e:#}"),
    }
}

impl World {
    pub async fn assemble(
        cfg: &Config,
        profile: Arc<Profile>,
        state: AppState,
        metrics_handle: metrics_exporter_prometheus::PrometheusHandle,
        trigger_tx: Arc<watch::Sender<PlanTriggerSignal>>,
    ) -> Self {
        let (sim_params, planner_params, asset_params) = build_domain_params(&profile);
        let data_dir = data_dir_from(cfg);
        let ports = Ports::build(cfg, &profile, &data_dir);

        background::spawn_fleet_trace(&state, ports.telemetry.clone(), cfg.ven_name.clone());
        seed_from_store(&state, &ports).await;

        // Asset configs are always rebuilt from the profile, so profile
        // changes (k_loss, thermal_mass, …) take effect on every restart.
        let sim = Arc::new(Mutex::new(
            crate::simulator::persist::load_with_params(
                &data_dir,
                &asset_params,
                chrono::Utc::now(),
            )
            .await,
        ));

        let poll = tasks::poll_config::resolve(cfg, &profile);
        let poll_jitter_s = tasks::poll_config::compute_startup_jitter_s(
            poll.events_secs,
            poll.startup_jitter_fixed_pct,
            poll.startup_jitter_random_max_pct,
            &mut <rand::rngs::StdRng as rand::SeedableRng>::from_entropy(),
        )
        .round() as u64;

        let (planner_event_tx_inner, _) = tokio::sync::broadcast::channel::<PlannerEvent>(128);

        Self {
            notifier: services::notify::Notifier::new(ports.history.clone()),
            active_objective: Arc::new(RwLock::new(planner_params.objective)),
            ven_name: cfg.ven_name.clone(),
            listen_addr: cfg.listen_addr.clone(),
            persist_path: cfg.persist_path.clone(),
            grid_max_import_kw: profile.grid.max_import_kw,
            grid_max_export_kw: profile.grid.max_export_kw,
            pv_co2_g_kwh: pv_co2_g_kwh(&asset_params),
            weather_pv_params: profile.weather_pv_params(),
            pv_measurement_enabled: profile.pv_measurement_enabled(),
            base_load_measurement_enabled: profile.base_load_measurement_enabled(),
            comms_loss: profile.comms_loss,
            history_retention_days: profile.history.retention_days,
            heuristics_config: profile.heuristics_config(),
            metrics_handle: Arc::new(metrics_handle),
            planner_event_tx: Arc::new(planner_event_tx_inner),
            state,
            ports,
            sim,
            data_dir,
            sim_params,
            planner_params,
            asset_params,
            poll,
            poll_jitter_s,
            trigger_tx,
        }
    }
}

/// The PlanTrigger watch channel: event poll and dispatcher send triggers,
/// the planning loop receives them for reactive replanning.
pub fn trigger_channel() -> (
    Arc<watch::Sender<PlanTriggerSignal>>,
    watch::Receiver<PlanTriggerSignal>,
) {
    let (tx, rx) = watch::channel(PlanTriggerSignal::bare(PlanTrigger::Periodic));
    (Arc::new(tx), rx)
}

fn data_dir_from(cfg: &Config) -> String {
    cfg.persist_path
        .as_deref()
        .and_then(|p| std::path::Path::new(p).parent())
        .and_then(|p| p.to_str())
        .unwrap_or("/data")
        .to_string()
}

fn pv_co2_g_kwh(asset_params: &[AssetParams]) -> f64 {
    asset_params
        .iter()
        .find_map(|p| match p {
            AssetParams::Pv(pv) => Some(pv.co2_g_kwh),
            _ => None,
        })
        .unwrap_or(0.0)
}

/// Re-seed the two in-memory feeds that outlive a restart: WP4.2 (BL-19)
/// comfort-curve overrides and the WP4.3 (BL-20) notification ring.
async fn seed_from_store(state: &AppState, ports: &Ports) {
    if let Some(s) = ports.settings.clone() {
        services::comfort::load_overrides(state, s).await;
    }
    let Some(h) = ports.history.clone() else {
        return;
    };
    let seeded = tokio::task::spawn_blocking(move || h.query_notifications(None, 200, None)).await;
    if let Ok(Ok(rows)) = seeded {
        for n in rows {
            state.push_notification(n).await;
        }
    }
}
