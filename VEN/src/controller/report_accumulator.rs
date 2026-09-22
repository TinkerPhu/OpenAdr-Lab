//! Keeping a report's intervals across submissions (D-3).
//!
//! Each submission builds only the intervals that closed since the last one
//! (F-5). Sent as-is, that replaces the report on the VTN with a window two or
//! three intervals wide, and the series a reader sees is whatever the last PUT
//! happened to carry. The VTN does not merge — `PUT` replaces the object — so
//! the accumulation has to happen here, and the report we send is the whole
//! remembered window.
//!
//! ## Bounded by count, not by time
//!
//! D-3 says 24 hours. That decision was recorded without the payload
//! arithmetic, and the arithmetic matters: the fleet monitoring event's grid is
//! 60 s with `frequency: 1`, so a 24 h window is ~1440 intervals, roughly
//! 200 KB of JSON, PUT every minute, per VEN. Across twenty VENs that is
//! megabytes a minute into a Pi — for a series the BFF's own telemetry store
//! already keeps at full resolution.
//!
//! So the bound is a number of intervals rather than a duration. It is the
//! quantity that actually needs bounding (the request size), it holds whatever
//! grid an event asks for, and at the default it is a couple of hours of
//! 60 s intervals in a request measured in tens of kilobytes.
//!
//! ## What a restart costs
//!
//! Nothing is persisted: after a restart the window refills from empty, and
//! the VTN's copy of the report shrinks to match before growing again. That is
//! deliberate — the durable record of every interval is the BFF recorder's
//! archive of each report version (GB-36), which keeps them all — and it is
//! why the bound is small enough that refilling takes minutes.

use chrono::{DateTime, Utc};

use crate::controller::vtn_port::OadrReportInterval;

/// How many intervals one report carries at most.
///
/// 120 is two hours at the fleet grid's 60 s, in a request of tens of
/// kilobytes. Raising it raises every PUT's size in direct proportion.
pub const MAX_REPORT_INTERVALS: usize = 120;

/// Merge freshly-closed intervals into the ones already submitted.
///
/// - Keyed on each interval's own start, so re-measuring an interval replaces
///   it rather than duplicating it. Fresh always wins: a later measurement of
///   the same window is the better one.
/// - An interval with no start cannot be positioned or deduplicated, so it is
///   kept only among the fresh ones — carrying it forward would mean an
///   ever-growing tail of intervals nothing can place.
/// - Oldest dropped first once the bound is reached, and ids renumbered from
///   zero, because `id` is a position within the report and the report has
///   changed.
pub fn accumulate(
    previous: &[OadrReportInterval],
    fresh: &[OadrReportInterval],
    max_intervals: usize,
) -> Vec<OadrReportInterval> {
    let fresh_starts: Vec<&str> = fresh.iter().filter_map(start_of).collect();

    let mut merged: Vec<OadrReportInterval> = previous
        .iter()
        .filter(|iv| match start_of(iv) {
            Some(start) => !fresh_starts.contains(&start),
            None => false,
        })
        .cloned()
        .collect();
    merged.extend(fresh.iter().cloned());

    // Lexicographic on the RFC 3339 string is chronological for the UTC
    // timestamps this VEN writes, and the intervals being sorted are all its
    // own. Intervals with no start sort first and are dropped first.
    merged.sort_by(|a, b| start_of(a).cmp(&start_of(b)));

    if merged.len() > max_intervals {
        merged.drain(0..merged.len() - max_intervals);
    }
    for (i, iv) in merged.iter_mut().enumerate() {
        iv.id = i;
    }
    merged
}

fn start_of(interval: &OadrReportInterval) -> Option<&str> {
    interval.intervalPeriod.as_ref()?.start.as_deref()
}

