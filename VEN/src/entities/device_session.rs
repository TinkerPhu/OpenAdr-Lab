use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::entities::asset::ComfortRate;
use crate::entities::design_vocabulary::UserRequestMode;

/// Who created an `EvSession` (`ev-usage-simulation`) — used only for
/// precedence, never by the MILP: a real user or VTN request always wins over
/// one the simulated usage schedule wrote, and the simulated schedule may only
/// create or refresh a session when none is active or the active one is
/// already `SimulatedUsage`-origin. No `Default` on purpose (see
/// `EvSession.origin`'s doc comment) — every construction site must say which
/// this is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum EvSessionOrigin {
    UserRequest,
    /// Historical: no producer creates it any more (R-100, a VTN SoC command is not applied).
    /// Kept so a session persisted under it still loads after an upgrade.
    Vtn,
    SimulatedUsage,
}

fn default_ev_session_origin() -> EvSessionOrigin {
    EvSessionOrigin::UserRequest
}

/// A device-centric EV charging session.
/// Only carries user intent — sim state (current_soc, plugged) is
/// injected at solve time from `SimState::ev_state()`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvSession {
    pub id: Uuid,
    /// Target SoC (0.0–1.0). E.g. 0.80 = "charge to 80%".
    #[serde(rename = "target_soc")]
    pub target_soc_frac: f64,
    /// When this session's charging window opens — the instant the vehicle
    /// becomes available for it. With `departure_time` it forms the half-open
    /// window `[window_start, departure_time)`.
    ///
    /// Before the queue existed a session's window was implicitly "from now
    /// until `departure_time`", which is only meaningful for the one session
    /// that is current. Two queued sessions cannot be checked for overlap
    /// without it, and the gap between one session's departure and the next
    /// one's start is exactly the absence a trip's charge loss belongs to.
    pub window_start: DateTime<Utc>,
    /// When the EV must be ready (departure time). Closes the window above.
    pub departure_time: DateTime<Utc>,
    /// Distance the vehicle is expected to travel after this session's departure
    /// [km]. `None` = the user did not say, and the EV's configured default is used
    /// — visibly, not silently (see `EvCharger::expected_trip_drop`).
    ///
    /// Distance rather than a state-of-charge percentage because that is what a
    /// driver knows: "about 120 km", not "38 % of my pack". Converting it needs the
    /// consumption rate and the pack size, both of which the EV owns.
    #[serde(default)]
    pub expected_trip_distance_km: Option<f64>,
    /// When the vehicle is expected back from that trip. `None` = the user did not
    /// say.
    ///
    /// Paired with `expected_trip_distance_km`: both or neither. A distance with no
    /// return time is energy with no instant to apply it to, and a return time with
    /// no distance is an instant with no energy, so the half-stated combination is
    /// refused at the route boundary rather than carried into the planner. With
    /// neither, the plan projects no drop at all and holds the state of charge flat
    /// until the real return is measured — nothing is invented on the user's behalf.
    #[serde(default)]
    pub expected_return_time: Option<DateTime<Utc>>,
    /// If true, MILP treats as MayRun (soft reward, best-effort by departure).
    /// If false (default), MustRun (hard constraint, must reach target SoC by departure).
    #[serde(default)]
    pub soft_deadline: bool,
    /// Who created this session (`ev-usage-simulation`). No `Default` impl on
    /// `EvSessionOrigin` itself, so every *Rust* construction site must name
    /// this explicitly (the compiler enforces it) — the `#[serde(default)]`
    /// below exists only so a payload from before this field existed still
    /// deserializes (as `UserRequest`, the pre-existing behavior), the same
    /// backward-compatibility pattern already used for `mode` below.
    #[serde(default = "default_ev_session_origin")]
    pub origin: EvSessionOrigin,
    /// How the user expressed this request (BL-28); BY_DEADLINE = legacy behaviour.
    #[serde(default)]
    pub mode: UserRequestMode,
    /// MAX_COST (WP4.1-c): total charging-cost ceiling [€]. None = no cap.
    #[serde(default)]
    pub budget_eur: Option<f64>,
    /// Resolved comfort/value curve carried from the linking `UserRequest` (BL-34).
    #[serde(default)]
    pub comfort_rates: Vec<ComfortRate>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl EvSession {
    /// A session the simulated usage schedule queues on the user's behalf: charge to
    /// `target_soc` between `window_start` and `departure_time`. It states no trip distance
    /// (the generated trip already carries its own consumption as a SoC percentage, so
    /// round-tripping it through kilometres would only invite the two to disagree), no
    /// budget and no comfort curve. It stands in for a user, so the queue refuses it
    /// wherever a stated session already covers the window.
    pub fn simulated(
        target_soc_frac: f64,
        window_start: DateTime<Utc>,
        departure_time: DateTime<Utc>,
        now: DateTime<Utc>,
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            target_soc_frac,
            window_start,
            departure_time,
            expected_trip_distance_km: None,
            expected_return_time: None,
            soft_deadline: false,
            mode: Default::default(),
            origin: EvSessionOrigin::SimulatedUsage,
            budget_eur: None,
            comfort_rates: vec![],
            created_at: now,
            updated_at: now,
        }
    }
}

