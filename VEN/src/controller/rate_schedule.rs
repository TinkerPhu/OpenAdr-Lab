//! Interval-schedule parsing shared by tariffs and the capacity-limit envelope.
//! Split out of `openadr_interface.rs` (2026-08-10) once that file crossed the
//! `VEN/src/` 500-production-line cap — the shared priority-merge/cycle-looping
//! core plus its two callers are a cohesive, self-contained unit. Tests for both
//! `parse_rate_snapshots` and `parse_capacity_schedule` stay in
//! `openadr_interface.rs`'s existing test module (re-exported here via `pub use`
//! at that file's top), so this split touches no test code.

use chrono::{DateTime, Duration, Utc};

use crate::common::parse_iso8601_duration_secs;
use crate::controller::vtn_port::OadrEvent;
use crate::entities::capacity::CapacitySnapshot;
use crate::entities::tariff_snapshot::TariffSnapshot;

/// One resolved segment: [start, end) plus every requested payload type's value
/// for that segment, each taken from the highest-ranked event covering it.
type IntervalGroup = (
    DateTime<Utc>,
    DateTime<Utc>,
    std::collections::HashMap<String, f64>,
);

/// One event interval after looping expansion, before overlap resolution.
struct Candidate {
    start: DateTime<Utc>,
    end: DateTime<Utc>,
    /// Position in the BL-02 "wins last" order: a higher rank wins an overlap.
    rank: usize,
    payloads: Vec<(String, f64)>,
}

