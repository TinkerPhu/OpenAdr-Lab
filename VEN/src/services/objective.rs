//! Changing the planner objective: the one setter every source uses (the VEN UI's
//! `PUT /plan/objective`, a fleet command from the lab broker), so each change is applied,
//! published on the fleet channel and replanned the same way (openspec `opportunistic-objective`
//! D9). The rules of the change live on `ObjectiveSetting`; this applies them, publishes the
//! result and triggers the replan. Also decides what a fleet command asks for (D10), so the
//! consumer in `tasks::fleet_command` only carries out the answer.

use chrono::{DateTime, Utc};
use lab_core::fleet_channel::ObjectiveCommand;
use tokio::sync::{watch, RwLock};

use crate::controller::telemetry_port::TelemetryPort;
use crate::entities::asset::{PlanTrigger, PlanTriggerSignal};
use crate::entities::objective_setting::{ObjectiveSetting, ObjectiveSource};
use crate::entities::planner_params::PlannerObjective;

/// Set the objective, publish the new setting on the `objective` leaf and replan at once.
pub async fn set_objective(
    setting: &RwLock<ObjectiveSetting>,
    trigger: &watch::Sender<PlanTriggerSignal>,
    telemetry: &dyn TelemetryPort,
    objective: PlannerObjective,
    source: ObjectiveSource,
    now: DateTime<Utc>,
) {
    let updated = {
        let mut s = setting.write().await;
        s.set(objective, source, now);
        s.clone()
    };
    telemetry.publish_objective(&updated);
    let _ = trigger.send(PlanTriggerSignal::bare(PlanTrigger::UserRequest));
}

/// What one `command/objective` payload asks this VEN to do.
#[derive(Debug, Clone, PartialEq)]
pub enum FleetCommandDecision {
    /// Switch to this objective, with `Fleet` as the source; remember `requested_at`.
    Apply(ObjectiveCommand<PlannerObjective>),
    /// The command was withdrawn while it was in force: back to the profile objective.
    RevertToProfile,
    /// Nothing to do, and why: the broker redelivering a command already applied, or a
    /// withdrawal of a command a later local change has already overridden.
    Ignore(&'static str),
    /// Not a command this VEN can read; logged, the setting stays as it is.
    Malformed(String),
}

/// Decide what to do with one payload from `command/objective`.
///
/// `last_applied` is the `requested_at` of the last command applied since start-up: the broker
/// redelivers the retained command after every reconnect, and that redelivery must not undo a
/// local change made since. After a restart the memory is empty, so the retained command
/// applies again. An empty payload withdraws the command, which reverts to the profile
/// objective only while the fleet's objective is still the one in force.
pub fn decide_fleet_command(
    payload: &[u8],
    last_applied: Option<DateTime<Utc>>,
    setting: &ObjectiveSetting,
) -> FleetCommandDecision {
    if payload.is_empty() {
        return if setting.source == ObjectiveSource::Fleet {
            FleetCommandDecision::RevertToProfile
        } else {
            FleetCommandDecision::Ignore("withdrawn command is no longer the one in force")
        };
    }
    match serde_json::from_slice::<ObjectiveCommand<PlannerObjective>>(payload) {
        Ok(cmd) if Some(cmd.requested_at) == last_applied => {
            FleetCommandDecision::Ignore("command already applied (broker redelivery)")
        }
        Ok(cmd) => FleetCommandDecision::Apply(cmd),
        Err(e) => FleetCommandDecision::Malformed(e.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use std::sync::Mutex;

    fn channel() -> (
        watch::Sender<PlanTriggerSignal>,
        watch::Receiver<PlanTriggerSignal>,
    ) {
        watch::channel(PlanTriggerSignal::bare(PlanTrigger::Periodic))
    }

    /// Records every objective it is asked to publish.
    #[derive(Default)]
    struct Published(Mutex<Vec<ObjectiveSetting>>);

    #[async_trait::async_trait]
    impl TelemetryPort for Published {
        async fn publish_telemetry(&self, _: crate::controller::telemetry_port::TelemetryBody<'_>) {
        }
        fn is_connected(&self) -> bool {
            true
        }
        fn publish_objective(&self, setting: &ObjectiveSetting) {
            self.0.lock().unwrap().push(setting.clone());
        }
    }

    fn t(min: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 16, 12, min, 0).unwrap()
    }

    fn command(objective: &str, at: DateTime<Utc>) -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({"objective": objective, "requested_at": at}))
            .unwrap()
    }