/// Whether `now` says this accumulation has gone stale enough to discard.
///
/// A report whose event ended hours ago should not keep re-sending the same
/// intervals for ever. The caller drops the window when the obligation itself
/// is retired; this is the guard for the case where it is not.
pub fn is_stale(last_submitted: DateTime<Utc>, now: DateTime<Utc>) -> bool {
    now - last_submitted > chrono::Duration::hours(6)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::controller::vtn_port::{OadrIntervalPeriod, OadrReportPayload};

    fn interval(start: &str, value: f64) -> OadrReportInterval {
        OadrReportInterval {
            id: 0,
            intervalPeriod: Some(OadrIntervalPeriod {
                start: Some(start.to_string()),
                duration: Some("PT1M".to_string()),
                randomizeStart: None,
            }),
            payloads: vec![OadrReportPayload::power_kw("DEMAND", value)],
        }
    }

    fn value_of(iv: &OadrReportInterval) -> f64 {
        iv.payloads[0].values[0].as_f64().unwrap()
    }

    fn starts(ivs: &[OadrReportInterval]) -> Vec<String> {
        ivs.iter()
            .map(|iv| iv.intervalPeriod.as_ref().unwrap().start.clone().unwrap())
            .collect()
    }

    #[test]
    fn accumulate_keeps_what_was_already_submitted_and_appends_the_new() {
        let previous = vec![interval("2026-09-22T10:00:00Z", 1.0)];
        let fresh = vec![interval("2026-09-22T10:01:00Z", 2.0)];
        let out = accumulate(&previous, &fresh, MAX_REPORT_INTERVALS);
        assert_eq!(
            starts(&out),
            ["2026-09-22T10:00:00Z", "2026-09-22T10:01:00Z"]
        );
    }

    /// The same window measured twice is one interval, and the later
    /// measurement is the one that counts.
    #[test]
    fn accumulate_replaces_an_interval_rather_than_duplicating_it() {
        let previous = vec![interval("2026-09-22T10:00:00Z", 1.0)];
        let fresh = vec![interval("2026-09-22T10:00:00Z", 9.0)];
        let out = accumulate(&previous, &fresh, MAX_REPORT_INTERVALS);
        assert_eq!(out.len(), 1);
        assert_eq!(value_of(&out[0]), 9.0);
    }

    #[test]
    fn accumulate_orders_intervals_chronologically_whatever_order_they_arrive_in() {
        let previous = vec![interval("2026-09-22T10:02:00Z", 3.0)];
        let fresh = vec![
            interval("2026-09-22T10:01:00Z", 2.0),
            interval("2026-09-22T10:00:00Z", 1.0),
        ];
        let out = accumulate(&previous, &fresh, MAX_REPORT_INTERVALS);
        assert_eq!(
            starts(&out),
            [
                "2026-09-22T10:00:00Z",
                "2026-09-22T10:01:00Z",
                "2026-09-22T10:02:00Z"
            ]
        );
    }

    /// The bound is what stops a report growing until the PUT that carries it
    /// becomes the problem.
    #[test]
    fn accumulate_drops_the_oldest_once_the_bound_is_reached() {
        let previous: Vec<_> = (0..5)
            .map(|i| interval(&format!("2026-09-22T10:0{i}:00Z"), i as f64))
            .collect();
        let fresh = vec![interval("2026-09-22T10:05:00Z", 5.0)];
        let out = accumulate(&previous, &fresh, 3);
        assert_eq!(
            starts(&out),
            [
                "2026-09-22T10:03:00Z",
                "2026-09-22T10:04:00Z",
                "2026-09-22T10:05:00Z"
            ]
        );
    }

    /// `id` is a position within the report, and the report changed.
    #[test]
    fn accumulate_renumbers_ids_from_zero() {
        let previous = vec![interval("2026-09-22T10:00:00Z", 1.0)];
        let fresh = vec![
            interval("2026-09-22T10:01:00Z", 2.0),
            interval("2026-09-22T10:02:00Z", 3.0),
        ];
        let out = accumulate(&previous, &fresh, MAX_REPORT_INTERVALS);
        assert_eq!(out.iter().map(|iv| iv.id).collect::<Vec<_>>(), [0, 1, 2]);
    }

    /// An interval with no start cannot be placed or deduplicated. Keeping it
    /// across submissions would grow a tail of intervals nothing can position.
    #[test]
    fn accumulate_does_not_carry_forward_an_interval_with_no_start() {
        let mut placeless = interval("2026-09-22T10:00:00Z", 1.0);
        placeless.intervalPeriod = None;
        let out = accumulate(&[placeless], &[interval("2026-09-22T10:01:00Z", 2.0)], 10);
        assert_eq!(starts(&out), ["2026-09-22T10:01:00Z"]);
    }

    /// A submission that produced nothing new must not silently erase the
    /// window: the report still describes the intervals it already carried.
    #[test]
    fn accumulate_with_nothing_fresh_keeps_the_window_intact() {
        let previous = vec![
            interval("2026-09-22T10:00:00Z", 1.0),
            interval("2026-09-22T10:01:00Z", 2.0),
        ];
        let out = accumulate(&previous, &[], MAX_REPORT_INTERVALS);
        assert_eq!(out.len(), 2);
    }
}
