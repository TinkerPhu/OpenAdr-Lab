//! The HTTP surface and the shutdown path.

use std::sync::Arc;
use tracing::{error, info};

use super::World;
use crate::{routes, simulator, AppCtx};

pub async fn serve(w: World) -> anyhow::Result<()> {
    let listen_addr = w.listen_addr.clone();
    let data_dir = w.data_dir.clone();
    let sim = w.sim.clone();

    let listener = tokio::net::TcpListener::bind(&listen_addr).await?;
    info!("listening on {listen_addr}");

    axum::serve(
        listener,
        routes::build_router(app_ctx(w))
            .into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .with_graceful_shutdown(persist_on_shutdown(sim, data_dir))
    .await?;

    Ok(())
}

fn app_ctx(w: World) -> AppCtx {
    // One handle on the shared simulator behind every port the routes and services use; the
    // tick loop and the other tasks keep the concrete `Arc<Mutex<SimState>>` (`w.sim`).
    let handle = Arc::new(simulator::SimHandle::new(w.sim.clone()));
    AppCtx {
        sim_schema: Arc::new(simulator::schema_from_params(&w.asset_params)),
        telemetry: w.ports.telemetry,
        state: w.state,
        vtn: w.ports.vtn,
        metrics_handle: w.metrics_handle,
        trigger_tx: w.trigger_tx,
        roster: handle.clone(),
        headroom: handle.clone(),
        sim_read: handle,
        active_objective: w.active_objective,
        planner_event_tx: w.planner_event_tx,
        history: w.ports.history,
        notifier: w.notifier,
        settings: w.ports.settings,
        weather: w.ports.weather,
        weather_pv_params: w.weather_pv_params,
        pv_measurement: w.ports.pv_measurement,
        pv_measurement_enabled: w.pv_measurement_enabled,
        base_load_measurement: w.ports.base_load_measurement,
        base_load_measurement_enabled: w.base_load_measurement_enabled,
        comms_loss_debounce_s: w.comms_loss.map(|c| c.debounce_s),
        grid_max_import_kw: w.grid_max_import_kw,
        grid_max_export_kw: w.grid_max_export_kw,
    }
}

/// Wait for whichever stop signal arrives first, then persist.
///
/// `docker stop` / `docker compose up -d` (container recreate) send SIGTERM,
/// not SIGINT — listening only for `ctrl_c()` meant every container-initiated
/// stop skipped this handler entirely, leaving state.json up to
/// `persist_every_s` seconds stale relative to the continuously-decaying
/// pv_smoothing offset. On the next start, reloading that stale
/// (larger-magnitude) offset looked exactly like a fresh external
/// `/sim/inject` call — see the ven-1 PV-injection mystery in
/// `docs/history/project_journal.md`.
async fn persist_on_shutdown(sim: Arc<tokio::sync::Mutex<simulator::SimState>>, data_dir: String) {
    #[cfg(unix)]
    {
        let mut sigterm = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler");
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {}
            _ = sigterm.recv() => {}
        }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
    info!("shutdown signal received, persisting sim state");
    let sim_guard = sim.lock().await;
    if let Err(e) = simulator::persist::save(&sim_guard, &data_dir).await {
        error!("shutdown persist failed: {e:#}");
    } else {
        info!("sim state persisted on shutdown");
    }
}
