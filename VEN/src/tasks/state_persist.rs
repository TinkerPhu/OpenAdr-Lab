// Extracted from VEN/src/loops.rs — state persistence (spawn_state_persist)

use tracing::error;
use crate::state::AppState;

pub(crate) fn spawn_state_persist(state: AppState, path: String) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(15));
        loop {
            interval.tick().await;
            match state.to_json().await {
                Ok(json) => {
                    if let Err(e) = tokio::fs::write(&path, json).await {
                        error!("persist write failed: {e:#}");
                    }
                }
                Err(e) => error!("persist serialization failed: {e:#}"),
            }
        }
    })
}
