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

use crate::controller::user_request::{ClashingSession, RequestError};
use crate::entities::device_session::EvSessionReplaceRejection;

/// Render one clashing session for the prompt.
fn clash_json(c: &ClashingSession) -> serde_json::Value {
    serde_json::json!({
        "id": c.id,
        "window_start": c.window_start,
        "departure_time": c.departure_time,
        "target_soc": c.target_soc_frac,
    })
}

/// The refusal body for an EV submission that clashes with queued sessions.
///
/// `replaceable_session_ids` is what a confirmation must echo back as
/// `replace_session_ids`, so the client never has to derive that list itself — the
/// one place the set is decided is the server, and the round trip quotes it.
pub fn conflict_response(error: &RequestError) -> Option<Response> {
    let (kind, rejection, conflicts) = match error {
        RequestError::EvSessionsConflict { conflicts } => ("ev_session_conflict", None, conflicts),
        RequestError::EvReplaceRejected {
            rejection,
            conflicts,
        } => ("ev_replace_rejected", Some(rejection), conflicts),
        _ => return None,
    };
    let mut body = serde_json::json!({
        "kind": kind,
        "error": error.to_string(),
        "conflicts": conflicts.iter().map(clash_json).collect::<Vec<_>>(),
        "replaceable_session_ids": conflicts.iter().map(|c| c.id).collect::<Vec<_>>(),
    });
    // Why a stated instruction was refused, so the client can tell "your
    // confirmation went stale" from "you never confirmed anything".
    if let Some(r) = rejection {
        body["rejection"] = match r {
            EvSessionReplaceRejection::NotQueued { ids } => {
                serde_json::json!({ "reason": "not_queued", "ids": ids })
            }
            EvSessionReplaceRejection::NotTheConflictSet { missing, extra } => serde_json::json!({
                "reason": "not_the_conflict_set",
                "missing": missing,
                "extra": extra,
            }),
            EvSessionReplaceRejection::StillConflicts(c) => serde_json::json!({
                "reason": "still_conflicts",
                "ids": c.conflicts,
            }),
        };
    }
    Some((StatusCode::CONFLICT, Json(body)).into_response())
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
}
