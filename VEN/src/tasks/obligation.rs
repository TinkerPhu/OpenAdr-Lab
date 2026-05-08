// Extracted from VEN/src/loops.rs — obligation check task

use chrono::Utc;
use tracing::{info, error, debug};
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::controller;
use crate::state::AppState;
use crate::simulator::SimState;
use crate::vtn::VtnClient;

pub(crate) fn spawn_obligation_check(
    state: AppState,
    sim: Arc<Mutex<SimState>>,
    vtn: VtnClient,
    ven_name: String,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(5));
        loop {
            interval.tick().await;
            let now = Utc::now();
            let due = state.due_obligations(now).await;
            for ob in due {
                let env = state.site_envelope().await;
                let report_opt = {
                    let sim_guard = sim.lock().await;
                    controller::reporter::build_measurement_report_for_obligation(
                        &ob,
                        &*sim_guard,
                        &ven_name,
                        env.as_ref(),
                    )
                };
                if let Some(report) = report_opt {
                    match vtn.upsert_report(report).await {
                        Ok(_) => {
                            state.mark_obligation_fulfilled(ob.id).await;
                            info!(
                                obligation_id = %ob.id,
                                payload_type = %ob.payload_type,
                                "obligation report submitted"
                            );
                        }
                        Err(e) => {
                            error!(
                                obligation_id = %ob.id,
                                "obligation report submission failed: {e:#}"
                            );
                        }
                    }
                } else {
                    // No history data to build report — mark fulfilled to avoid retry loop
                    state.mark_obligation_fulfilled(ob.id).await;
                    debug!(
                        obligation_id = %ob.id,
                        "obligation skipped (no history data)"
                    );
                }
            }
        }
    })
}