/// Two sessions conflict when their charging windows overlap.
///
/// Carries the ids rather than a message: the follow-up change's UI has to name
/// the clashing plans back to the user, and re-deriving the overlap at that layer
/// would be a second copy of the rule this type exists to centralise.
#[derive(Debug, Clone, PartialEq)]
pub struct EvSessionConflict {
    /// The session that was refused.
    pub candidate: Uuid,
    /// Every queued session it overlaps, in window order.
    pub conflicts: Vec<Uuid>,
}

impl std::fmt::Display for EvSessionConflict {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "charging window overlaps {} queued session(s)",
            self.conflicts.len()
        )
    }
}

/// A refused insert, with the clashing sessions themselves rather than their ids.
///
/// `EvSessionQueue::insert` answers in ids, which is right for the queue: it is the
/// overlap authority and ids are all the invariant needs. But a caller that has to
/// *describe* the clash needs the plans, and resolving ids to sessions afterwards
/// means reading the queue a second time - so the account the user is shown could
/// disagree with the refusal that produced it. This type exists so the resolution
/// happens inside the same critical section that detected the clash.
// No PartialEq: it would require it on EvSession, and nothing compares two
// of these - callers read the clashing sessions, they do not equate refusals.
#[derive(Debug, Clone)]
pub struct EvSessionClash {
    /// The session that was refused.
    pub candidate: Uuid,
    /// Every queued session it overlaps, in window order.
    pub conflicts: Vec<EvSession>,
}

impl std::fmt::Display for EvSessionClash {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "charging window overlaps {} queued session(s)",
            self.conflicts.len()
        )
    }
}

/// Why a replace instruction was refused.
///
/// The instruction names the sessions the user agreed to lose, and it is checked
/// against the conflict set the queue derives *now*, not the one the refusal
/// reported earlier. The queue can move between the two (a simulated session
/// landing, another tab submitting, a plan expiring), so this is a precondition
/// in the `If-Match` sense: the confirmation must still describe the situation it
/// was given. Anything else is refused and re-prompted rather than guessed at,
/// because guessing is exactly how a standing commitment disappears unnoticed -
/// the failure the queue exists to prevent.
#[derive(Debug, Clone, PartialEq)]
pub enum EvSessionReplaceRejection {
    /// Ids that are not in the queue at all.
    NotQueued { ids: Vec<Uuid> },
    /// The instruction is not exactly the candidate's conflict set: `missing` are
    /// clashes it failed to name, `extra` are sessions it named that the candidate
    /// does not actually clash with. Both are refusals - a partial instruction
    /// would leave an overlap, and an over-broad one would delete a plan the user
    /// never needed to lose.
    NotTheConflictSet {
        missing: Vec<Uuid>,
        extra: Vec<Uuid>,
    },
    /// The candidate still clashed once the named sessions were removed. Removing
    /// every overlapping session cannot leave an overlap, so this is unreachable
    /// today; it exists so the checked `insert` stays the only authority on the
    /// invariant rather than this function assuming its result.
    StillConflicts(EvSessionConflict),
}

