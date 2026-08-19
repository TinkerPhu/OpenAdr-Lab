// Async publishers and persist helpers for the simulator tick.

use chrono::{DateTime, Utc};
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{error, info};

use crate::controller;
use crate::controller::history_port::record_report_sent;
use crate::controller::HistoryPort;
use crate::controller::SimSnapshot;
use crate::controller::VtnPort;
use crate::entities::asset::PlanTrigger;
use crate::entities::plan::{Plan, SiteFlexibilityEnvelope, SiteFlexibilityForecastSlot};
use crate::entities::tariff_snapshot::TariffSnapshot;
use crate::models::SensorSnapshot;
use crate::simulator::SimState;
use crate::state::AppState;

#[allow(clippy::too_many_arguments)]
pub(crate) async fn publish_sim_tick_result(
    sensor: SensorSnapshot,
    mut sim_snap: SimSnapshot,
    envelope: SiteFlexibilityEnvelope,
    forecast: Vec<SiteFlexibilityForecastSlot>,
    plan_snap: Option<&Plan>,
    state: &AppState,
    trigger_tx: &tokio::sync::watch::Sender<PlanTrigger>,
    rates_snap: &[TariffSnapshot],
    dt_s: f64,
    now: DateTime<Utc>,
    pv_co2_g_kwh: f64,
) -> SimSnapshot {
    // Update sensor snapshot (backward compat)
    state.update_sensor(sensor).await;

    // SITE_RESIDUAL (BL-08): computed from the raw simulator snapshot,
    // before any synthetic assets (shiftable-load runtimes) are inserted
    // below, so a currently-running shiftable load is not double-counted
    // as unexplained residual.
    {
        let residual_kw = controller::residual::compute_site_residual_kw(&sim_snap);
        sim_snap.assets.insert(
            controller::residual::SITE_RESIDUAL_ASSET_ID.to_string(),
            controller::residual::site_residual_snapshot(residual_kw),
        );
    }

    // Update sim in app state — augmented with shiftable runtimes

    // ── Shiftable load runtime: start / complete / augment ──────

    // Start: detect shiftable loads that the current plan slot wants
    // to run but that have no runtime yet.
    if let Some(plan) = plan_snap {
        if let Some(slot) = plan.slots.iter().find(|s| s.start <= now && now < s.end) {
            let runtimes = state.shiftable_runtimes().await;
            let loads = state.shiftable_loads().await;
            for alloc in &slot.allocations {
                if sim_snap.assets.contains_key(alloc.asset_id.as_str()) {
                    continue;
                }
                let already_running = runtimes.iter().any(|r| r.asset_id == alloc.asset_id);
                if !already_running {
                    if let Some(load) = loads.iter().find(|l| l.asset_id == alloc.asset_id) {
                        let ends_at = now + chrono::Duration::minutes(load.duration_min as i64);
                        state
                            .start_shiftable(
                                crate::entities::device_session::ShiftableLoadRuntime {
                                    load_id: load.id,
                                    asset_id: load.asset_id.clone(),
                                    power_kw: load.power_kw,
                                    started_at: now,
                                    ends_at,
                                },
                            )
                            .await;
                        info!(asset_id = %load.asset_id, ends_at = %ends_at, "shiftable load started");
                    }
                }
            }
        }
    }

    // Complete: remove expired runtimes and trigger replan.
    {
        let runtimes = state.shiftable_runtimes().await;
        for rt in &runtimes {
            if now >= rt.ends_at {
                info!(asset_id = %rt.asset_id, "shiftable load completed");
                state.complete_shiftable(rt.load_id).await;
                let _ = trigger_tx.send(PlanTrigger::UserRequest);
            }
        }
    }

    // Augment SimSnapshot with running shiftable runtimes so they
    // appear in GET /sim and ledger accounting.
    {
        let runtimes = state.shiftable_runtimes().await;
        for rt in &runtimes {
            if rt.is_running(now) {
                sim_snap.assets.insert(
                    rt.asset_id.clone(),
                    crate::controller::AssetSnapshot {
                        power_kw: rt.power_kw,
                        asset_type: "base_load".into(),
                        cap_max_import_kw: rt.power_kw,
                        cap_max_export_kw: 0.0,
                        available_discharge_kwh: None,
                        available_charge_kwh: None,
                        default_setpoint_kw: rt.power_kw,
                        setpoint_kw: rt.power_kw,
                        values: {
                            let mut m = std::collections::HashMap::new();
                            m.insert("running".into(), 1.0);
                            m.insert("ends_at_unix".into(), rt.ends_at.timestamp() as f64);
                            m
                        },
                    },
                );
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

    sim_snap
}

/// R-43 (design.md D3): `history`, when present, receives one `ReportSent`
/// row per report the VTN accepts. `report_type` is taken from `reportName`
/// (best-available identifier at this call site — timer-driven reports don't
/// carry a single obligation payload_type).
pub(crate) async fn run_measurement_reports(
    state: &AppState,
    sim_snap: &SimSnapshot,
    vtn: &dyn VtnPort,
    ven_name: &str,
    now: DateTime<Utc>,
    history: Option<Arc<dyn HistoryPort>>,
) {
    use crate::controller::reporter::AssetReportSample;
    let events = state.events().await;

    let asset_samples: std::collections::HashMap<String, Vec<AssetReportSample>> = sim_snap
        .assets
        .iter()
        .map(|(id, asset)| {
            let sample = AssetReportSample {
                ts: sim_snap.ts,
                power_kw: asset.power_kw,
                soc: asset.values.get("soc").copied(),
            };
            (id.clone(), vec![sample])
        })
        .collect();

    let grid_net_import_kw = sim_snap.grid.net_power_w.max(0.0) / 1000.0;
    let grid_net_export_kw = (-sim_snap.grid.net_power_w).max(0.0) / 1000.0;

    let reports = controller::reporter::build_measurement_reports_for_active_events(
        &events,
        &asset_samples,
        grid_net_import_kw,
        grid_net_export_kw,
        ven_name,
        now,
    );
    for report in reports {
        let report_name = report.reportName.clone().unwrap_or_default();
        let event_id = report.eventID.clone().unwrap_or_default();
        match vtn.upsert_report(report).await {
            Ok(()) => {
                record_report_sent(history.clone(), report_name, event_id, now).await;
            }
            Err(e) => error!("measurement report submission failed: {e:#}"),
        }
    }
}

pub(crate) async fn persist_sim_state(sim: &Arc<Mutex<SimState>>, data_dir: &str) {
    let sim_clone = { sim.lock().await.clone() };
    if let Err(e) = crate::simulator::persist::save(&sim_clone, data_dir).await {
        error!("sim persist failed: {e:#}");
    }
}

#[cfg(test)]
mod tests {
    //! R-43: `run_measurement_reports` must append a `ReportSent` row for
    //! every report the VTN accepts, and stay a no-op (not an error) when no
    //! `HistoryPort` is configured.
    use super::*;
    use crate::controller::simulator_port::GridSnapshot;
    use crate::controller::vtn_port::{OadrEvent, OadrInterval, OadrPayload};
    use crate::services::test_support::mock_history_port::MockHistoryPort;
    use crate::services::test_support::mock_vtn::MockVtn;

    fn active_event() -> OadrEvent {
        OadrEvent {
            id: "evt-1".to_string(),
            programID: "prog-1".to_string(),
            eventName: None,
            priority: None,
            createdDateTime: None,
            intervalPeriod: None,
            intervals: vec![OadrInterval {
                intervalPeriod: None,
                payloads: vec![OadrPayload {
                    r#type: "SIMPLE".to_string(),
                    values: vec![],
                }],
            }],
            reportDescriptors: None,
        }
    }

    fn empty_sim_snap(now: DateTime<Utc>) -> SimSnapshot {
        SimSnapshot {
            ts: now,
            grid: GridSnapshot {
                net_power_w: 1000.0,
                voltage_v: 230.0,
                import_kwh: 0.0,
                export_kwh: 0.0,
            },
            assets: std::collections::HashMap::new(),
        }
    }

    #[tokio::test]
    async fn run_measurement_reports_appends_a_report_sent_row() {
        let state = AppState::new();
        state.set_events(vec![active_event()], 500).await;
        let vtn = MockVtn::new();
        let history: Arc<dyn HistoryPort> = Arc::new(MockHistoryPort::new());
        let now = Utc::now();

        run_measurement_reports(
            &state,
            &empty_sim_snap(now),
            &vtn,
            "ven-1",
            now,
            Some(history.clone()),
        )
        .await;

        assert_eq!(vtn.submitted().len(), 1, "one measurement report submitted");
        let page = history
            .query_reports(
                now - chrono::Duration::seconds(1),
                now + chrono::Duration::seconds(1),
                100,
                0,
            )
            .unwrap();
        assert_eq!(page.rows.len(), 1, "one ReportSent row appended");
        assert_eq!(page.rows[0].event_id, "evt-1");
    }

    #[tokio::test]
    async fn run_measurement_reports_no_history_port_is_a_no_op_not_a_panic() {
        let state = AppState::new();
        state.set_events(vec![active_event()], 500).await;
        let vtn = MockVtn::new();
        let now = Utc::now();

        run_measurement_reports(&state, &empty_sim_snap(now), &vtn, "ven-1", now, None).await;

        assert_eq!(
            vtn.submitted().len(),
            1,
            "report still submitted even without a HistoryPort"
        );
    }
}
