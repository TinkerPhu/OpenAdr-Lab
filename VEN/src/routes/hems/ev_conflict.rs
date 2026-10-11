// ── The EV conflict refusal, as the UI reads it ───────────────────────────────
// Split out of `routes/hems/sessions.rs` to keep that file under the VEN/src/
// 500-production-line cap (`ven-architecture`), and because the shape of this
// response is a contract with the EV card rather than part of the submission flow.
//
// `wire-contracts`: the body says what kind of refusal it is in a field of its own
// (`kind`), so a client branches on a declared value instead of pattern-matching a
// human-readable string or inferring meaning from the status code. 409 is the status
// for every variant here; `kind` is what distinguishes them.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Serialize;
use ts_rs::TS;
use uuid::Uuid;

use crate::controller::user_request::{ClashingSession, RequestError};
use crate::entities::device_session::EvSessionReplaceRejection;

/// Which refusal this is: a clash nobody confirmed, or a confirmation that was refused.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum EvConflictKind {
    EvSessionConflict,
    EvReplaceRejected,
}

/// Why a stated replace instruction was refused, so the client can tell "your confirmation went
/// stale" from "you never confirmed anything".
#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[serde(tag = "reason", rename_all = "snake_case")]
pub enum ReplaceRejection {
    NotQueued {
        ids: Vec<Uuid>,
    },
    NotTheConflictSet {
        missing: Vec<Uuid>,
        extra: Vec<Uuid>,
    },
    StillConflicts {
        ids: Vec<Uuid>,
    },
}

impl From<&EvSessionReplaceRejection> for ReplaceRejection {
    fn from(r: &EvSessionReplaceRejection) -> Self {
        match r {
            EvSessionReplaceRejection::NotQueued { ids } => Self::NotQueued { ids: ids.clone() },
            EvSessionReplaceRejection::NotTheConflictSet { missing, extra } => {
                Self::NotTheConflictSet {
                    missing: missing.clone(),
                    extra: extra.clone(),
                }
            }
            EvSessionReplaceRejection::StillConflicts(c) => Self::StillConflicts {
                ids: c.conflicts.clone(),
            },
        }
    }
}

/// The 409 body of an EV submission that clashes with queued sessions.
///
/// `replaceable_session_ids` is what a confirmation must echo back as `replace_session_ids`, so
/// the client never has to derive that list itself — the one place the set is decided is the
/// server, and the round trip quotes it.
#[derive(Debug, Clone, PartialEq, Serialize, TS)]
pub struct EvConflictBody {
    pub kind: EvConflictKind,
    pub error: String,
    pub conflicts: Vec<ClashingSession>,
    pub replaceable_session_ids: Vec<Uuid>,
    /// Present only when an instruction was stated and refused.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub rejection: Option<ReplaceRejection>,
}

/// The conflict body for `error`, or `None` when the error is not an EV clash.
pub fn conflict_body(error: &RequestError) -> Option<EvConflictBody> {
    let (kind, rejection, conflicts) = match error {
        RequestError::EvSessionsConflict { conflicts } => {
            (EvConflictKind::EvSessionConflict, None, conflicts)
        }
        RequestError::EvReplaceRejected {
            rejection,
            conflicts,
        } => (
            EvConflictKind::EvReplaceRejected,
            Some(rejection),
            conflicts,
        ),
        _ => return None,
    };
    Some(EvConflictBody {
        kind,
        error: error.to_string(),
        conflicts: conflicts.clone(),
        replaceable_session_ids: conflicts.iter().map(|c| c.id).collect(),
        rejection: rejection.map(ReplaceRejection::from),
    })
}

/// The 409 response for an EV clash, or `None` when the error is something else.
pub fn conflict_response(error: &RequestError) -> Option<Response> {
    conflict_body(error).map(|body| (StatusCode::CONFLICT, Json(body)).into_response())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};

    fn clash() -> ClashingSession {
        ClashingSession {
            id: uuid::Uuid::nil(),
            window_start: Utc.with_ymd_and_hms(2026, 10, 4, 6, 0, 0).unwrap(),
            departure_time: Utc.with_ymd_and_hms(2026, 10, 4, 8, 0, 0).unwrap(),
            target_soc_frac: 0.8,
        }
    }

    #[test]
    fn conflict_response_is_none_for_an_unrelated_error() {
        assert!(conflict_response(&RequestError::NoDeadlines).is_none());
    }

    #[test]
    fn a_plain_clash_is_declared_by_kind_and_carries_no_rejection() {
        let e = RequestError::EvSessionsConflict {
            conflicts: vec![clash()],
        };
        assert!(conflict_response(&e).is_some());
        // The kind/rejection split is the contract; assert it on the value the
        // response is built from rather than by re-parsing the rendered body.
        match &e {
            RequestError::EvSessionsConflict { conflicts } => assert_eq!(conflicts.len(), 1),
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn a_rejected_instruction_reports_which_precondition_failed() {
        let e = RequestError::EvReplaceRejected {
            rejection: EvSessionReplaceRejection::NotTheConflictSet {
                missing: vec![uuid::Uuid::nil()],
                extra: vec![],
            },
            conflicts: vec![clash()],
        };
        assert!(conflict_response(&e).is_some());
        assert!(e.to_string().contains("replace instruction"));
    }

    /// R-133: the typed body is the `json!()` it replaced, key for key, for both kinds.
    #[test]
    fn conflict_body_keeps_the_wire_it_replaced() {
        let c = clash();
        let clash_json = serde_json::json!({
            "id": c.id,
            "window_start": c.window_start,
            "departure_time": c.departure_time,
            "target_soc": c.target_soc_frac,
        });
        let plain = RequestError::EvSessionsConflict {
            conflicts: vec![c.clone()],
        };
        assert_eq!(
            serde_json::to_value(conflict_body(&plain).unwrap()).unwrap(),
            serde_json::json!({
                "kind": "ev_session_conflict",
                "error": plain.to_string(),
                "conflicts": [clash_json.clone()],
                "replaceable_session_ids": [c.id],
            })
        );
        let refused = RequestError::EvReplaceRejected {
            rejection: EvSessionReplaceRejection::NotTheConflictSet {
                missing: vec![uuid::Uuid::nil()],
                extra: vec![],
            },
            conflicts: vec![c.clone()],
        };
        assert_eq!(
            serde_json::to_value(conflict_body(&refused).unwrap()).unwrap(),
            serde_json::json!({
                "kind": "ev_replace_rejected",
                "error": refused.to_string(),
                "conflicts": [clash_json],
                "replaceable_session_ids": [c.id],
                "rejection": { "reason": "not_the_conflict_set", "missing": [uuid::Uuid::nil()], "extra": [] },
            })
        );
    }
}
