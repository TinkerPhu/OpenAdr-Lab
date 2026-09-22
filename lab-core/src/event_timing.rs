//! When does each interval of an OpenADR event run — the one answer every
//! event parser uses (price and capacity schedules, alert/SIMPLE/dispatch/
//! charge-state windows, report activity). OpenADR 3.1 User Guide §7.3
//! ("intervalPeriod"): an interval's own `intervalPeriod` wins; an interval
//! without its own start follows the previous interval immediately, the first
//! one starting at `event.intervalPeriod.start`; an interval without its own
//! duration lasts the event-level duration.
//!
//! Where the spec leaves a gap, one project rule applies to every event type:
//! no duration anywhere → open-ended (the VTN only lists events whose lifespan
//! has not ended, so the event bounds it); no start derivable → in force for as
//! long as the VTN lists the event. A start of `0001-01-01` ("now") and a
//! `P9999Y` duration ("infinity") need no special case: parsed literally they
//! already mean what the spec says.

use chrono::{DateTime, Utc};

// The wire crate's own types: this rule reads an OpenADR event, so it takes
// one rather than a local re-description of one.
use crate::time_window::TimeWindow;
pub use openleadr_wire::event::{Event as OadrEvent, EventInterval as OadrInterval};

/// The start of an interval no start could be derived for.
pub const OPEN_START: DateTime<Utc> = DateTime::<Utc>::MIN_UTC;
/// The end of an interval without a duration.
pub const OPEN_END: DateTime<Utc> = DateTime::<Utc>::MAX_UTC;

/// One interval of an event with its absolute `[start, end)`.
#[derive(Debug, Clone, Copy)]
pub struct TimedInterval<'a> {
    pub interval: &'a OadrInterval,
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
}

impl TimeWindow for TimedInterval<'_> {
    fn start(&self) -> DateTime<Utc> {
        self.start
    }
    fn end(&self) -> DateTime<Utc> {
        self.end
    }
}

impl TimedInterval<'_> {
    /// Neither end is open.
    pub fn is_bounded(&self) -> bool {
        self.start != OPEN_START && self.end != OPEN_END
    }
}

/// An enum's name exactly as it appears on the wire.
///
/// Derived from its own `Serialize`, so the name can never drift from what is
/// actually sent — writing the strings out by hand is how a match arm and a
/// payload end up disagreeing about `IMPORT_CAPACITY_LIMIT`. Shared because
/// the VEN matches on these names and the BFF labels bands with them.
pub fn wire_name<T: serde::Serialize + std::fmt::Debug>(v: &T) -> String {
    match serde_json::to_value(v) {
        Ok(serde_json::Value::String(s)) => s,
        other => {
            debug_assert!(false, "{v:?} did not serialise to a string: {other:?}");
            String::new()
        }
    }
}

/// A payload's first value as a number.
///
/// `Integer` is matched as well as `Number` and that is not an afterthought:
/// `EventType::Simple`'s declared value kind *is* Integer, so a reader that
/// only matched `Number` drops every load-shed level and says nothing about
/// it. Lives here because both the VEN (deciding what to do) and the BFF
/// (drawing what was asked) need the same answer, and two copies of this match
/// is exactly how one of them ends up silently ignoring SIMPLE.
pub fn numeric_value(value: &openleadr_wire::values_map::Value) -> Option<f64> {
    match value {
        openleadr_wire::values_map::Value::Number(n) => Some(*n),
        openleadr_wire::values_map::Value::Integer(i) => Some(*i as f64),
        _ => None,
    }
}

/// How many intervals `event.duration` may expand to. The spec defines `"P9999Y"`
/// as infinity "as agreed to by communicating parties" (User Guide 8.x), so an
/// indefinitely repeating tariff cannot be materialised; expansion stops here.
/// 2000 intervals covers any horizon this VEN reads -- the 288-slot 5-minute
/// grid several times over -- and the planner only looks inside its own horizon.
const MAX_LOOPED_INTERVALS: usize = 2000;

