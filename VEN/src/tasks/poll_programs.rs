// Extracted from VEN/src/loops.rs — background poll for programs

use metrics::counter;
use tracing::{info, error};
use crate::controller::VtnPort;
use crate::state::AppState;
use crate::vtn::VtnClient;

/// Spawn a background task that polls the VTN for programs.
pub(crate) fn spawn_program_poll(
    state: AppState,
    vtn: VtnClient,
    secs: u64,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(secs));
        loop {
            interval.tick().await;
            let vtn_port: &dyn VtnPort = &vtn;
            match vtn_port.fetch_programs().await {
                Ok(programs) => {
                    counter!("poll_success_total", "resource" => "programs").increment(1);
                    info!(
                        resource = "programs",
                        count = programs.len(),
                        "poll success"
                    );
                    state.set_programs(programs).await;
                }
                Err(e) => {
                    counter!("poll_error_total", "resource" => "programs").increment(1);
                    error!(resource = "programs", "poll failed: {e:#}");
                }
            }
        }
    })
}