impl std::fmt::Display for EvSessionReplaceRejection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotQueued { ids } => {
                write!(f, "{} named session(s) are no longer queued", ids.len())
            }
            Self::NotTheConflictSet { missing, extra } => write!(
                f,
                "replace instruction names the wrong sessions ({} unnamed clash(es), {} named without clashing)",
                missing.len(),
                extra.len()
            ),
            Self::StillConflicts(c) => write!(f, "{c}"),
        }
    }
}

/// A refused replace, with the candidate's *current* conflict set in full.
///
/// Same reason as `EvSessionClash`: a rejection has to be re-prompted, and the
/// prompt must describe the queue as it is now - which is precisely what the stale
/// instruction got wrong. Carrying the conflicts with the rejection means the
/// caller never has to read the queue again to explain why it said no.
// No PartialEq: it would require it on EvSession, and nothing compares two
// of these - callers read the clashing sessions, they do not equate refusals.
#[derive(Debug, Clone)]
pub struct EvReplaceRefusal {
    pub rejection: EvSessionReplaceRejection,
    pub conflicts: Vec<EvSession>,
}

impl std::fmt::Display for EvReplaceRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.rejection)
    }
}

/// An EV's charging sessions, ordered by window start and never overlapping.
///
/// A newtype rather than a bare `Vec` because that invariant is the whole point:
/// three producers write this queue (a user request, a VTN charge signal, and the
/// simulated usage schedule), and a bare `Vec` would leave each of them free to
/// insert at the wrong index or on top of an overlap. The only way in is the
/// checked `insert`, which makes an overlapping queue unrepresentable rather than
/// merely untested.
///
/// Windows are half-open: one session's `departure_time` may equal the next
/// one's `window_start` without conflicting, which is the natural encoding of
/// "the car leaves and comes back" and matches `ev_schedule::active_trip_at`'s
/// existing `ts >= leave_at && ts < return_at` convention.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(transparent)]
pub struct EvSessionQueue(Vec<EvSession>);

impl EvSessionQueue {
    /// Do these two windows overlap? The one definition of the rule.
    fn overlaps(a: &EvSession, b: &EvSession) -> bool {
        a.window_start < b.departure_time && b.window_start < a.departure_time
    }

    /// Every queued session `candidate` would overlap, in window order.
    ///
    /// A session already in the queue never conflicts with itself, so re-checking
    /// one that is being replaced reports only the others.
    pub fn conflicts(&self, candidate: &EvSession) -> Vec<Uuid> {
        self.0
            .iter()
            .filter(|s| s.id != candidate.id && Self::overlaps(s, candidate))
            .map(|s| s.id)
            .collect()
    }

    /// Insert, keeping the queue ordered by window start and free of overlaps.
    ///
    /// What to *do* about a conflict is the producer's policy, not the queue's:
    /// the simulated schedule skips, the VTN replaces the session it owns by id,
    /// and the user-facing path prompts. Keeping that decision out of here is what
    /// lets one `insert` serve all three.
    pub fn insert(&mut self, session: EvSession) -> Result<(), EvSessionConflict> {
        let conflicts = self.conflicts(&session);
        if !conflicts.is_empty() {
            return Err(EvSessionConflict {
                candidate: session.id,
                conflicts,
            });
        }
        let at = self
            .0
            .partition_point(|s| s.window_start <= session.window_start);
        self.0.insert(at, session);
        Ok(())
    }