    fn setting_from(source: ObjectiveSource) -> ObjectiveSetting {
        let mut s = ObjectiveSetting::from_profile(PlannerObjective::MinCost);
        if source != ObjectiveSource::Profile {
            s.set(PlannerObjective::Opportunistic, source, t(0));
        }
        s
    }

    #[tokio::test]
    async fn set_objective_applies_publishes_and_replans() {
        let setting = RwLock::new(ObjectiveSetting::from_profile(PlannerObjective::MinCost));
        let (tx, mut rx) = channel();
        rx.mark_unchanged();
        let published = Published::default();
        let now = Utc::now();
        set_objective(
            &setting,
            &tx,
            &published,
            PlannerObjective::Opportunistic,
            ObjectiveSource::VenUi,
            now,
        )
        .await;
        let s = setting.read().await.clone();
        assert_eq!(
            (s.objective, s.source),
            (PlannerObjective::Opportunistic, ObjectiveSource::VenUi)
        );
        assert_eq!(*published.0.lock().unwrap(), vec![s]);
        assert!(rx.has_changed().unwrap());
        assert_eq!(rx.borrow_and_update().trigger, PlanTrigger::UserRequest);
    }

    #[test]
    fn a_new_command_is_applied() {
        let decision = decide_fleet_command(
            &command("opportunistic", t(5)),
            None,
            &setting_from(ObjectiveSource::Profile),
        );
        assert_eq!(
            decision,
            FleetCommandDecision::Apply(ObjectiveCommand {
                objective: PlannerObjective::Opportunistic,
                requested_at: t(5),
            })
        );
    }

    /// The broker resends the retained command after every reconnect; a local change made in
    /// between must survive it.
    #[test]
    fn a_redelivered_command_is_ignored() {
        let decision = decide_fleet_command(
            &command("opportunistic", t(5)),
            Some(t(5)),
            &setting_from(ObjectiveSource::VenUi),
        );
        assert!(matches!(decision, FleetCommandDecision::Ignore(_)));
    }

    #[test]
    fn a_newer_command_is_applied_after_an_older_one() {
        let decision = decide_fleet_command(
            &command("min_cost", t(9)),
            Some(t(5)),
            &setting_from(ObjectiveSource::Fleet),
        );
        assert!(matches!(decision, FleetCommandDecision::Apply(c) if c.requested_at == t(9)));
    }

    #[test]
    fn withdrawing_the_command_in_force_reverts_to_the_profile() {
        let decision = decide_fleet_command(b"", Some(t(5)), &setting_from(ObjectiveSource::Fleet));
        assert_eq!(decision, FleetCommandDecision::RevertToProfile);
    }

    #[test]
    fn withdrawing_after_a_local_change_keeps_the_local_change() {
        for source in [ObjectiveSource::VenUi, ObjectiveSource::Profile] {
            let decision = decide_fleet_command(b"", Some(t(5)), &setting_from(source));
            assert!(
                matches!(decision, FleetCommandDecision::Ignore(_)),
                "{source:?}: {decision:?}"
            );
        }
    }

    #[test]
    fn an_unreadable_command_is_malformed() {
        for payload in [
            b"not json".to_vec(),
            command("max_happiness", t(5)),
            br#"{"objective": "min_cost"}"#.to_vec(),
        ] {
            let decision =
                decide_fleet_command(&payload, None, &setting_from(ObjectiveSource::Profile));
            assert!(
                matches!(decision, FleetCommandDecision::Malformed(_)),
                "{}: {decision:?}",
                String::from_utf8_lossy(&payload)
            );
        }
    }
}
