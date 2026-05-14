// Obligation check task — delegates to ObligationService.

use chrono::Utc;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::error;

use crate::services::ObligationService;
use crate::simulator::SimState;
use crate::state::AppState;
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
            if let Err(e) =
                ObligationService::check_and_report(&state, &sim, &vtn, &ven_name, now).await
            {
                error!("obligation check failed: {e:#}");
            }
        }
    })
}
