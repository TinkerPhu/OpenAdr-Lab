//! Background OpenADR event polling loop. The pure change-detection core
//! lives in `detect.rs` (split out under R-64 to stay under the `tasks/`
//! 200-production-line cap); this file is the impure I/O loop that drives it.

mod detect;

use chrono::Utc;
use metrics::counter;
use std::sync::Arc;
use tracing::{error, info};

use crate::controller;
use crate::controller::VtnPort;
use crate::entities::asset::PlanTrigger;
use crate::state::AppState;
use crate::tasks::backoff::Backoff;
use detect::detect_event_changes;

/// `startup_delay_s` (GB-09, WP2.5): see `spawn_program_poll`.
pub(crate) fn spawn_event_poll(
    state: AppState,
    vtn: Arc<dyn VtnPort>,
    secs: u64,
    trigger_tx: Arc<tokio::sync::watch::Sender<PlanTrigger>>,
    notifier: crate::services::notify::Notifier,
    startup_delay_s: u64,
    history: Option<Arc<dyn crate::controller::HistoryPort>>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        if startup_delay_s > 0 {
            tokio::time::sleep(std::time::Duration::from_secs(startup_delay_s)).await;
        }
        let mut backoff = Backoff::new(secs, secs.saturating_mul(30).min(900), 0);
        // Track previous event IDs and tariff count for change detection (T034/T035)
        let mut prev_event_ids: std::collections::HashSet<String> =
            std::collections::HashSet::new();
        let mut prev_tariff_count: usize = 0;
        let mut prev_import_limit: Option<f64> = None;
        let mut signal_prevs = super::poll_signals::SignalPrevs::default();
        let mut vtn_ok = true; // WP4.3: notify only on reachable⇄unreachable edges
        loop {
            use crate::services::notify::notify_outage_edge as outage_edge;
            match vtn.fetch_events().await {
                Ok(events) => {
                    vtn_ok = outage_edge(&notifier, &state, Utc::now(), vtn_ok, true).await;
                    counter!("poll_success_total", "resource" => "events").increment(1);
                    info!(resource = "events", count = events.len(), "poll success");

                    let now = Utc::now();
                    let changes = detect_event_changes(
                        &events,
                        &prev_event_ids,
                        prev_tariff_count,
                        prev_import_limit,
                        now,
                    );

                    // Check before the trace_events vec is consumed by the for loop.
                    let any_change = !changes.trace_events.is_empty();

                    for evt in changes.trace_events {
                        state.push_controller_event(evt).await;
                    }
                    prev_event_ids = changes.current_ids;
                    prev_tariff_count = changes.rates.len();
                    prev_import_limit = changes.capacity.import_limit_kw;

                    state.set_planned_tariffs(changes.rates).await;
                    state.set_capacity_state(changes.capacity).await;
                    state
                        .set_planned_capacity_limits(changes.capacity_schedule)
                        .await;

                    // WP3.1/3.2/3.4: apply alert/SIMPLE/dispatch/charge-state
                    // signal changes (see poll_signals.rs). True = a plan
                    // trigger was already sent; don't overwrite it below.
                    let signal_trigger_sent = super::poll_signals::apply_signal_changes(
                        &state,
                        &trigger_tx,
                        &notifier,
                        changes.signals,
                        now,
                        &mut signal_prevs,
                    )
                    .await;

                    let existing_obs = state.report_obligations().await;
                    let new_obs = controller::openadr_interface::extract_report_obligations(
                        &events,
                        now,
                        &existing_obs,
                    );
                    state.add_obligations(new_obs).await;
                    state.retire_obligations_not_in(&prev_event_ids).await;

                    state.set_events(events, 500).await;

                    if let Some(h) = history.clone() {
                        for row in changes.event_records {
                            let h = h.clone();
                            let row = row.clone();
                            let _ =
                                tokio::task::spawn_blocking(move || h.append_event_received(&row))
                                    .await;
                        }
                    }

                    // Signal planner only when something actually changed (new/expired event,
                    // tariff count change, or capacity change). Firing on every poll caused
                    // continuous replanning at the poll interval (~30s) regardless of whether
                    // rates changed, which destabilised the plan.
                    // trigger_tx is a watch channel (latest wins) — don't overwrite
                    // an Alert/CapacityChange trigger sent above with RateChange.
                    if any_change && !signal_trigger_sent {
                        let _ = trigger_tx.send(PlanTrigger::RateChange);
                    }
                    super::backoff::record_success(&mut backoff, &state, now).await;
                    tokio::time::sleep(std::time::Duration::from_secs(secs)).await;
                }
                Err(e) => {
                    counter!("poll_error_total", "resource" => "events").increment(1);
                    error!(resource = "events", "poll failed: {e:#}");
                    vtn_ok = outage_edge(&notifier, &state, Utc::now(), vtn_ok, false).await;
                    super::backoff::record_fail_sleep(&mut backoff, &state, Utc::now(), e).await;
                }
            }
        }
    })
}