/// Every interval of `event`, in order, with its absolute `[start, end)`.
///
/// OpenADR 3.1 `event.duration` "may be used to augment intervalPeriod
/// definitions to **shorten or lengthen** the temporal span of an event"
/// (User Guide, *event.duration*). It is one window, applied in both directions:
///
/// * longer than the sequence -- the intervals repeat to fill it, which is how
///   the spec expresses a persistent tariff: 24 hourly prices plus
///   `duration = "P9999Y"` repeat indefinitely, or `"P7D"` for a week.
/// * shorter than the sequence -- the surplus is dropped, and an interval
///   straddling the end is clipped. The spec's own example: 24 hourly intervals
///   with `duration = "P12H"` "effectively omits the last 12 intervals".
///
/// With no `duration`, the sequence is returned exactly as declared.
pub fn timed_intervals(event: &OadrEvent) -> Vec<TimedInterval<'_>> {
    let base = base_intervals(event);
    let Some(event_end) = event_window_end(event, &base) else {
        return base;
    };

    // Trim first: this alone handles a duration shorter than the sequence.
    let mut out: Vec<TimedInterval<'_>> = base
        .iter()
        .filter(|it| it.start < event_end)
        .map(|it| TimedInterval {
            interval: it.interval,
            start: it.start,
            end: it.end.min(event_end),
        })
        .collect();

    // Then repeat, if the declared sequence does not reach the window's end.
    let (Some(first), Some(last)) = (base.first(), base.last()) else {
        return out;
    };
    if !first.is_bounded() || !last.is_bounded() || last.end <= first.start {
        return out;
    }
    let span = last.end - first.start;
    let mut shift = span;
    while out.len() < MAX_LOOPED_INTERVALS && first.start + shift < event_end {
        for it in &base {
            if out.len() >= MAX_LOOPED_INTERVALS {
                break;
            }
            let start = it.start + shift;
            if start >= event_end {
                break;
            }
            out.push(TimedInterval {
                interval: it.interval,
                start,
                end: (it.end + shift).min(event_end),
            });
        }
        shift += span;
    }
    out
}

