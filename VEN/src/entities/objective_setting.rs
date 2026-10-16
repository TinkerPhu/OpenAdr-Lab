//! The planner objective in force on this VEN and where it came from (openspec
//! `opportunistic-objective` D9): the profile at start-up, the VEN UI, or a fleet command from
//! the VTN UI. Owns the rule for changing it (the latest change wins). Holds no clock and
//! triggers nothing: `services::objective` applies a change and replans.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::entities::planner_params::PlannerObjective;

/// Who set the objective in force.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum ObjectiveSource {
    /// The profile's `planner.objective`, at start-up or after fleet control was released.
    Profile,
    /// `PUT /plan/objective`, the VEN UI's Planner page.
    VenUi,
    /// A fleet command on the lab broker, sent from the VTN UI.
    Fleet,
}

/// The objective in force, its source and when it was set, plus what a reader needs to change
/// it: the profile's objective (what releasing fleet control returns to) and the objectives this
/// VEN accepts. Served by `GET /plan/objective` and, with the same body, on the lab broker.
#[derive(Debug, Clone, PartialEq, Serialize, TS)]
pub struct ObjectiveSetting {
    pub objective: PlannerObjective,
    pub source: ObjectiveSource,
    /// `None` until anything but the profile set it.
    pub set_at: Option<DateTime<Utc>>,
    pub profile_objective: PlannerObjective,
    pub available_objectives: Vec<PlannerObjective>,
}

impl ObjectiveSetting {
    /// The setting at start-up: the profile's objective.
    pub fn from_profile(profile_objective: PlannerObjective) -> Self {
        Self {
            objective: profile_objective,
            source: ObjectiveSource::Profile,
            set_at: None,
            profile_objective,
            available_objectives: PlannerObjective::ALL.to_vec(),
        }
    }

    /// Apply a change; the latest one wins, whoever made it.
    pub fn set(
        &mut self,
        objective: PlannerObjective,
        source: ObjectiveSource,
        now: DateTime<Utc>,
    ) {
        self.objective = objective;
        self.source = source;
        self.set_at = Some(now);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn t(min: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 16, 12, min, 0).unwrap()
    }

    #[test]
    fn from_profile_reports_the_profile_and_every_objective() {
        let s = ObjectiveSetting::from_profile(PlannerObjective::MinCost);
        assert_eq!(s.objective, PlannerObjective::MinCost);
        assert_eq!(s.source, ObjectiveSource::Profile);
        assert_eq!(s.set_at, None);
        assert_eq!(s.available_objectives, PlannerObjective::ALL.to_vec());
    }

    #[test]
    fn the_latest_change_wins_whoever_made_it() {
        let mut s = ObjectiveSetting::from_profile(PlannerObjective::MinCost);
        s.set(
            PlannerObjective::Opportunistic,
            ObjectiveSource::Fleet,
            t(0),
        );
        s.set(PlannerObjective::MinGhg, ObjectiveSource::VenUi, t(1));
        assert_eq!(
            (s.objective, s.source, s.set_at),
            (PlannerObjective::MinGhg, ObjectiveSource::VenUi, Some(t(1)))
        );
    }
}
