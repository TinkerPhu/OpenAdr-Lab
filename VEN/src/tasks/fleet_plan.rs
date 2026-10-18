//! Publishing the adopted plan to the fleet channel (phase 0 §6.2, the `plan` topic).
//!
//! A bridge like `fleet_trace` and with the same rule: the code that plans never learns that anyone
//! is watching. It does not hook the planner. `adopt_if_warranted` pushes a `PlanCycle` onto the
//! controller trace right after it has decided whether to adopt, so on each one this reads the
//! active plan and publishes it if it is not the one already published. A cycle whose plan was
//! rejected leaves the active plan unchanged and so publishes nothing, which is the behaviour the
//! fleet view needs without this task having to know what "rejected" means.
//!
//! It never changes the plan, never waits on the broker for the planner, and is not started at all
//! for a VEN that does not publish.

use std::sync::Arc;

use tokio::sync::broadcast::error::RecvError;
use tokio::sync::broadcast::Receiver;
use uuid::Uuid;

use crate::controller::telemetry_port::{plan_body, TelemetryPort};
use crate::controller::trace::ControllerEvent;
use crate::state::AppState;

/// Publish the active plan whenever it changes, until the process ends.
pub fn spawn(
    mut rx: Receiver<ControllerEvent>,
    state: AppState,
    telemetry: Arc<dyn TelemetryPort>,
    ven_name: String,
) -> Option<tokio::task::JoinHandle<()>> {
    if !telemetry.publishes() {
        return None;
    }
    Some(tokio::spawn(async move {
        let mut published: Option<Uuid> = None;
        loop {
            match rx.recv().await {
                Ok(ControllerEvent::PlanCycle { .. }) => {}
                Ok(_) => continue,
                // A lagging subscriber may have skipped the cycle that mattered; reading the
                // active plan is idempotent, so checking costs nothing and loses nothing.
                Err(RecvError::Lagged(_)) => {}
                Err(RecvError::Closed) => break,
            }
            let Some(plan) = state.active_plan().await else {
                continue;
            };
            if published == Some(plan.id) {
                continue;
            }
            telemetry.publish_plan(plan_body(&ven_name, &plan)).await;
            published = Some(plan.id);
        }
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::controller::telemetry_port::{to_wire, PlanBody, TelemetryBody};
    use crate::services::test_support::plans::flat_plan;
    use async_trait::async_trait;
    use std::sync::Mutex;
    use std::time::Duration;

    #[derive(Default)]
    struct Recorder {
        plans: Mutex<Vec<serde_json::Value>>,
        publishes: bool,
    }

    #[async_trait]
    impl TelemetryPort for Recorder {
        async fn publish_telemetry(&self, _body: TelemetryBody<'_>) {}
        async fn publish_plan(&self, body: PlanBody<'_>) {
            let wire = serde_json::from_str(&to_wire(&body).unwrap()).unwrap();
            self.plans.lock().unwrap().push(wire);
        }
        fn is_connected(&self) -> bool {
            true
        }
        fn publishes(&self) -> bool {
            self.publishes
        }
    }

    fn cycle() -> ControllerEvent {
        ControllerEvent::PlanCycle {
            ts: chrono::Utc::now(),
            trigger_reason: "Periodic".into(),
            event_ids: vec![],
            total_slots: 4,
        }
    }

    /// What `adopt_if_warranted` does, in the order it does it: set the active plan (when
    /// adopted), then push the cycle.
    async fn cycle_that_adopts(state: &AppState, plan: crate::entities::plan::Plan) {
        state.set_active_plan(Some(plan)).await;
        state.push_controller_event(cycle()).await;
    }

    async fn settle() {
        tokio::task::yield_now().await;
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    fn running(state: &AppState) -> Arc<Recorder> {
        let port = Arc::new(Recorder {
            publishes: true,
            ..Default::default()
        });
        spawn(
            state.subscribe_controller_trace(),
            state.clone(),
            port.clone(),
            "ven-7".into(),
        )
        .expect("publisher runs");
        port
    }

    #[tokio::test]
    async fn an_adopted_plan_is_published_with_its_id_and_ven_name() {
        let state = AppState::new();
        let port = running(&state);
        let plan = flat_plan(300, 4, chrono::Utc::now());
        let id = plan.id;

        cycle_that_adopts(&state, plan).await;
        settle().await;

        let plans = port.plans.lock().unwrap();
        assert_eq!(plans.len(), 1);
        assert_eq!(plans[0]["id"], serde_json::json!(id));
        assert_eq!(plans[0]["venName"], "ven-7");
    }

    /// The gate rejected the new plan: the active plan is still the old one, which is already out.
    #[tokio::test]
    async fn a_rejected_cycle_publishes_nothing_new() {
        let state = AppState::new();
        let port = running(&state);
        cycle_that_adopts(&state, flat_plan(300, 4, chrono::Utc::now())).await;
        settle().await;

        state.push_controller_event(cycle()).await; // a cycle that adopted nothing
        settle().await;

        assert_eq!(port.plans.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn the_next_adopted_plan_replaces_it() {
        let state = AppState::new();
        let port = running(&state);
        let second = flat_plan(300, 4, chrono::Utc::now());
        let second_id = second.id;

        cycle_that_adopts(&state, flat_plan(300, 4, chrono::Utc::now())).await;
        settle().await;
        cycle_that_adopts(&state, second).await;
        settle().await;

        let plans = port.plans.lock().unwrap();
        assert_eq!(plans.len(), 2);
        assert_eq!(plans[1]["id"], serde_json::json!(second_id));
    }

    #[tokio::test]
    async fn other_controller_events_do_not_publish_a_plan() {
        let state = AppState::new();
        let port = running(&state);
        state.set_active_plan(Some(flat_plan(300, 4, chrono::Utc::now()))).await;

        state
            .push_controller_event(ControllerEvent::OpenAdrArrived {
                ts: chrono::Utc::now(),
                event_id: "ev-1".into(),
                modification_date_time: None,
                event_name: "Peak DR".into(),
                signal_type: "PRICE".into(),
                value: 0.3,
                interval: 1,
            })
            .await;
        settle().await;

        assert!(port.plans.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_cycle_before_any_plan_exists_publishes_nothing() {
        let state = AppState::new();
        let port = running(&state);
        state.push_controller_event(cycle()).await;
        settle().await;
        assert!(port.plans.lock().unwrap().is_empty());
    }

    /// A VEN outside a monitored fleet does not serialise plans nobody will read.
    #[tokio::test]
    async fn does_not_run_when_this_ven_does_not_publish() {
        let state = AppState::new();
        let port = Arc::new(Recorder::default());
        assert!(spawn(
            state.subscribe_controller_trace(),
            state.clone(),
            port,
            "ven-7".into()
        )
        .is_none());
    }
}
