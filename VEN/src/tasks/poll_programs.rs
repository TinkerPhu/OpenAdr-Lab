// Extracted from VEN/src/loops.rs — background poll for programs

use crate::controller::VtnPort;
use crate::state::AppState;
use crate::tasks::backoff::Backoff;
use metrics::counter;
use std::sync::Arc;
use tracing::{error, info};

/// Spawn a background task that polls the VTN for programs.
///
/// Poll interval follows `secs` on success; on failure it backs off
/// exponentially (WP2.1, BL-03) up to `secs * 30` capped at 900s, resetting
/// to `secs` on the next success — so a healthy VTN keeps the configured
/// cadence and an unreachable one doesn't get hammered.
///
/// `startup_delay_s` (GB-09, WP2.5) is a one-time wait before the first poll
/// — the fleet generator staggers this per instance so N VENs don't all
/// poll in lockstep.
pub(crate) fn spawn_program_poll(
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
            match vtn.fetch_programs().await {
                Ok(programs) => {
                    counter!("poll_success_total", "resource" => "programs").increment(1);
                    info!(
                        resource = "programs",
                        count = programs.len(),
                        "poll success"
                    );
                    state.set_programs(programs).await;
                    backoff.on_success();
                    tokio::time::sleep(std::time::Duration::from_secs(secs)).await;
                }
                Err(e) => {
                    counter!("poll_error_total", "resource" => "programs").increment(1);
                    error!(resource = "programs", "poll failed: {e:#}");
                    tokio::time::sleep(backoff.on_failure()).await;
                }
            }
        }
    })
}
