//! Which event wins where events overlap in time: the one ranking the VEN acts on and the BFF shows.
//!
//! Owns: ranking events (BL-02) and resolving them into non-overlapping segments, per payload
//! type (GB-45). Reads: events, through `event_timing::timed_intervals` (when intervals run) and
//! `PayloadReader` (what a value means). Must never: re-read a number or re-time an interval itself,
//! or be copied by a caller — the VEN's tariff and capacity parsers and the BFF's signal bands all
//! call `resolve`, so a band drawn for an operator and the value a VEN applies cannot disagree.

use std::collections::HashMap;

use chrono::{DateTime, Utc};

use crate::event_timing::{timed_intervals, wire_name, OadrEvent};
use crate::wire_contract::{DeclaredBy, PayloadReader, Refusal};

/// What won one payload type in one segment.
#[derive(Debug, Clone, PartialEq)]
pub struct Winner {
    pub value: f64,
    pub declared_by: DeclaredBy,
    pub event_id: String,
    pub event_name: Option<String>,
}

/// One stretch of time in which the winner of every payload type is constant.
#[derive(Debug, Clone, PartialEq)]
pub struct Segment {
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
    /// Payload type (as OpenADR spells it) -> its winner.
    pub winners: HashMap<String, Winner>,
}

/// A payload whose declared unit or currency the profile does not read: it took no part in the
/// ranking (a VEN drops it too), and is listed so a surface can say so.
#[derive(Debug, Clone, PartialEq)]
pub struct RefusedInterval {
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
    pub event_name: Option<String>,
    pub refusal: Refusal,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Resolved {
    pub segments: Vec<Segment>,
    pub refused: Vec<RefusedInterval>,
}

/// One event interval after looping expansion, before overlap resolution.
struct Candidate<'a> {
    start: DateTime<Utc>,
    end: DateTime<Utc>,
    /// Position in the BL-02 "wins last" order: a higher rank wins an overlap.
    rank: usize,
    event: &'a OadrEvent,
    payloads: Vec<(String, f64, DeclaredBy)>,
}

/// Resolve `events` into non-overlapping segments, each payload type won by the highest-ranked
/// event covering the segment that carries it. `payload_types` limits the types read (`None` =
/// every type that carries a number). Segments no event covers are not emitted (gaps stay gaps).
///
/// Ranking (OpenADR 3.1 User Guide §7.1): a lower `priority` number wins, an absent priority
/// ranks lowest, equal priority is decided by the newer `createdDateTime`, and a remaining tie
/// keeps input order (later input wins).
///
/// Looping is not done here: `timed_intervals` is the one place that applies `event.duration`
/// (User Guide 647).
pub fn resolve(
    events: &[OadrEvent],
    reader: &PayloadReader,
    payload_types: Option<&[&str]>,
) -> Resolved {
    let mut ordered: Vec<&OadrEvent> = events.iter().collect();
    // Plain ascending on `Priority`, which already *is* this ordering: its `Ord` puts
    // `UNSPECIFIED` below every number and reverses the rest, so ascending gives "lowest priority
    // first, highest last". `createdDateTime` is required on the wire, so no default is needed.
    ordered.sort_by(|a, b| {
        a.content
            .priority
            .cmp(&b.content.priority)
            .then_with(|| a.created_date_time.cmp(&b.created_date_time))
    });

    let mut candidates: Vec<Candidate> = Vec::new();
    let mut refused: Vec<RefusedInterval> = Vec::new();
    for (rank, event) in ordered.into_iter().enumerate() {
        for timed in timed_intervals(event) {
            if timed.start >= timed.end {
                continue;
            }
            let mut payloads = Vec::new();
            for p in &timed.interval.payloads {
                let name = wire_name(&p.value_type);
                if payload_types.is_some_and(|wanted| !wanted.contains(&name.as_str())) {
                    continue;
                }
                match reader.read(event, p) {
                    Some(Ok(r)) => payloads.push((name, r.value, r.declared_by)),
                    Some(Err(refusal)) => refused.push(RefusedInterval {
                        start: timed.start,
                        end: timed.end,
                        event_name: event.content.event_name.clone(),
                        refusal,
                    }),
                    None => {}
                }
            }
            // An interval carrying none of the wanted types must not add segment boundaries
            // that would split relevant ones.
            if !payloads.is_empty() {
                candidates.push(Candidate {
                    start: timed.start,
                    end: timed.end,
                    rank,
                    event,
                    payloads,
                });
            }
        }
    }
    Resolved {
        segments: resolve_segments(&candidates),
        refused,
    }
}

