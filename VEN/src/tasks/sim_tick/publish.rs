// Async publishers and persist helpers for the simulator tick.

use chrono::{DateTime, Utc};
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{error, info};

use crate::controller;
use crate::controller::SimSnapshot;
use crate::entities::asset::PlanTrigger;
use crate::entities::capacity_curve::CapacityCurve;
use crate::entities::plan::{SiteFlexibilityEnvelope, SiteFlexibilityForecastSlot};
use crate::entities::tariff_snapshot::TariffSnapshot;
use crate::simulator::SensorSnapshot;
use crate::simulator::SimState;
use crate::state::AppState;

#[allow(clippy::too_many_arguments)]
pub(crate) async fn publish_sim_tick_result(
    sensor: SensorSnapshot,
    sim_snap: SimSnapshot,
    envelope: SiteFlexibilityEnvelope,
    forecast: Vec<SiteFlexibilityForecastSlot>,
    capacity_curves: (CapacityCurve, CapacityCurve),
    state: &AppState,
    trigger_tx: &tokio::sync::watch::Sender<PlanTrigger>,
    rates_snap: &[TariffSnapshot],
    dt_s: f64,
    now: DateTime<Utc>,
    pv_co2_g_kwh: f64,
    telemetry: &dyn crate::controller::telemetry_port::TelemetryPort,
    ven_name: &str,
) -> SimSnapshot {
    // Update sensor snapshot (backward compat)
    state.update_sensor(sensor).await;

    // Update sim in app state.

    // Shiftable-load completion: starting is now handled entirely by the
    // ordinary per-tick setpoint path (`ShiftableLoadAsset::step()` latches
    // non-interruptible on the plan's first nonzero setpoint), and a
    // finished load is already removed from `SimState` by its generic
    // `is_removable()` pass (design.md D3a) before `sim_snap` was built. This
    // just detects that a still-open request's asset_id has disappeared from
    // the live snapshot and closes out the request/ledger side of it.
    {
        let loads = state.shiftable_loads().await;
        for load in &loads {
            if !sim_snap.assets.contains_key(load.asset_id.as_str()) {
                info!(asset_id = %load.asset_id, "shiftable load completed");
                state.complete_shiftable(load.id).await;
                let _ = trigger_tx.send(PlanTrigger::UserRequest);
            }
        }
    }

    state.update_sim(sim_snap.clone()).await;

    // Post-tick: consolidated ledger accounting, plus (BL-39) per-session
    // accumulated cost for any active UserRequest sharing the ledger's tick.
    let mut ledger = state.asset_ledger().await;
    let mut requests = state.active_requests().await;
    controller::monitor::record_tick(
        &mut ledger,
        &mut requests,
        &sim_snap,
        rates_snap,
        dt_s,
        now,
        pv_co2_g_kwh,
    );
    state.set_asset_ledger(ledger).await;
    state.set_active_requests(requests).await;

    // Refresh site envelope (computed in-lock from final sim state).
    state.set_site_envelope(envelope).await;
    // Refresh the forward headroom forecast — recomputed fresh every tick,
    // anchored to the real state just published above (see
    // `SiteFlexibilityForecastSlot`'s doc comment for why this must never
    // be read statically off the plan).
    state.set_site_headroom_forecast(forecast).await;
    // Sustained-commitment capacity curves — same "recompute fresh every
    // tick" reasoning as the headroom forecast above, see
    // `controller::capacity_headroom`'s module doc for why this is a
    // separate computation, not derived from `forecast`.
    state.set_capacity_curves(capacity_curves).await;

    // The live fleet feed. Fire-and-forget by design: a fleet view going dark
    // is an observability problem, and making the tick wait on a broker would
    // turn it into an operational one (fleet-monitor phase 0 §6.2).
    if telemetry.sample_due(now) {
        telemetry
            .publish_telemetry(crate::controller::telemetry_port::telemetry_body(
                ven_name, &sim_snap,
            ))
            .await;
    }

    sim_snap
}

pub(crate) async fn persist_sim_state(sim: &Arc<Mutex<SimState>>, data_dir: &str) {
    let sim_clone = { sim.lock().await.clone() };
    if let Err(e) = crate::simulator::persist::save(&sim_clone, data_dir).await {
        error!("sim persist failed: {e:#}");
    }
}
