//! Publishing controller decisions to the fleet channel (phase 0 §6.2).
//!
//! A bridge and nothing more: subscribe to the controller-trace broadcast,
//! hand each decision to the telemetry port. It exists so that the code that
//! *makes* decisions never has to know that anything is watching — the
//! broadcast absorbs a slow or absent subscriber, and this task absorbs a
//! broker that is away.

use std::sync::Arc;

use tokio::sync::broadcast::Receiver;

use crate::controller::telemetry_port::{trace_body, TelemetryPort};
use crate::controller::trace::ControllerEvent;

/// Forward controller decisions to the broker until the process ends.
///
/// Does nothing at all when this VEN does not publish: a no-op port would
/// still cost a serialisation per decision, and "not configured for a fleet"
/// is the normal state of a standalone VEN.
pub fn spawn(
    mut rx: Receiver<ControllerEvent>,
    telemetry: Arc<dyn TelemetryPort>,
    ven_name: String,
) -> Option<tokio::task::JoinHandle<()>> {
    if !telemetry.publishes() {
        return None;
    }
    Some(tokio::spawn(async move {
        loop {
            match rx.recv().await {
                Ok(event) => telemetry.publish_trace(trace_body(&ven_name, &event)).await,
                // Lagged means decisions were made faster than they were
                // published. Say how many were lost rather than pretend the
                // stream is complete -- a reaction chain with a silent hole in
                // it is worse than one that admits the hole.
                Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                    tracing::warn!(missed = n, "fleet trace publisher fell behind");
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::AppState;
    use async_trait::async_trait;
    use std::sync::Mutex;

    #[derive(Default)]
    struct Recorder {
        published: Mutex<Vec<serde_json::Value>>,
        publishes: bool,
    }

    #[async_trait]
    impl TelemetryPort for Recorder {
        async fn publish_telemetry(&self, _body: serde_json::Value) {}
        async fn publish_trace(&self, body: serde_json::Value) {
            self.published.lock().unwrap().push(body);
        }
        fn is_connected(&self) -> bool {
            true
        }
        fn publishes(&self) -> bool {
            self.publishes
        }
    }

    fn arrived(id: &str) -> ControllerEvent {
        ControllerEvent::OpenAdrArrived {
            ts: chrono::Utc::now(),
            event_id: id.into(),
            modification_date_time: None,
            event_name: "Peak DR".into(),
            signal_type: "PRICE".into(),
            value: 0.3,
            interval: 1,
        }
    }

    #[tokio::test]
    async fn publishes_every_controller_decision_with_its_ven_name() {
        let state = AppState::new();
        let port = Arc::new(Recorder {
            publishes: true,
            ..Default::default()
        });
        spawn(
            state.subscribe_controller_trace(),
            port.clone(),
            "ven-7".into(),
        )
        .expect("publisher runs");

        state.push_controller_event(arrived("ev-1")).await;
        state.push_controller_event(arrived("ev-2")).await;
        tokio::task::yield_now().await;
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        let published = port.published.lock().unwrap();
        assert_eq!(published.len(), 2);
        assert_eq!(published[0]["event_id"], "ev-1");
        assert_eq!(published[1]["venName"], "ven-7");
    }

    /// A VEN outside a monitored fleet does not serialise decisions nobody
    /// will read.
    #[tokio::test]
    async fn does_not_run_when_this_ven_does_not_publish() {
        let state = AppState::new();
        let port = Arc::new(Recorder::default());
        assert!(spawn(state.subscribe_controller_trace(), port, "ven-7".into()).is_none());
    }
}