/// Where `event.duration` puts the end of the event, or `None` when the event
/// declares no duration or nothing to anchor it to. Unlike the looping-only
/// reading, this is returned whether it falls before or after the sequence ends.
fn event_window_end(event: &OadrEvent, base: &[TimedInterval<'_>]) -> Option<DateTime<Utc>> {
    let duration = event.content.duration.as_ref()?;
    let first = base.first()?;
    if !first.is_bounded() {
        return None;
    }
    // A typed duration, so no string to misparse -- and in particular no
    // `unwrap_or(3600)` for a string that does not start with `P`, which is
    // what `common::parse_iso8601_duration_secs` did for this until 3.1b.
    // Months and years are resolved against the interval's own start, which is
    // the only point at which they have a length.
    first
        .start
        .checked_add_signed(duration.to_chrono_at_datetime(first.start))
}

/// The interval list exactly as the event declares it, before any looping.
fn base_intervals(event: &OadrEvent) -> Vec<TimedInterval<'_>> {
    let event_period = event.content.interval_period.as_ref();
    let default_duration = event_period.and_then(|p| p.duration.as_ref());
    // Where the next interval starts unless it says otherwise.
    let mut cursor = event_period.map(|p| p.start);

    event
        .content
        .intervals
        .iter()
        .flatten()
        .map(|interval| {
            let own = interval.interval_period.as_ref();
            // `start` is non-optional on the wire type, so an interval that
            // declares a period declares a start -- the `(None, _)` arm below
            // is now reachable only when *neither* the interval nor the event
            // declares one at all.
            let start = own.map(|p| p.start).or(cursor);
            let duration = own.and_then(|p| p.duration.as_ref()).or(default_duration);
            let (start, end) = match (start, duration) {
                (None, _) => (OPEN_START, OPEN_END),
                (Some(start), None) => (start, OPEN_END),
                (Some(start), Some(d)) => {
                    let end = start.checked_add_signed(d.to_chrono_at_datetime(start));
                    (start, end.unwrap_or(OPEN_END))
                }
            };
            cursor = Some(end);
            TimedInterval {
                interval,
                start,
                end,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use serde_json::json;

    /// Every timing fixture routes through here, so the fields 3.1 requires
    /// but no timing test is about (`id`, `createdDateTime`,
    /// `modificationDateTime`, `interval.id`) are filled in one place rather
    /// than spelled out eighteen times.
    fn event(value: serde_json::Value) -> OadrEvent {
        crate::test_fixtures::events_from_json(value).remove(0)
    }

    fn at(h: u32, m: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2023, 2, 10, h, m, 0).unwrap()
    }

    fn spans(e: &OadrEvent) -> Vec<(DateTime<Utc>, DateTime<Utc>)> {
        timed_intervals(e)
            .iter()
            .map(|t| (t.start, t.end))
            .collect()
    }

    fn limit_interval(id: i64, kw: f64) -> serde_json::Value {
        json!({"id": id, "payloads": [{"type": "IMPORT_CAPACITY_LIMIT", "values": [kw]}]})
    }

    #[test]
    fn timed_intervals_follow_the_event_period_contiguously() {
        // User Guide Example 8.10.1-1: one event-level period, intervals without their own.
        let e = event(json!({
            "id": "doe", "programID": "p",
            "intervalPeriod": {"start": "2023-02-10T00:00:00.000Z", "duration": "PT30M"},
            "intervals": [limit_interval(0, 10.0), limit_interval(1, 8.0)]
        }));
        assert_eq!(
            spans(&e),
            vec![(at(0, 0), at(0, 30)), (at(0, 30), at(1, 0))]
        );
    }

    #[test]
    fn timed_intervals_cover_a_day_of_48_half_hours() {
        let intervals: Vec<_> = (0..48).map(|i| limit_interval(i, 5.0)).collect();
        let e = event(json!({
            "id": "doe-day", "programID": "p",
            "intervalPeriod": {"start": "2023-02-10T00:00:00Z", "duration": "PT30M"},
            "intervals": intervals
        }));
        let s = spans(&e);
        assert_eq!(s.len(), 48);
        assert_eq!(s[0].0, at(0, 0));
        assert_eq!(s[47].1, at(0, 0) + chrono::Duration::days(1));
        assert!(s.windows(2).all(|w| w[0].1 == w[1].0), "contiguous");
    }

    #[test]
    fn timed_intervals_own_period_wins_and_the_next_follows_it() {
        let e = event(json!({
            "id": "mixed", "programID": "p",
            "intervalPeriod": {"start": "2023-02-10T10:00:00Z", "duration": "PT1H"},
            "intervals": [
                {"id": 0, "intervalPeriod": {"start": "2023-02-10T12:00:00Z", "duration": "PT30M"}, "payloads": []},
                {"id": 1, "payloads": []}
            ]
        }));
        assert_eq!(
            spans(&e),
            vec![(at(12, 0), at(12, 30)), (at(12, 30), at(13, 30))]
        );
    }

    #[test]
    fn timed_intervals_single_interval_takes_the_event_period() {
        let e = event(json!({
            "id": "single", "programID": "p",
            "intervalPeriod": {"start": "2023-02-10T10:00:00Z", "duration": "PT10M"},
            "intervals": [limit_interval(0, 1.5)]
        }));
        assert_eq!(spans(&e), vec![(at(10, 0), at(10, 10))]);
    }

    #[test]
    fn timed_intervals_own_duration_without_own_start_starts_at_the_cursor() {
        let e = event(json!({
            "id": "dur-only", "programID": "p",
            "intervalPeriod": {"start": "2023-02-10T10:00:00Z", "duration": "PT1H"},
            "intervals": [
                // The wire `IntervalPeriod` requires `start`, so "own duration,
                // inherited start" is stated as an interval period that repeats
                // the cursor's start rather than omitting it. Same meaning, and
                // the only form this stack can carry -- see R-87.
                {"id": 0,
                 "intervalPeriod": {"start": "2023-02-10T10:00:00Z", "duration": "PT15M"},
                 "payloads": []},
                {"id": 1, "payloads": []}
            ]
        }));
        assert_eq!(
            spans(&e),
            vec![(at(10, 0), at(10, 15)), (at(10, 15), at(11, 15))]
        );
    }

    #[test]
    fn timed_intervals_without_any_duration_are_open_ended() {
        let e = event(json!({
            "id": "no-dur", "programID": "p",
            "intervalPeriod": {"start": "2023-02-10T10:00:00Z"},
            "intervals": [limit_interval(0, 3.0)]
        }));
        assert_eq!(spans(&e), vec![(at(10, 0), OPEN_END)]);
    }

    #[test]
    fn timed_intervals_without_any_timing_are_in_force_while_listed() {
        let e = event(json!({
            "id": "untimed", "programID": "p",
            "intervals": [limit_interval(0, 10000.0)]
        }));
        let t = timed_intervals(&e);
        assert_eq!((t[0].start, t[0].end), (OPEN_START, OPEN_END));
        assert!(t[0].covers(Utc::now()));
        assert!(!t[0].is_bounded());
    }

    /// A duration alone cannot place an interval in time. Since 3.1b the
    /// *shape* that used to express this -- an `intervalPeriod` carrying a
    /// duration and no `start` -- cannot be built at all: the wire type makes
    /// `start` mandatory even though the schema does not (R-87). What remains
    /// reachable, and what this now pins, is an event with no interval period
    /// anywhere: nothing to anchor the interval to, so it is open at both ends.
    #[test]
    fn timed_intervals_are_untimed_when_nothing_anchors_them() {
        let e = event(json!({
            "id": "no-anchor", "programID": "p",
            "intervals": [limit_interval(0, 2.0)]
        }));
        let t = timed_intervals(&e);
        assert_eq!((t[0].start, t[0].end), (OPEN_START, OPEN_END));
    }

    #[test]
    fn timed_intervals_do_it_now_with_infinity_covers_now() {
        let e = event(json!({
            "id": "alert", "programID": "p",
            "intervalPeriod": {"start": "0001-01-01T00:00:00Z", "duration": "P9999Y"},
            "intervals": [{"id": 0, "payloads": []}]
        }));
        assert!(timed_intervals(&e)[0].covers(Utc::now()));
    }

    #[test]
    fn timed_intervals_covers_is_half_open() {
        let e = event(json!({
            "id": "edge", "programID": "p",
            "intervalPeriod": {"start": "2023-02-10T10:00:00Z", "duration": "PT1H"},
            "intervals": [{"id": 0, "payloads": []}]
        }));
        let t = timed_intervals(&e)[0];
        assert!(t.covers(at(10, 0)) && t.covers(at(10, 59)) && !t.covers(at(11, 0)));
        assert!(t.is_bounded());
    }

    // ── OpenADR 3.1 looping intervals (User Guide, "Looping intervals") ──────
    //
    // "An event's intervals may be repeated by setting event.duration to a value
    // greater than the sum of all interval durations."

    #[test]
    fn event_duration_shorter_than_the_sequence_omits_the_surplus() {
        // The User Guide's own example under *event.duration*: "24 hourly price
        // intervals but duration = \"P12H\" effectively omits the last 12 intervals".
        let intervals: Vec<_> = (0..24).map(|i| limit_interval(i, 5.0)).collect();
        let e = event(json!({
            "id": "tariff-12h", "programID": "p",
            "duration": "PT12H",
            "intervalPeriod": {"start": "2023-02-10T00:00:00Z", "duration": "PT1H"},
            "intervals": intervals
        }));
        let s = spans(&e);
        assert_eq!(
            s.len(),
            12,
            "the last 12 of 24 hourly intervals are omitted"
        );
        assert_eq!(s[0], (at(0, 0), at(1, 0)));
        assert_eq!(s[11], (at(11, 0), at(12, 0)));
    }

    #[test]
    fn an_interval_straddling_the_event_end_is_clipped() {
        let e = event(json!({
            "id": "clip", "programID": "p",
            "duration": "PT45M",
            "intervalPeriod": {"start": "2023-02-10T00:00:00Z", "duration": "PT30M"},
            "intervals": [limit_interval(0, 10.0), limit_interval(1, 8.0)]
        }));
        // The second interval would run to 01:00; the event stops at 00:45.
        assert_eq!(
            spans(&e),
            vec![(at(0, 0), at(0, 30)), (at(0, 30), at(0, 45))]
        );
    }

    #[test]
    fn a_persistent_daily_tariff_repeats_indefinitely() {
        // The spec's motivating case: "a tariff that defines 24 hourly prices
        // that persist indefinitely" -- 24 hourly intervals plus P9999Y.
        let intervals: Vec<_> = (0..24).map(|i| limit_interval(i, 5.0)).collect();
        let e = event(json!({
            "id": "tariff-forever", "programID": "p",
            "duration": "P9999Y",
            "intervalPeriod": {"start": "2023-02-10T00:00:00Z", "duration": "PT1H"},
            "intervals": intervals
        }));
        let s = spans(&e);
        assert_eq!(s.len(), MAX_LOOPED_INTERVALS);
        // Day two repeats day one's shape, hour for hour.
        assert_eq!(s[24].0, s[0].0 + chrono::Duration::days(1));
        assert_eq!(s[24].1, s[0].1 + chrono::Duration::days(1));
    }

    #[test]
    fn event_duration_no_longer_than_the_sequence_does_not_loop() {
        let e = event(json!({
            "id": "no-loop", "programID": "p",
            "duration": "PT1H",
            "intervalPeriod": {"start": "2023-02-10T00:00:00Z", "duration": "PT30M"},
            "intervals": [limit_interval(0, 10.0), limit_interval(1, 8.0)]
        }));
        // Two half-hours already fill the hour, so there is nothing to repeat.
        assert_eq!(
            spans(&e),
            vec![(at(0, 0), at(0, 30)), (at(0, 30), at(1, 0))]
        );
    }

    #[test]
    fn event_duration_longer_than_the_sequence_repeats_it() {
        let e = event(json!({
            "id": "loop-2h", "programID": "p",
            "duration": "PT2H",
            "intervalPeriod": {"start": "2023-02-10T00:00:00Z", "duration": "PT30M"},
            "intervals": [limit_interval(0, 10.0), limit_interval(1, 8.0)]
        }));
        // One hour of intervals repeated to fill two hours.
        assert_eq!(
            spans(&e),
            vec![
                (at(0, 0), at(0, 30)),
                (at(0, 30), at(1, 0)),
                (at(1, 0), at(1, 30)),
                (at(1, 30), at(2, 0)),
            ]
        );
    }

    #[test]
    fn a_looping_repetition_is_truncated_at_the_event_end() {
        let e = event(json!({
            "id": "loop-90m", "programID": "p",
            "duration": "PT1H45M",
            "intervalPeriod": {"start": "2023-02-10T00:00:00Z", "duration": "PT30M"},
            "intervals": [limit_interval(0, 10.0), limit_interval(1, 8.0)]
        }));
        let s = spans(&e);
        assert_eq!(s.len(), 4, "two full hours' worth, the last one clipped");
        // The final interval starts inside the event and ends where the event does.
        assert_eq!(s[3], (at(1, 30), at(1, 45)));
    }

    #[test]
    fn looping_forever_is_bounded_rather_than_materialised() {
        // "To loop indefinitely, set event.duration as: P9999Y". Expanding that
        // literally would be tens of millions of intervals; the cap keeps it finite
        // while still covering any horizon the planner asks about.
        let e = event(json!({
            "id": "loop-forever", "programID": "p",
            "duration": "P9999Y",
            "intervalPeriod": {"start": "2023-02-10T00:00:00Z", "duration": "PT30M"},
            "intervals": [limit_interval(0, 10.0), limit_interval(1, 8.0)]
        }));
        let s = spans(&e);
        assert_eq!(s.len(), MAX_LOOPED_INTERVALS);
        // Still contiguous, and far past any planning horizon: 2000 half-hours
        // is roughly 41 days, so the last interval ends well beyond this day.
        assert_eq!(s[0].0, at(0, 0));
        assert!(s.last().unwrap().1 > at(23, 30));
    }

    #[test]
    fn an_open_ended_sequence_is_never_looped() {
        // Without a duration the sequence has no span to repeat, so the event
        // duration cannot be filled by repetition and the list stays as declared.
        let e = event(json!({
            "id": "loop-open", "programID": "p",
            "duration": "PT4H",
            "intervalPeriod": {"start": "2023-02-10T00:00:00Z"},
            "intervals": [limit_interval(0, 10.0)]
        }));
        assert_eq!(spans(&e).len(), 1);
    }
}
