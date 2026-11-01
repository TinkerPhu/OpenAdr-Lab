//! The events the previous poll saw, for change detection (GB-63).
//!
//! Split out of `detect.rs` to keep it under the `tasks/` 200-production-line cap. It owns one
//! decision: when an event counts as new. An event is identified by its id *and* its
//! `modificationDateTime`: the VTN keeps the id when it edits an event in place, so a set of ids
//! cannot tell an edit from no change. It holds the versions it was given and decides nothing
//! about what an arrival means.

use chrono::{DateTime, Utc};
use std::collections::{HashMap, HashSet};

use crate::controller::vtn_port::OadrEvent;

/// Every event of one poll, as `(id, modificationDateTime)`.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct KnownEvents {
    version_by_id: HashMap<String, DateTime<Utc>>,
}

impl KnownEvents {
    pub(crate) fn from_events(events: &[OadrEvent]) -> Self {
        Self {
            version_by_id: events
                .iter()
                .map(|e| (e.id.to_string(), e.modification_date_time))
                .collect(),
        }
    }

    /// Whether `event` is new relative to this poll: its id was not there, or its version
    /// differs, earlier or later (a VTN restored from a backup may move it backwards).
    pub(crate) fn is_new(&self, event: &OadrEvent) -> bool {
        self.version_by_id.get(event.id.as_str()) != Some(&event.modification_date_time)
    }

    /// The ids, for what only needs to know which events exist.
    pub(crate) fn ids(&self) -> HashSet<String> {
        self.version_by_id.keys().cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(id: &str, modified: &str) -> OadrEvent {
        lab_core::test_fixtures::events_from_json(serde_json::json!({
            "id": id,
            "programID": "p",
            "modificationDateTime": modified,
            "intervals": []
        }))
        .remove(0)
    }

    #[test]
    fn is_new_same_id_and_version_is_known() {
        let known = KnownEvents::from_events(&[event("ev1", "2026-03-21T09:00:00Z")]);
        assert!(!known.is_new(&event("ev1", "2026-03-21T09:00:00Z")));
    }

    #[test]
    fn is_new_unseen_id_is_new() {
        let known = KnownEvents::from_events(&[event("ev1", "2026-03-21T09:00:00Z")]);
        assert!(known.is_new(&event("ev2", "2026-03-21T09:00:00Z")));
    }

    #[test]
    fn is_new_later_version_of_a_known_id_is_new() {
        let known = KnownEvents::from_events(&[event("ev1", "2026-03-21T09:00:00Z")]);
        assert!(known.is_new(&event("ev1", "2026-03-21T09:05:00Z")));
    }

    #[test]
    fn is_new_earlier_version_of_a_known_id_is_new() {
        let known = KnownEvents::from_events(&[event("ev1", "2026-03-21T09:00:00Z")]);
        assert!(known.is_new(&event("ev1", "2026-03-21T08:00:00Z")));
    }

    #[test]
    fn is_new_everything_is_new_before_the_first_poll() {
        assert!(KnownEvents::default().is_new(&event("ev1", "2026-03-21T09:00:00Z")));
    }

    #[test]
    fn ids_lists_each_event_once() {
        let known = KnownEvents::from_events(&[
            event("ev1", "2026-03-21T09:00:00Z"),
            event("ev2", "2026-03-21T09:00:00Z"),
        ]);
        assert_eq!(
            known.ids(),
            HashSet::from(["ev1".to_string(), "ev2".to_string()])
        );
    }
}
