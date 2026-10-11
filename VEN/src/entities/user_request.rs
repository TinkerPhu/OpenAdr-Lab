/// UserRequest: the user-facing representation of an energy task.
///
/// A UserRequest captures the user's intent (deadline tiers, budget) and
/// links to the generated device session (EvSession, HeaterTarget, or ShiftableLoad).
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::entities::asset::ComfortRate;
use crate::entities::design_vocabulary::UserRequestMode;
use ts_rs::TS;

/// A single deadline tier from the user's request.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct RequestDeadline {
    pub latest_end: DateTime<Utc>,
    pub max_total_cost_eur: Option<f64>,
    pub max_marginal_rate_eur_kwh: Option<f64>,
    pub min_completion: f64,
}

/// Status of the overall user request.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, TS)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum UserRequestStatus {
    Active,    // packet is scheduled or executing
    Completed, // packet reached target
    Cancelled, // user cancelled via DELETE /requests/:id
    Failed,    // packet failed
}

/// Discriminator for the linked device session type.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, TS)]
#[serde(rename_all = "snake_case")]
pub enum SessionType {
    Ev,
    Heater,
    ShiftableLoad,
}

/// A user-originated energy task request, linking to a device session.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct UserRequest {
    pub id: Uuid,
    pub asset_id: String,
    #[serde(rename = "target_soc")]
    pub target_soc_frac: Option<f64>,
    pub target_energy_kwh: f64,
    pub desired_power_kw: f64,
    pub deadlines: Vec<RequestDeadline>,
    /// How the user expressed this request (BL-28); BY_DEADLINE = legacy behaviour.
    #[serde(default)]
    pub mode: UserRequestMode,
    pub completion_policy: String,
    pub max_total_cost_eur: Option<f64>, // from first tier (for API convenience)
    pub tier_count: usize,               // number of deadline tiers
    pub session_id: Option<Uuid>, // linked DeviceSession (EvSession, HeaterTarget, or ShiftableLoad)
    #[serde(default)]
    pub session_type: Option<SessionType>,
    /// Resolved comfort/value curve (user override or asset default) — BL-34: carried through
    /// to the linked device session so the MILP planner can shape reward coefficients from it.
    #[serde(default)]
    pub comfort_rates: Vec<ComfortRate>,
    pub status: UserRequestStatus,
    pub estimated_cost_eur: f64,
    pub estimated_co2_g: f64,
    /// BL-39: real money spent so far on this session, accumulated per
    /// dispatcher tick from this request's `asset_id`'s actual import power
    /// (`controller::monitor::record_tick`) — distinct from `estimated_cost_eur`
    /// above, which is a plan-time projection. `#[serde(default)]` so old
    /// persisted requests deserialize as 0.0.
    #[serde(default)]
    pub accumulated_cost_eur: f64,
    // ── Leeway fields (§8.2 / ven_asset_interface_spec §7) ──────────────────
    pub interruptible: bool,        // planner may pause/resume this packet
    pub tolerance_min: Option<i64>, // ±N minutes around deadline acceptable
    pub budget_eur: Option<f64>,    // max the user is willing to pay (top-level ceiling)
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
/// A finished request stays listed this long, whatever the list's size.
pub const FINISHED_REQUEST_RETENTION_DAYS: i64 = 7;
/// Past that, the newest this many finished requests stay listed as history.
pub const FINISHED_REQUESTS_KEPT: usize = 20;

impl UserRequest {
    /// Completed, cancelled or failed: nothing will change it again.
    pub fn is_finished(&self) -> bool {
        self.status != UserRequestStatus::Active
    }

    /// The one way a request ends: `updated_at` is when it did, which is what
    /// `prune_finished` counts its retention from.
    pub fn finish(&mut self, status: UserRequestStatus, now: DateTime<Utc>) {
        self.status = status;
        self.updated_at = now;
    }
}

/// Drop the finished requests nobody needs listed any more (R-137): one finished more than
/// `FINISHED_REQUEST_RETENTION_DAYS` ago goes once more than `FINISHED_REQUESTS_KEPT` finished
/// requests are newer than it. An active request and a recently finished one always stay.
pub fn prune_finished(requests: &mut Vec<UserRequest>, now: DateTime<Utc>) {
    let cutoff = now - chrono::Duration::days(FINISHED_REQUEST_RETENTION_DAYS);
    let mut finished: Vec<(DateTime<Utc>, Uuid)> = requests
        .iter()
        .filter(|r| r.is_finished())
        .map(|r| (r.updated_at, r.id))
        .collect();
    // Newest first; by rank, not by timestamp, so requests finished in the same instant
    // (a displaced queue, a restart) do not all count as "the newest".
    finished.sort_unstable_by(|a, b| b.cmp(a));
    let dropped: std::collections::HashSet<Uuid> = finished
        .into_iter()
        .skip(FINISHED_REQUESTS_KEPT)
        .filter(|(finished_at, _)| *finished_at < cutoff)
        .map(|(_, id)| id)
        .collect();
    requests.retain(|r| !dropped.contains(&r.id));
}

