//! Interval-schedule parsing shared by tariffs and the capacity-limit envelope.
//! Split out of `openadr_interface.rs` (2026-08-10) once that file crossed the
//! `VEN/src/` 500-production-line cap — the shared priority-merge/cycle-looping
//! core plus its two callers are a cohesive, self-contained unit. Tests for both
//! `parse_rate_snapshots` and `parse_capacity_schedule` stay in
//! `openadr_interface.rs`'s existing test module (re-exported here via `pub use`
//! at that file's top), so this split touches no test code.

use chrono::{DateTime, Utc};

use crate::controller::vtn_port::{EventTypeName, OadrEvent, PayloadValues};
use crate::entities::capacity::CapacitySnapshot;
use crate::entities::tariff_snapshot::TariffSnapshot;

/// One requested payload type's value in a resolved segment, with the event it
/// came from (the highest-ranked event covering the segment).
#[derive(Debug, Clone)]
struct SegmentValue {
    value: f64,
    event_id: String,
}

/// One resolved segment: [start, end) plus every requested payload type's value.
type IntervalGroup = (
    DateTime<Utc>,
    DateTime<Utc>,
    std::collections::HashMap<String, SegmentValue>,
);

/// One event interval after looping expansion, before overlap resolution.
struct Candidate<'a> {
    start: DateTime<Utc>,
    end: DateTime<Utc>,
    /// Position in the BL-02 "wins last" order: a higher rank wins an overlap.
    rank: usize,
    event_id: &'a str,
    payloads: Vec<(String, f64)>,
}

/// Shared interval-collection core for both `parse_rate_snapshots` and
/// `parse_capacity_schedule` — same priority semantics, differing only in which
/// OpenADR payload types are collected. Extracted so the two callers don't
/// duplicate the priority logic (one-concept-one-function).
///
/// Looping is *not* done here. `event_timing::timed_intervals` is the one place
/// that applies `event.duration`, the spec's control for repeating a sequence
/// ("persistent daily prices", `event.duration = "P9999Y"`, User Guide 647).
///
/// GB-45: the result is non-overlapping and already priority-resolved (see
/// `resolve_segments`), so every consumer — tick-time cost, history sampler,
/// planner tariff series, planned capacity limits — reads the same value for the
/// same instant without a resolution rule of its own.
fn collect_interval_groups(events: &[OadrEvent], payload_types: &[&str]) -> Vec<IntervalGroup> {
    let mut candidates: Vec<Candidate> = Vec::new();

    // ── BL-02: priority order ───────────────────────────────────────────────
    // OpenADR 3.1 User Guide §7.1: event `priority` — lower number = higher
    // priority; an absent priority is treated as lowest. Sorted into "wins last"
    // order: lowest-priority events (including `None`) first, highest-priority
    // last; equal priority breaks the tie on `createdDateTime` — newer last. The
    // sort is stable, so a remaining tie keeps input order (later input wins).
    // Each event's position in this order is its rank in `resolve_segments`.
    let mut ordered: Vec<&OadrEvent> = events.iter().collect();
    ordered.sort_by(|a, b| {
        // Plain ascending on `Priority`, which already *is* this ordering:
        // its `Ord` puts `UNSPECIFIED` below every number and reverses the
        // rest, so ascending gives "lowest priority first, highest last".
        // This used to be `unwrap_or(i64::MAX)` then `pb.cmp(&pa)`; carrying
        // that shape across would have silently inverted BL-02.
        a.content
            .priority
            .cmp(&b.content.priority)
            // `createdDateTime` is required on the wire, so there is no
            // absent-value default to pick here any more.
            .then_with(|| a.created_date_time.cmp(&b.created_date_time))
    });

    for (rank, event) in ordered.into_iter().enumerate() {
        // Interval timing: the one shared rule (`controller::event_timing`).
        let base: Vec<_> = lab_core::event_timing::timed_intervals(event)
            .into_iter()
            .map(|t| {
                let payloads: Vec<(String, f64)> = t
                    .interval
                    .payloads
                    .iter()
                    .filter_map(|p| {
                        let name = p.value_type.wire_name();
                        payload_types
                            .contains(&name.as_str())
                            .then(|| Some((name, p.numeric()?)))
                            .flatten()
                    })
                    .collect();
                (t, payloads)
            })
            // An interval carrying none of the requested payload types must not
            // contribute segment boundaries that would split relevant ones.
            .filter(|(t, payloads)| !payloads.is_empty() && t.start < t.end)
            .collect();
        if base.is_empty() {
            continue;
        }

        // Looping already happened: `timed_intervals` is the one place that
        // applies `event.duration`, the spec's control for repeating or
        // truncating an interval sequence (User Guide 647). A second expansion
        // used to live here, keyed on `event.intervalPeriod.duration` -- which
        // the spec defines as the *default duration of one interval* (User
        // Guide 572), not an event span. It therefore both duplicated the
        // shared rule and read the wrong field to do it, and an event
        // declaring both got expanded twice (GB-48's shape).
        for (t, payloads) in &base {
            candidates.push(Candidate {
                start: t.start,
                end: t.end,
                rank,
                event_id: event.id.as_str(),
                payloads: payloads.clone(),
            });
        }
    }

    resolve_segments(&candidates)
}