    /// Displace exactly the sessions named by `replace_ids` and queue `session`.
    ///
    /// The whole operation is all-or-nothing: on any refusal the queue is left
    /// untouched, and the insertion is still the checked `insert` above, so this
    /// function adds a precondition and never a second copy of the overlap rule.
    /// `replace_ids` must name exactly the candidate's current conflict set - see
    /// `EvSessionReplaceRejection` for why naming fewer or more is refused rather
    /// than reconciled.
    ///
    /// Returns the displaced sessions, so a caller can report what it removed.
    pub fn replace(
        &mut self,
        replace_ids: &[Uuid],
        session: EvSession,
    ) -> Result<Vec<EvSession>, EvSessionReplaceRejection> {
        let not_queued: Vec<Uuid> = replace_ids
            .iter()
            .copied()
            .filter(|id| !self.0.iter().any(|s| s.id == *id))
            .collect();
        if !not_queued.is_empty() {
            return Err(EvSessionReplaceRejection::NotQueued { ids: not_queued });
        }

        let clashing = self.conflicts(&session);
        let missing: Vec<Uuid> = clashing
            .iter()
            .copied()
            .filter(|id| !replace_ids.contains(id))
            .collect();
        let extra: Vec<Uuid> = replace_ids
            .iter()
            .copied()
            .filter(|id| !clashing.contains(id))
            .collect();
        if !missing.is_empty() || !extra.is_empty() {
            return Err(EvSessionReplaceRejection::NotTheConflictSet { missing, extra });
        }

        let removed: Vec<EvSession> = replace_ids
            .iter()
            .filter_map(|id| self.remove(*id))
            .collect();
        match self.insert(session) {
            Ok(()) => Ok(removed),
            Err(conflict) => {
                // Put back what was taken: a refused replace must change nothing.
                for s in removed {
                    let _ = self.insert(s);
                }
                Err(EvSessionReplaceRejection::StillConflicts(conflict))
            }
        }
    }

    /// Remove one session by id, returning it when it was queued.
    pub fn remove(&mut self, id: Uuid) -> Option<EvSession> {
        let at = self.0.iter().position(|s| s.id == id)?;
        Some(self.0.remove(at))
    }

    /// The session whose window contains `now`, if any.
    ///
    /// This is what "a session is active" means. Not "the queue is non-empty":
    /// under a rolling schedule the queue is almost never empty, and treating
    /// that as active would pause opportunistic charging permanently.
    pub fn current(&self, now: DateTime<Utc>) -> Option<&EvSession> {
        self.0
            .iter()
            .find(|s| s.window_start <= now && now < s.departure_time)
    }

    /// The session this plan is for: the earliest one that has not departed yet.
    ///
    /// Distinct from `current` on purpose. `current` answers "is a window open right
    /// now", which is what the live tick and the opportunistic-charging pause need.
    /// A *planner* asking the same question gets the wrong answer, because GB-54
    /// aligns a replan's `now` to the slot grid, so it can sit before wall-clock
    /// time: a session created at 14:20 against a plan starting at 14:00 is not yet
    /// "current", and treating it as absent dropped its flexibility envelope from
    /// the plan entirely. What a plan wants is the next commitment it must serve,
    /// whether its window has opened yet or not.
    pub fn upcoming(&self, now: DateTime<Utc>) -> Option<&EvSession> {
        self.0.iter().find(|s| s.departure_time > now)
    }

    /// Drop every session whose departure has passed; returns how many went.
    ///
    /// Nothing else expires a session, so a finished or missed one would
    /// otherwise keep acting as an obligation forever.
    pub fn expire(&mut self, now: DateTime<Utc>) -> usize {
        let before = self.0.len();
        self.0.retain(|s| s.departure_time > now);
        before - self.0.len()
    }

    /// The queued sessions in window order, for callers that take a slice.
    pub fn as_slice(&self) -> &[EvSession] {
        &self.0
    }

