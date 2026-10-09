mod app_ctx;
mod assets;
mod boot;
mod config;
mod controller;
mod domain_params;
mod entities;
mod fleet_telemetry;
mod history_store;
mod ids;
mod measurement;
mod measurement_translation;
mod planner_events;
mod profile;
mod routes;
mod services;
mod simulator;
mod state;
mod tasks;
#[cfg(test)]
mod ui_types;
mod vtn;
mod vtn_reports;
mod weather;

use config::Config;
use metrics_exporter_prometheus::PrometheusBuilder;
use state::AppState;
use tracing::info;

pub use app_ctx::AppCtx;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    boot::init_tracing();
    let metrics_handle = PrometheusBuilder::new().install_recorder()?;

    let cfg = Config::from_env()?;
    info!(
        "starting ven {} listening on {}",
        cfg.ven_name, cfg.listen_addr
    );

    let state = AppState::new();
    boot::restore_persisted_state(&cfg, &state).await;
    let profile = boot::load_profile(&cfg).await?;
    let (trigger_tx, trigger_rx) = boot::trigger_channel();

    let world = boot::World::assemble(&cfg, profile, state, metrics_handle, trigger_tx).await;
    boot::background::spawn_all(&world, trigger_rx);
    boot::serve::serve(world).await
}