/// Shared interval-collection core for both `parse_rate_snapshots` and
/// `parse_capacity_schedule` — same priority and cycle-looping semantics,
/// differing only in which OpenADR payload types are collected. Extracted so the
/// two callers don't duplicate the looping/priority logic (generic-over-bespoke).
///
/// Supports looping events: when `event.intervalPeriod.duration` exceeds the total
/// span of all intervals, the interval set is repeated (offset by one cycle each time)
/// to cover [now − 1 cycle … now + 3 days]. This implements the OpenADR 3 spec's
/// "persistent daily prices" pattern (`event.intervalPeriod.duration = "P9999Y"`).
///
/// GB-45: the result is non-overlapping and already priority-resolved (see
/// `resolve_segments`), so every consumer — tick-time cost, history sampler,
/// planner tariff series, planned capacity limits — reads the same value for the
/// same instant without a resolution rule of its own.
fn collect_interval_groups(
    events: &[OadrEvent],
    now: DateTime<Utc>,
    payload_types: &[&str],
) -> Vec<IntervalGroup> {
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
        let pa = a.priority.unwrap_or(i64::MAX);
        let pb = b.priority.unwrap_or(i64::MAX);
        pb.cmp(&pa).then_with(|| {
            let created = |e: &OadrEvent| {
                e.createdDateTime
                    .as_deref()
                    .and_then(|s| s.parse::<DateTime<Utc>>().ok())
                    .unwrap_or(DateTime::<Utc>::MIN_UTC)
            };
            created(a).cmp(&created(b))
        })
    });

    for (rank, event) in ordered.into_iter().enumerate() {
        if event.intervals.is_empty() {
            continue;
        }

        // ── Collect base intervals ────────────────────────────────────────────
        type IntervalEntry = (DateTime<Utc>, i64, Vec<(String, f64)>);
        let mut base: Vec<IntervalEntry> = Vec::new();

        for interval in &event.intervals {
            // GB-33: a single-interval event (every capacity_limit/reservation
            // event this project's own experiment tooling sends, and any
            // spec-compliant one-window DR event) is allowed to set
            // `intervalPeriod` only at the event level, not per-interval. Only
            // fall back for that narrow, unambiguous case — a multi-interval
            // event without per-interval periods has spec-ambiguous sequential
            // timing nothing here needs to guess at (see
            // test_parse_capacity_schedule_does_not_guess_for_multi_interval_events).
            let event_level_fallback = (event.intervals.len() == 1)
                .then_some(event.intervalPeriod.as_ref())
                .flatten();
            let ip = match interval.intervalPeriod.as_ref().or(event_level_fallback) {
                Some(ip) => ip,
                None => continue,
            };
            let start_str = match ip.start.as_deref() {
                Some(s) => s,
                None => continue,
            };
            let interval_start: DateTime<Utc> = match start_str.parse() {
                Ok(dt) => dt,
                Err(_) => continue,
            };
            let duration_secs =
                parse_iso8601_duration_secs(ip.duration.as_deref().unwrap_or("PT1H"));

            let mut payloads: Vec<(String, f64)> = Vec::new();
            for p in &interval.payloads {
                let t = p.r#type.as_str();
                let v = p.values.first().and_then(|v| v.as_f64());
                if payload_types.contains(&t) {
                    if let Some(val) = v {
                        payloads.push((t.to_string(), val));
                    }
                }
            }

            base.push((interval_start, duration_secs, payloads));
        }

        if base.is_empty() {
            continue;
        }

        // ── Determine looping offsets ─────────────────────────────────────────
        let first_start = base.iter().map(|(s, _, _)| *s).min().unwrap();
        let last_end = base
            .iter()
            .map(|(s, d, _)| *s + Duration::seconds(*d))
            .max()
            .unwrap();
        let cycle_secs = (last_end - first_start).num_seconds();

        let event_dur_secs = event
            .intervalPeriod
            .as_ref()
            .and_then(|ip| ip.duration.as_deref())
            .map(parse_iso8601_duration_secs)
            .unwrap_or(cycle_secs);

        let offsets: Vec<i64> = if cycle_secs > 0 && event_dur_secs > cycle_secs {
            let elapsed = (now - first_start).num_seconds().max(0);
            let n = elapsed / cycle_secs; // index of the cycle that contains now
            let from = n.saturating_sub(1); // one cycle back for "most recent past" fallback
            let ahead = (3 * 86400i64) / cycle_secs + 2; // cycles needed to cover 3 days ahead
            let to = (from + ahead).min(from + 10); // hard cap: at most 11 cycles total
            (from..=to).map(|k| k * cycle_secs).collect()
        } else {
            vec![0i64]
        };

        // ── Expand into candidates for each offset ────────────────────────────
        for &offset in &offsets {
            for (base_start, dur, payloads) in &base {
                // An interval carrying none of the requested payload types must not
                // contribute segment boundaries that would split relevant ones.
                if payloads.is_empty() || *dur <= 0 {
                    continue;
                }
                let start = *base_start + Duration::seconds(offset);
                candidates.push(Candidate {
                    start,
                    end: start + Duration::seconds(*dur),
                    rank,
                    payloads: payloads.clone(),
                });
            }
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
fn resolve_segments(candidates: &[Candidate]) -> Vec<IntervalGroup> {
    let mut bounds: Vec<DateTime<Utc>> = candidates.iter().flat_map(|c| [c.start, c.end]).collect();
    bounds.sort();
    bounds.dedup();

    let mut result = Vec::new();
    for w in bounds.windows(2) {
        let (seg_start, seg_end) = (w[0], w[1]);
        let mut winners: std::collections::HashMap<String, (usize, f64)> =
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
                    winners.insert(payload_type.clone(), (c.rank, *value));
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
pub fn parse_rate_snapshots(events: &[OadrEvent], now: DateTime<Utc>) -> Vec<TariffSnapshot> {
    collect_interval_groups(events, now, &["PRICE", "EXPORT_PRICE", "GHG"])
        .into_iter()
        .filter_map(|(interval_start, interval_end, payloads)| {
            let import_tariff_eur_kwh = payloads.get("PRICE").copied();
            let export_tariff_eur_kwh = payloads.get("EXPORT_PRICE").copied();
            let co2_g_kwh = payloads.get("GHG").copied();
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
/// IMPORT_CAPACITY_LIMIT/EXPORT_CAPACITY_LIMIT payload types per event interval,
/// keeping the full per-interval schedule — unlike `parse_capacity_state`, which
/// collapses everything into a single current-value scalar.
pub fn parse_capacity_schedule(events: &[OadrEvent], now: DateTime<Utc>) -> Vec<CapacitySnapshot> {
    collect_interval_groups(
        events,
        now,
        &["IMPORT_CAPACITY_LIMIT", "EXPORT_CAPACITY_LIMIT"],
    )
    .into_iter()
    .filter_map(|(interval_start, interval_end, payloads)| {
        let import_limit_kw = payloads.get("IMPORT_CAPACITY_LIMIT").copied();
        let export_limit_kw = payloads.get("EXPORT_CAPACITY_LIMIT").copied();
        if import_limit_kw.is_none() && export_limit_kw.is_none() {
            return None;
        }
        Some(CapacitySnapshot {
            interval_start,
            interval_end,
            import_limit_kw,
            export_limit_kw,
        })
    })
    .collect()
}
