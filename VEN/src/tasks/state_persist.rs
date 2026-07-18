// Extracted from VEN/src/loops.rs — state persistence (spawn_state_persist)

use crate::state::AppState;
use tracing::error;

pub(crate) fn spawn_state_persist(state: AppState, path: String) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(15));
        loop {
            interval.tick().await;
            match state.to_json().await {
                Ok(json) => match tokio::fs::write(&path, json).await {
                    Ok(()) => state.set_storage_ok(true).await,
                    Err(e) => {
                        error!("persist write failed: {e:#}");
                        state.set_storage_ok(false).await;
                        state
                            .record_event(
                                chrono::Utc::now(),
                                "storage",
                                format!("write failed: {e:#}"),
                            )
                            .await;
                    }
                },
                Err(e) => {
                    error!("persist serialization failed: {e:#}");
                    state.set_storage_ok(false).await;
                    state
                        .record_event(
                            chrono::Utc::now(),
                            "storage",
                            format!("serialization failed: {e:#}"),
                        )
                        .await;
                }
            }
        }
    })
}