/// GB-45: resolve possibly-overlapping candidates into non-overlapping segments.
/// Boundaries are the union of every candidate's start and end, so input without
/// overlaps comes out unchanged. Within each segment, each payload type takes the
/// value of the highest-ranked candidate covering the segment that carries that
/// type (OpenADR 3.1 User Guide §7.1: priority governs events that overlap in time,
/// not only identical intervals; e.g. PRICE from one event, GHG from another).
/// Segments no candidate covers are not emitted (gaps stay gaps).
fn resolve_segments(candidates: &[Candidate<'_>]) -> Vec<IntervalGroup> {
    let mut bounds: Vec<DateTime<Utc>> = candidates.iter().flat_map(|c| [c.start, c.end]).collect();
    bounds.sort();
    bounds.dedup();

    let mut result = Vec::new();
    for w in bounds.windows(2) {
        let (seg_start, seg_end) = (w[0], w[1]);
        let mut winners: std::collections::HashMap<String, (usize, SegmentValue)> =
            std::collections::HashMap::new();
        for c in candidates
            .iter()
            .filter(|c| c.start <= seg_start && seg_end <= c.end)
        {
            for (payload_type, value) in &c.payloads {
                let beaten = winners
                    .get(payload_type)
                    .is_some_and(|(rank, _)| *rank > c.rank);
                if !beaten {
                    let value = SegmentValue {
                        value: *value,
                        event_id: c.event_id.to_string(),
                    };
                    winners.insert(payload_type.clone(), (c.rank, value));
                }
            }
        }
        if !winners.is_empty() {
            let values = winners.into_iter().map(|(t, (_, v))| (t, v)).collect();
            result.push((seg_start, seg_end, values));
        }
    }
    result
}

/// Parse all rate snapshots from a slice of OpenADR events.
/// Handles PRICE, EXPORT_PRICE, GHG payload types per event interval.
/// Multiple payload types for the same interval are merged into one TariffSnapshot.
pub fn parse_rate_snapshots(events: &[OadrEvent]) -> Vec<TariffSnapshot> {
    collect_interval_groups(events, &["PRICE", "EXPORT_PRICE", "GHG"])
        .into_iter()
        .filter_map(|(interval_start, interval_end, payloads)| {
            let value = |t: &str| payloads.get(t).map(|v| v.value);
            let import_tariff_eur_kwh = value("PRICE");
            let export_tariff_eur_kwh = value("EXPORT_PRICE");
            let co2_g_kwh = value("GHG");
            if import_tariff_eur_kwh.is_none()
                && export_tariff_eur_kwh.is_none()
                && co2_g_kwh.is_none()
            {
                return None;
            }
            Some(TariffSnapshot {
                interval_start,
                interval_end,
                import_tariff_eur_kwh,
                export_tariff_eur_kwh,
                co2_g_kwh,
            })
        })
        .collect()
}

/// Parse the capacity-limit schedule (Dynamic Operating Envelope, OpenADR 3.1
/// User Guide §8.10.1) from a slice of OpenADR events. Handles
/// IMPORT_CAPACITY_LIMIT/EXPORT_CAPACITY_LIMIT payload types per event interval.
/// The single source for "which limit applies when" (GB-48): read it through
/// `entities::capacity::tightest_capacity_limit`.
pub fn parse_capacity_schedule(events: &[OadrEvent]) -> Vec<CapacitySnapshot> {
    collect_interval_groups(events, &["IMPORT_CAPACITY_LIMIT", "EXPORT_CAPACITY_LIMIT"])
        .into_iter()
        .filter_map(|(interval_start, interval_end, payloads)| {
            let import = payloads.get("IMPORT_CAPACITY_LIMIT");
            let export = payloads.get("EXPORT_CAPACITY_LIMIT");
            if import.is_none() && export.is_none() {
                return None;
            }
            Some(CapacitySnapshot {
                interval_start,
                interval_end,
                import_limit_kw: import.map(|v| v.value),
                export_limit_kw: export.map(|v| v.value),
                import_limit_event_id: import.map(|v| v.event_id.clone()),
                export_limit_event_id: export.map(|v| v.event_id.clone()),
            })
        })
        .collect()
}
