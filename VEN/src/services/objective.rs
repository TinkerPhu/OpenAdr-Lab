//! Changing the planner objective: the one setter every source uses (the VEN UI's
//! `PUT /plan/objective`, a fleet command from the lab broker), so each change is applied and
//! replanned the same way (openspec `opportunistic-objective` D9). The rules of the change live
//! on `ObjectiveSetting`; this applies them and triggers the replan.

use chrono::{DateTime, Utc};
use tokio::sync::{watch, RwLock};

use crate::entities::asset::{PlanTrigger, PlanTriggerSignal};
use crate::entities::objective_setting::{ObjectiveSetting, ObjectiveSource};
use crate::entities::planner_params::PlannerObjective;

/// Set the objective and replan at once.
pub async fn set_objective(
    setting: &RwLock<ObjectiveSetting>,
    trigger: &watch::Sender<PlanTriggerSignal>,
    objective: PlannerObjective,
    source: ObjectiveSource,
    now: DateTime<Utc>,
) {
    setting.write().await.set(objective, source, now);
    let _ = trigger.send(PlanTriggerSignal::bare(PlanTrigger::UserRequest));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn channel() -> (
        watch::Sender<PlanTriggerSignal>,
        watch::Receiver<PlanTriggerSignal>,
    ) {
        watch::channel(PlanTriggerSignal::bare(PlanTrigger::Periodic))
    }

    #[tokio::test]
    async fn set_objective_applies_the_change_and_replans() {
        let setting = RwLock::new(ObjectiveSetting::from_profile(PlannerObjective::MinCost));
        let (tx, mut rx) = channel();
        rx.mark_unchanged();
        let now = Utc::now();
        set_objective(
            &setting,
            &tx,
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
        assert!(rx.has_changed().unwrap());
        assert_eq!(rx.borrow_and_update().trigger, PlanTrigger::UserRequest);
    }
}