    pub fn iter(&self) -> impl Iterator<Item = &EvSession> {
        self.0.iter()
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// A device-centric heater temperature target.
/// Only carries user intent — sim state (current_temp_c) is
/// injected at solve time from `SimState::heater_state()`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeaterTarget {
    pub id: Uuid,
    /// Desired water/room temperature in °C.
    pub target_temp_c: f64,
    /// When the target temperature must be reached.
    pub ready_by: DateTime<Utc>,
    /// How the user expressed this request (BL-28); BY_DEADLINE = legacy behaviour.
    #[serde(default)]
    pub mode: UserRequestMode,
    /// Resolved comfort/value curve carried from the linking `UserRequest` (BL-34).
    #[serde(default)]
    pub comfort_rates: Vec<ComfortRate>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// A shiftable load (e.g. washing machine, heat pump cycle).
///
/// Fixed power level for a fixed duration; the MILP chooses optimal
/// start time within `[earliest_start, latest_end - duration]`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShiftableLoad {
    pub id: Uuid,
    /// Asset identifier (e.g. "wm", "hp").
    pub asset_id: String,
    /// Fixed power level while running [kW].
    pub power_kw: f64,
    /// Total run time [minutes].
    pub duration_min: u32,
    /// Earliest allowed start time.
    pub earliest_start: DateTime<Utc>,
    /// Latest allowed end time (load must finish by this time).
    pub latest_end: DateTime<Utc>,
    /// How the user expressed this request (BL-28); BY_DEADLINE = legacy behaviour.
    #[serde(default)]
    pub mode: UserRequestMode,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entities::design_vocabulary::UserRequestMode;

    /// Payloads from before the mode field existed must default to BY_DEADLINE
    /// (today's implicit behaviour).
    #[test]
    fn test_ev_session_deserialize_missing_mode_defaults_by_deadline() {
        let json = r#"{
            "id": "3fa85f64-5717-4562-b3fc-2c963f66afa6",
            "target_soc": 0.8,
            "window_start": "2026-07-11T20:00:00Z",
            "departure_time": "2026-07-12T06:00:00Z",
            "created_at": "2026-07-11T20:00:00Z",
            "updated_at": "2026-07-11T20:00:00Z"
        }"#;
        let s: EvSession = serde_json::from_str(json).unwrap();
        assert_eq!(s.mode, UserRequestMode::ByDeadline);
    }

    /// Mode survives a serde roundtrip in SCREAMING_SNAKE_CASE wire form.
    #[test]
    fn test_ev_session_serde_roundtrip_preserves_mode() {
        let session = EvSession {
            id: Uuid::new_v4(),
            target_soc_frac: 0.9,
            window_start: Utc::now(),
            expected_trip_distance_km: None,
            expected_return_time: None,
            departure_time: Utc::now(),
            soft_deadline: false,
            mode: UserRequestMode::Opportunistic,
            origin: EvSessionOrigin::UserRequest,
            budget_eur: None,
            comfort_rates: vec![],
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        let json = serde_json::to_string(&session).unwrap();
        assert!(json.contains("\"OPPORTUNISTIC\""));
        let back: EvSession = serde_json::from_str(&json).unwrap();
        assert_eq!(back.mode, UserRequestMode::Opportunistic);
    }

    #[test]
    fn test_heater_target_deserialize_missing_mode_defaults_by_deadline() {
        let json = r#"{
            "id": "3fa85f64-5717-4562-b3fc-2c963f66afa6",
            "target_temp_c": 55.0,
            "ready_by": "2026-07-12T06:00:00Z",
            "created_at": "2026-07-11T20:00:00Z",
            "updated_at": "2026-07-11T20:00:00Z"
        }"#;
        let t: HeaterTarget = serde_json::from_str(json).unwrap();
        assert_eq!(t.mode, UserRequestMode::ByDeadline);
    }