/// Boundaries are the union of every candidate's start and end, so input without overlaps comes
/// out unchanged.
fn resolve_segments(candidates: &[Candidate<'_>]) -> Vec<Segment> {
    let mut bounds: Vec<DateTime<Utc>> = candidates.iter().flat_map(|c| [c.start, c.end]).collect();
    bounds.sort();
    bounds.dedup();

    let mut result = Vec::new();
    for w in bounds.windows(2) {
        let (start, end) = (w[0], w[1]);
        let mut winners: HashMap<String, (usize, Winner)> = HashMap::new();
        for c in candidates
            .iter()
            .filter(|c| c.start <= start && end <= c.end)
        {
            for (payload_type, value, declared_by) in &c.payloads {
                let beaten = winners
                    .get(payload_type)
                    .is_some_and(|(rank, _)| *rank > c.rank);
                if !beaten {
                    let winner = Winner {
                        value: *value,
                        declared_by: *declared_by,
                        event_id: c.event.id.to_string(),
                        event_name: c.event.content.event_name.clone(),
                    };
                    winners.insert(payload_type.clone(), (c.rank, winner));
                }
            }
        }
        if !winners.is_empty() {
            result.push(Segment {
                start,
                end,
                winners: winners.into_iter().map(|(t, (_, w))| (t, w)).collect(),
            });
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_fixtures::events_from_json;
    use serde_json::json;

    fn t(s: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc)
    }

    /// One event with a single PRICE interval at `start` for `duration` ISO text.
    fn price(
        id: &str,
        priority: Option<i64>,
        created: &str,
        start: &str,
        duration: &str,
        value: f64,
    ) -> serde_json::Value {
        json!({
            "id": id,
            "eventName": format!("name-{id}"),
            "createdDateTime": created,
            "priority": priority,
            "intervalPeriod": {"start": start, "duration": duration},
            "intervals": [{"id": 0, "payloads": [{"type": "PRICE", "values": [value]}]}]
        })
    }

    fn resolve_all(events: Vec<serde_json::Value>) -> Resolved {
        resolve(
            &events_from_json(json!(events)),
            &PayloadReader::default(),
            None,
        )
    }

    #[test]
    fn priority_one_beats_no_priority_whatever_the_order() {
        let low = price(
            "low",
            None,
            "2026-01-01T00:00:00Z",
            "2026-10-30T08:00:00Z",
            "PT1H",
            0.25,
        );
        let high = price(
            "high",
            Some(1),
            "2026-01-01T00:00:00Z",
            "2026-10-30T08:00:00Z",
            "PT1H",
            0.12,
        );
        for events in [vec![low.clone(), high.clone()], vec![high, low]] {
            let r = resolve_all(events);
            assert_eq!(r.segments.len(), 1);
            assert_eq!(r.segments[0].winners["PRICE"].value, 0.12);
            assert_eq!(r.segments[0].winners["PRICE"].event_id, "high");
            assert_eq!(
                r.segments[0].winners["PRICE"].event_name.as_deref(),
                Some("name-high")
            );
        }
    }

    #[test]
    fn equal_priority_newer_created_wins() {
        let r = resolve_all(vec![
            price(
                "old",
                Some(3),
                "2026-01-01T00:00:00Z",
                "2026-10-30T08:00:00Z",
                "PT1H",
                0.10,
            ),
            price(
                "new",
                Some(3),
                "2026-02-01T00:00:00Z",
                "2026-10-30T08:00:00Z",
                "PT1H",
                0.20,
            ),
        ]);
        assert_eq!(r.segments[0].winners["PRICE"].event_id, "new");
    }

    #[test]
    fn a_partial_overlap_splits_the_lower_event() {
        let r = resolve_all(vec![
            price(
                "base",
                None,
                "2026-01-01T00:00:00Z",
                "2026-10-30T08:00:00Z",
                "PT1H",
                0.25,
            ),
            price(
                "dip",
                Some(1),
                "2026-01-01T00:00:00Z",
                "2026-10-30T08:20:00Z",
                "PT20M",
                0.06,
            ),
        ]);
        let got: Vec<_> = r
            .segments
            .iter()
            .map(|s| (s.start, s.end, s.winners["PRICE"].event_id.as_str()))
            .collect();
        assert_eq!(
            got,
            vec![
                (t("2026-10-30T08:00:00Z"), t("2026-10-30T08:20:00Z"), "base"),
                (t("2026-10-30T08:20:00Z"), t("2026-10-30T08:40:00Z"), "dip"),
                (t("2026-10-30T08:40:00Z"), t("2026-10-30T09:00:00Z"), "base"),
            ]
        );
    }

    #[test]
    fn each_payload_type_resolves_on_its_own() {
        let both = json!({
            "id": "both", "eventName": "both", "priority": null,
            "intervalPeriod": {"start": "2026-10-30T08:00:00Z", "duration": "PT1H"},
            "intervals": [{"id": 0, "payloads": [
                {"type": "PRICE", "values": [0.25]},
                {"type": "EXPORT_PRICE", "values": [0.19]}
            ]}]
        });
        let only_price = price(
            "p1",
            Some(1),
            "2026-01-01T00:00:00Z",
            "2026-10-30T08:00:00Z",
            "PT1H",
            0.12,
        );
        let r = resolve_all(vec![both, only_price]);
        assert_eq!(r.segments.len(), 1);
        assert_eq!(r.segments[0].winners["PRICE"].event_id, "p1");
        assert_eq!(r.segments[0].winners["EXPORT_PRICE"].event_id, "both");
    }

    #[test]
    fn wanted_types_limit_what_is_read_and_gaps_stay_gaps() {
        let events = events_from_json(json!([
            price(
                "a",
                None,
                "2026-01-01T00:00:00Z",
                "2026-10-30T08:00:00Z",
                "PT1H",
                0.25
            ),
            price(
                "b",
                None,
                "2026-01-01T00:00:00Z",
                "2026-10-30T10:00:00Z",
                "PT1H",
                0.30
            ),
        ]));
        let none = resolve(&events, &PayloadReader::default(), Some(&["EXPORT_PRICE"]));
        assert!(none.segments.is_empty());
        let r = resolve(&events, &PayloadReader::default(), Some(&["PRICE"]));
        assert_eq!(
            r.segments.len(),
            2,
            "the hour between the events is not a segment"
        );
    }

    #[test]
    fn a_payload_with_an_unreadable_unit_is_skipped_and_listed() {
        let bad = json!({
            "id": "bad", "eventName": "bad", "priority": 1,
            "payloadDescriptors": [{"payloadType": "PRICE", "units": "KWH", "currency": "USD"}],
            "intervalPeriod": {"start": "2026-10-30T08:00:00Z", "duration": "PT1H"},
            "intervals": [{"id": 0, "payloads": [{"type": "PRICE", "values": [9.0]}]}]
        });
        let base = price(
            "base",
            None,
            "2026-01-01T00:00:00Z",
            "2026-10-30T08:00:00Z",
            "PT1H",
            0.25,
        );
        let r = resolve_all(vec![bad, base]);
        assert_eq!(
            r.segments[0].winners["PRICE"].event_id, "base",
            "the lower event shows through"
        );
        assert_eq!(r.refused.len(), 1);
        assert_eq!(r.refused[0].event_name.as_deref(), Some("bad"));
        assert_eq!(r.refused[0].start, t("2026-10-30T08:00:00Z"));
    }
}