#[cfg(test)]
impl UserRequest {
    /// The one test builder: an active request on `asset_id`, created at `at`, owning no
    /// session. A test states what it is about with struct-update syntax.
    pub(crate) fn for_test(asset_id: &str, at: DateTime<Utc>) -> Self {
        Self {
            id: Uuid::new_v4(),
            asset_id: asset_id.to_string(),
            target_soc_frac: None,
            target_energy_kwh: 0.0,
            desired_power_kw: 0.0,
            deadlines: vec![],
            mode: Default::default(),
            completion_policy: "STOP".to_string(),
            max_total_cost_eur: None,
            tier_count: 0,
            session_id: None,
            session_type: None,
            comfort_rates: vec![],
            status: UserRequestStatus::Active,
            estimated_cost_eur: 0.0,
            estimated_co2_g: 0.0,
            accumulated_cost_eur: 0.0,
            interruptible: false,
            tolerance_min: None,
            budget_eur: None,
            created_at: at,
            updated_at: at,
        }
    }
}

#[cfg(test)]
mod prune_tests {
    use super::*;
    use chrono::Duration;

    fn request(status: UserRequestStatus, finished_at: DateTime<Utc>) -> UserRequest {
        UserRequest {
            status,
            ..UserRequest::for_test("ev", finished_at)
        }
    }

    #[test]
    fn finish_stamps_when_the_request_ended() {
        let now = Utc::now();
        let mut req = request(UserRequestStatus::Active, now - Duration::days(30));
        req.finish(UserRequestStatus::Completed, now);
        assert_eq!(req.status, UserRequestStatus::Completed);
        assert_eq!(req.updated_at, now);
        assert!(req.is_finished());
    }

    #[test]
    fn prune_finished_keeps_every_request_of_the_last_week_however_many() {
        let now = Utc::now();
        let mut requests: Vec<_> = (0..FINISHED_REQUESTS_KEPT + 30)
            .map(|_| request(UserRequestStatus::Completed, now - Duration::days(6)))
            .collect();
        prune_finished(&mut requests, now);
        assert_eq!(requests.len(), FINISHED_REQUESTS_KEPT + 30);
    }

    #[test]
    fn prune_finished_keeps_old_requests_while_the_list_is_short() {
        let now = Utc::now();
        let mut requests: Vec<_> = (0..FINISHED_REQUESTS_KEPT)
            .map(|_| request(UserRequestStatus::Cancelled, now - Duration::days(400)))
            .collect();
        prune_finished(&mut requests, now);
        assert_eq!(requests.len(), FINISHED_REQUESTS_KEPT);
    }

    #[test]
    fn prune_finished_drops_the_oldest_past_the_week_once_the_list_is_long() {
        let now = Utc::now();
        // 5 of this week, 30 older ones a day apart: the newest 20 finished are the 5 and the
        // 15 youngest old ones.
        let mut requests: Vec<_> = (0..5)
            .map(|_| request(UserRequestStatus::Completed, now - Duration::days(1)))
            .collect();
        requests.extend(
            (0..30).map(|i| request(UserRequestStatus::Failed, now - Duration::days(10 + i))),
        );
        prune_finished(&mut requests, now);
        assert_eq!(requests.len(), FINISHED_REQUESTS_KEPT);
        let oldest = requests.iter().map(|r| r.updated_at).min().unwrap();
        assert_eq!(oldest, now - Duration::days(24));
    }

    #[test]
    fn prune_finished_never_drops_an_active_request() {
        let now = Utc::now();
        let mut requests: Vec<_> = (0..FINISHED_REQUESTS_KEPT + 5)
            .map(|_| request(UserRequestStatus::Completed, now - Duration::days(1)))
            .collect();
        let active = request(UserRequestStatus::Active, now - Duration::days(400));
        let active_id = active.id;
        requests.push(active);
        requests.extend(
            (0..3).map(|_| request(UserRequestStatus::Completed, now - Duration::days(90))),
        );
        prune_finished(&mut requests, now);
        assert!(requests.iter().any(|r| r.id == active_id));
        assert_eq!(requests.len(), FINISHED_REQUESTS_KEPT + 5 + 1);
    }
}