    #[test]
    fn test_shiftable_load_deserialize_missing_mode_defaults_by_deadline() {
        let json = r#"{
            "id": "3fa85f64-5717-4562-b3fc-2c963f66afa6",
            "asset_id": "wm",
            "power_kw": 2.0,
            "duration_min": 60,
            "earliest_start": "2026-07-11T20:00:00Z",
            "latest_end": "2026-07-12T06:00:00Z",
            "created_at": "2026-07-11T20:00:00Z",
            "updated_at": "2026-07-11T20:00:00Z"
        }"#;
        let l: ShiftableLoad = serde_json::from_str(json).unwrap();
        assert_eq!(l.mode, UserRequestMode::ByDeadline);
    }

    // ── EvSessionQueue: the ordered, non-overlapping invariant ──────────────

    fn ts(h: i64) -> DateTime<Utc> {
        chrono::TimeZone::with_ymd_and_hms(&Utc, 2026, 7, 20, 0, 0, 0).unwrap()
            + chrono::Duration::hours(h)
    }

    /// A session occupying `[from, to)`, firm, user-created.
    fn sess(from: i64, to: i64) -> EvSession {
        EvSession {
            id: Uuid::new_v4(),
            target_soc_frac: 0.8,
            window_start: ts(from),
            expected_trip_distance_km: None,
            expected_return_time: None,
            departure_time: ts(to),
            soft_deadline: false,
            origin: EvSessionOrigin::UserRequest,
            mode: UserRequestMode::default(),
            budget_eur: None,
            comfort_rates: vec![],
            created_at: ts(0),
            updated_at: ts(0),
        }
    }

    #[test]
    fn conflicts_reports_an_overlapping_session() {
        let mut q = EvSessionQueue::default();
        let a = sess(0, 8);
        let a_id = a.id;
        q.insert(a).expect("first insert cannot conflict");
        assert_eq!(q.conflicts(&sess(4, 12)), vec![a_id]);
    }

    #[test]
    fn conflicts_allows_touching_windows() {
        // One session's departure is the next one's window start: the car leaves
        // and comes back. Half-open windows make this a non-conflict.
        let mut q = EvSessionQueue::default();
        q.insert(sess(0, 8)).unwrap();
        assert!(q.conflicts(&sess(8, 16)).is_empty());
        assert!(q.insert(sess(8, 16)).is_ok());
        assert_eq!(q.len(), 2);
    }

    #[test]
    fn insert_orders_by_window_start() {
        let mut q = EvSessionQueue::default();
        q.insert(sess(16, 20)).unwrap();
        q.insert(sess(0, 4)).unwrap();
        q.insert(sess(8, 12)).unwrap();
        let starts: Vec<_> = q.iter().map(|s| s.window_start).collect();
        assert_eq!(
            starts,
            vec![ts(0), ts(8), ts(16)],
            "ordered by window start"
        );
    }

    #[test]
    fn insert_rejects_an_overlap_and_names_every_clash() {
        let mut q = EvSessionQueue::default();
        let a = sess(0, 6);
        let b = sess(6, 12);
        let (a_id, b_id) = (a.id, b.id);
        q.insert(a).unwrap();
        q.insert(b).unwrap();

        // Spans both.
        let spanning = sess(3, 9);
        let err = q.insert(spanning).expect_err("an overlap must be refused");
        assert_eq!(err.conflicts, vec![a_id, b_id]);
        assert_eq!(q.len(), 2, "a refused insert must change nothing");
    }

    #[test]
    fn current_is_the_session_whose_window_contains_now() {
        let mut q = EvSessionQueue::default();
        let a = sess(0, 8);
        let a_id = a.id;
        q.insert(a).unwrap();
        q.insert(sess(10, 18)).unwrap();

        assert_eq!(q.current(ts(4)).map(|s| s.id), Some(a_id));
        // In the gap between two sessions nothing is current, even though the
        // queue is not empty.
        assert!(q.current(ts(9)).is_none());
        // The departure instant itself is outside the half-open window.
        assert!(q.current(ts(8)).map(|s| s.id) != Some(a_id));
    }

    /// The grid-lag case that cost a plan its flexibility envelope: GB-54 aligns a
    /// replan's `now` to the slot grid, so it can sit *before* wall-clock time. A
    /// session created at 14:20 against a plan whose `now` is 14:00 is not yet
    /// "current", and treating that as "no session" dropped it from the plan.
    #[test]
    fn upcoming_finds_a_session_whose_window_has_not_opened_yet() {
        let mut q = EvSessionQueue::default();
        let later = sess(2, 10);
        let later_id = later.id;
        q.insert(later).unwrap();

        // Grid-aligned plan time sits before the session's window start.
        assert!(q.current(ts(0)).is_none(), "its window has not opened");
        assert_eq!(
            q.upcoming(ts(0)).map(|s| s.id),
            Some(later_id),
            "but it is the commitment this plan must serve"
        );
    }

    #[test]
    fn upcoming_skips_a_session_that_has_already_departed() {
        let mut q = EvSessionQueue::default();
        q.insert(sess(-8, -2)).unwrap();
        let live = sess(4, 12);
        let live_id = live.id;
        q.insert(live).unwrap();

        assert_eq!(q.upcoming(ts(0)).map(|s| s.id), Some(live_id));
    }

    #[test]
    fn expire_drops_every_passed_session() {
        let mut q = EvSessionQueue::default();
        q.insert(sess(0, 4)).unwrap();
        q.insert(sess(4, 8)).unwrap();
        let live = sess(12, 20);
        let live_id = live.id;
        q.insert(live).unwrap();

        assert_eq!(q.expire(ts(10)), 2, "both passed sessions go at once");
        assert_eq!(q.len(), 1);
        assert_eq!(q.iter().next().unwrap().id, live_id);
    }

    #[test]
    fn remove_takes_only_the_named_session() {
        let mut q = EvSessionQueue::default();
        let a = sess(0, 4);
        let b = sess(4, 8);
        let c = sess(8, 12);
        let (a_id, b_id, c_id) = (a.id, b.id, c.id);
        q.insert(a).unwrap();
        q.insert(b).unwrap();
        q.insert(c).unwrap();

        assert_eq!(q.remove(b_id).map(|s| s.id), Some(b_id));
        let left: Vec<_> = q.iter().map(|s| s.id).collect();
        assert_eq!(left, vec![a_id, c_id]);
        assert!(q.remove(b_id).is_none(), "removing twice is not an error");
    }

    /// The invariant must survive arbitrary interleavings, not just the orders a
    /// hand-written test happens to try: three producers write this queue.
    #[test]
    fn the_invariant_holds_under_interleaved_inserts_removes_and_expiries() {
        let mut q = EvSessionQueue::default();
        let mut ids = Vec::new();

        // A deterministic but irregular interleaving: windows that touch, that
        // overlap, and that sit far apart, with removals and expiries between.
        for step in 0..40i64 {
            let from = (step * 7) % 23;
            let cand = sess(from, from + 3 + (step % 4));
            if let Ok(()) = q.insert(cand.clone()) {
                ids.push(cand.id);
            }
            if step % 5 == 4 && !ids.is_empty() {
                let victim = ids.remove(0);
                q.remove(victim);
            }
            if step % 11 == 10 {
                q.expire(ts(step % 23));
            }

            // Ordered by window start, and no two windows overlap.
            let w: Vec<_> = q
                .iter()
                .map(|s| (s.window_start, s.departure_time))
                .collect();
            for pair in w.windows(2) {
                assert!(pair[0].0 <= pair[1].0, "queue must stay ordered: {w:?}");
                assert!(
                    pair[0].1 <= pair[1].0,
                    "queued windows must never overlap: {w:?}"
                );
            }
        }
    }

    // ── replace: the one-click "replace that one?" path (050) ────────────────

    #[test]
    fn replace_displaces_exactly_the_named_conflict_and_queues_the_candidate() {
        let mut q = EvSessionQueue::default();
        let standing = sess(0, 6);
        let standing_id = standing.id;
        q.insert(standing).unwrap();
        q.insert(sess(12, 18)).unwrap();

        let spontaneous = sess(3, 9);
        let spontaneous_id = spontaneous.id;
        let removed = q
            .replace(&[standing_id], spontaneous)
            .expect("naming exactly the conflict set is accepted");

        assert_eq!(
            removed.iter().map(|s| s.id).collect::<Vec<_>>(),
            vec![standing_id]
        );
        let ids: Vec<_> = q.iter().map(|s| s.id).collect();
        assert!(ids.contains(&spontaneous_id), "the candidate is queued");
        assert!(!ids.contains(&standing_id), "the named session is gone");
        assert_eq!(q.len(), 2, "the untouched session stays");
    }

    #[test]
    fn replace_refuses_a_partial_instruction_and_removes_nothing() {
        let mut q = EvSessionQueue::default();
        let a = sess(0, 6);
        let b = sess(6, 12);
        let (a_id, b_id) = (a.id, b.id);
        q.insert(a).unwrap();
        q.insert(b).unwrap();

        // Spans both, but names only one.
        let err = q
            .replace(&[a_id], sess(3, 9))
            .expect_err("a partial instruction must be refused");
        assert_eq!(
            err,
            EvSessionReplaceRejection::NotTheConflictSet {
                missing: vec![b_id],
                extra: vec![],
            }
        );
        assert_eq!(q.len(), 2, "a refused replace must remove nothing");
    }

    #[test]
    fn replace_refuses_naming_a_session_it_does_not_clash_with() {
        let mut q = EvSessionQueue::default();
        let clashing = sess(0, 6);
        let innocent = sess(12, 18);
        let (clashing_id, innocent_id) = (clashing.id, innocent.id);
        q.insert(clashing).unwrap();
        q.insert(innocent).unwrap();

        let err = q
            .replace(&[clashing_id, innocent_id], sess(3, 9))
            .expect_err("naming a non-conflicting session must be refused");
        assert_eq!(
            err,
            EvSessionReplaceRejection::NotTheConflictSet {
                missing: vec![],
                extra: vec![innocent_id],
            }
        );
        assert_eq!(q.len(), 2, "a refused replace must remove nothing");
    }

    #[test]
    fn replace_refuses_an_id_that_is_no_longer_queued() {
        let mut q = EvSessionQueue::default();
        let standing = sess(0, 6);
        let standing_id = standing.id;
        q.insert(standing).unwrap();
        let stale = uuid::Uuid::new_v4();

        let err = q
            .replace(&[standing_id, stale], sess(3, 9))
            .expect_err("a stale id must be refused");
        assert_eq!(
            err,
            EvSessionReplaceRejection::NotQueued { ids: vec![stale] }
        );
        assert_eq!(q.len(), 1, "a refused replace must remove nothing");
    }

    #[test]
    fn replace_displaces_several_clashes_all_or_nothing() {
        let mut q = EvSessionQueue::default();
        let a = sess(0, 6);
        let b = sess(6, 12);
        let (a_id, b_id) = (a.id, b.id);
        q.insert(a).unwrap();
        q.insert(b).unwrap();

        let spanning = sess(3, 9);
        let spanning_id = spanning.id;
        let removed = q.replace(&[a_id, b_id], spanning).expect("both named");
        assert_eq!(removed.len(), 2);
        assert_eq!(
            q.iter().map(|s| s.id).collect::<Vec<_>>(),
            vec![spanning_id]
        );
    }

    #[test]
    fn replace_with_no_conflicts_and_no_names_is_a_plain_insert() {
        let mut q = EvSessionQueue::default();
        q.insert(sess(0, 6)).unwrap();
        let later = sess(12, 18);
        let later_id = later.id;
        let removed = q.replace(&[], later).expect("nothing clashes");
        assert!(removed.is_empty());
        assert_eq!(q.len(), 2);
        assert!(q.iter().any(|s| s.id == later_id));
    }
}

/// A single slot in a baseline override.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BaselineSlot {
    /// Start time of the slot (aligned to planning grid).
    pub slot_start: DateTime<Utc>,
    /// Additive power adjustment [kW]. Positive = more load.
    pub add_kw: f64,
}

/// User-specified additive adjustments to the non-controllable baseline.
///
/// E.g. "I know the dishwasher will run 1.5 kW from 14:00–15:00".
/// Applied in `build_milp_inputs()` as `p_base_kw[t] += add_kw`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BaselineOverride {
    pub id: Uuid,
    pub slots: Vec<BaselineSlot>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
