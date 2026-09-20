//! `AppState` accessors for wire conformance — what the VTN sent that this VEN
//! refused. Split out of `state/mod.rs` (R-40 "split proactively when next
//! touched") once that file crossed the `VEN/src/` 500-production-line cap.
//! Same split-`impl AppState` pattern as `grid_signals.rs` and `obligations.rs`.
//!
//! Written by the poll tasks through `services::notify::notify_wire_rejections`,
//! read by `GET /health`. The rule these serve: a rejection nobody can see is
//! the same failure as accepting the object silently.

use std::collections::BTreeMap;

use super::AppState;

impl AppState {
    /// Record (or clear) what the last poll of `resource` refused. `None`
    /// clears the entry, so a resource that starts parsing cleanly again stops
    /// being reported as degraded.
    pub async fn set_wire_rejections(&self, resource: &str, summary: Option<String>) {
        let mut w = self.wire_rejections.write().await;
        match summary {
            Some(s) => {
                w.insert(resource.to_string(), s);
            }
            None => {
                w.remove(resource);
            }
        }
    }

    /// What the VTN has sent that we refused, latest state per resource.
    pub async fn wire_rejections(&self) -> BTreeMap<String, String> {
        self.wire_rejections.read().await.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn set_wire_rejections_records_then_clears_per_resource() {
        let state = AppState::new();
        assert!(state.wire_rejections().await.is_empty());

        state
            .set_wire_rejections("events", Some("1 bad event".into()))
            .await;
        state
            .set_wire_rejections("programs", Some("2 bad programs".into()))
            .await;
        assert_eq!(state.wire_rejections().await.len(), 2);

        // A clean poll of one resource must not clear the other's standing state.
        state.set_wire_rejections("events", None).await;
        let left = state.wire_rejections().await;
        assert_eq!(left.len(), 1);
        assert!(left.contains_key("programs"));
    }
}
