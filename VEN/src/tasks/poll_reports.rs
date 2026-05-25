// Extracted from VEN/src/loops.rs — background poll for reports

use crate::controller::VtnPort;
use crate::state::AppState;
use metrics::counter;
use std::sync::Arc;
use tracing::{error, info};

/// Spawn a background task that polls the VTN for reports.
pub(crate) fn spawn_report_poll(
    state: AppState,
    vtn: Arc<dyn VtnPort>,
    secs: u64,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(secs));
        loop {
            interval.tick().await;
            match vtn.fetch_reports_raw().await {
                Ok(reports) => {
                    counter!("poll_success_total", "resource" => "reports").increment(1);
                    info!(resource = "reports", count = reports.len(), "poll success");
                    state.set_reports(reports).await;
                }
                Err(e) => {
                    counter!("poll_error_total", "resource" => "reports").increment(1);
                    error!(resource = "reports", "poll failed: {e:#}");
                }
            }
        }
    })
}
