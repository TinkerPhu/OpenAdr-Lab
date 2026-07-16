// Extracted from VEN/src/loops.rs — background poll for reports

use crate::controller::VtnPort;
use crate::state::AppState;
use crate::tasks::backoff::Backoff;
use metrics::counter;
use std::sync::Arc;
use tracing::{error, info};

/// Spawn a background task that polls the VTN for reports.
///
/// See `spawn_program_poll` for the backoff-on-failure (WP2.1, BL-03) and
/// `startup_delay_s` (GB-09, WP2.5) rationale.
pub(crate) fn spawn_report_poll(
    state: AppState,
    vtn: Arc<dyn VtnPort>,
    secs: u64,
    startup_delay_s: u64,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        if startup_delay_s > 0 {
            tokio::time::sleep(std::time::Duration::from_secs(startup_delay_s)).await;
        }
        let mut backoff = Backoff::new(secs, secs.saturating_mul(30).min(900), 0);
        loop {
            match vtn.fetch_reports().await {
                Ok(reports) => {
                    counter!("poll_success_total", "resource" => "reports").increment(1);
                    info!(resource = "reports", count = reports.len(), "poll success");
                    state.set_reports(reports).await;
                    backoff.on_success();
                    tokio::time::sleep(std::time::Duration::from_secs(secs)).await;
                }
                Err(e) => {
                    counter!("poll_error_total", "resource" => "reports").increment(1);
                    error!(resource = "reports", "poll failed: {e:#}");
                    tokio::time::sleep(backoff.on_failure()).await;
                }
            }
        }
    })
}
