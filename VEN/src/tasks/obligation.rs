// Obligation check task — delegates to ObligationService.

use chrono::{Duration, Utc};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::error;

use crate::controller::reporter::AssetReportSample;
use crate::controller::VtnPort;
use crate::services::ObligationService;
use crate::simulator::SimState;
use crate::state::AppState;

pub(crate) fn spawn_obligation_check(
    state: AppState,
    sim: Arc<Mutex<SimState>>,
    vtn: Arc<dyn VtnPort>,
    ven_name: String,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(5));
        loop {
            interval.tick().await;
            let now = Utc::now();
            let asset_samples: HashMap<String, Vec<AssetReportSample>> = {
                let sim_guard = sim.lock().await;
                sim_guard
                    .assets
                    .iter()
                    .map(|entry| {
                        let history = entry.history.slice(Duration::seconds(3600), now);
                        let samples = history
                            .iter()
                            .map(|p| AssetReportSample {
                                ts: p.ts,
                                power_kw: p.power_kw,
                                soc: p.state.soc(),
                            })
                            .collect();
                        (entry.id.clone(), samples)
                    })
                    .collect()
            };
            if let Err(e) =
                ObligationService::check_and_report(&state, asset_samples, vtn.as_ref(), &ven_name, now).await
            {
                error!("obligation check failed: {e:#}");
            }
        }
    })
}
