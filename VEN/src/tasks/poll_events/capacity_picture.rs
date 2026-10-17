//! What one event poll reads about capacity, for change detection (GB-48).
//!
//! Split out of `detect.rs` to keep it under the `tasks/` 200-production-line cap. It owns
//! one decision: when two polls count as "the same capacity". It reads the VTN's schedule
//! only, through the same view `GET /capacity` serves (`OadrCapacityState::at`), and must
//! never hold a capacity value of its own beyond the picture it compares.

use chrono::{DateTime, Utc};

use crate::entities::capacity::{CapacitySnapshot, OadrCapacityState};
use crate::entities::sim_inject::SimInjectState;

/// What this VEN reads about capacity after one poll: the limits, reservations and
/// subscriptions in force at the poll's time, and the capacity schedule. Two polls with
/// equal pictures leave the planner's capacity inputs unchanged. The poll's own timestamp
/// is left out, or every poll would differ. A limit that starts or ends between two polls
/// of an unchanged event list changes the picture, because "in force" is read at the
/// poll's `now`.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct CapacityPicture {
    in_force: OadrCapacityState,
    schedule: Vec<CapacitySnapshot>,
}

impl CapacityPicture {
    /// The picture of a poll that parsed `schedule` at `now`: the VTN's schedule alone, with
    /// no sim-injected stand-in (a manual limit is not something the VTN announced).
    pub(super) fn at(schedule: &[CapacitySnapshot], now: DateTime<Utc>) -> Self {
        Self {
            in_force: OadrCapacityState::at(schedule, &SimInjectState::default(), now, None),
            schedule: schedule.to_vec(),
        }
    }

    /// The import and export limits in force (kW), as the `CapacityChange` trace shows them.
    pub(super) fn limits_kw(&self) -> (Option<f64>, Option<f64>) {
        (self.in_force.import_limit_kw, self.in_force.export_limit_kw)
    }
}
