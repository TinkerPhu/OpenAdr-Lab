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

use crate::controller::vtn_port::{OadrEvent, OadrInterval};
use crate::entities::time_window::TimeWindow;

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

/// How many intervals a looping event may expand to. `duration = "P9999Y"` means
/// "loop indefinitely" (User Guide, "Looping intervals"), which cannot be
/// materialised, so expansion stops here. 2000 intervals covers any planning
/// horizon this VEN uses -- the 288-slot 5-minute grid several times over --
/// and the planner only ever reads the slots inside its own horizon.
const MAX_LOOPED_INTERVALS: usize = 2000;

/// Every interval of `event`, in order, with its absolute `[start, end)`.
///
/// OpenADR 3.1: when `event.duration` exceeds the sum of the interval durations,
/// the interval sequence repeats to fill it. Expansion is bounded by
/// `MAX_LOOPED_INTERVALS`; an unbounded loop simply yields as many intervals as
/// that allows, which is always more than the caller's horizon needs.
pub fn timed_intervals(event: &OadrEvent) -> Vec<TimedInterval<'_>> {
    let base = base_intervals(event);
    let Some(event_end) = looping_end(event, &base) else {
        return base;
    };

    // The sequence's own span; a non-positive or open span cannot be repeated.
    let (Some(first), Some(last)) = (base.first(), base.last()) else {
        return base;
    };
    if !first.is_bounded() || !last.is_bounded() || last.end <= first.start {
        return base;
    }
    let span = last.end - first.start;

    let mut out = base.clone();
    let mut shift = span;
    while out.len() < MAX_LOOPED_INTERVALS {
        let cycle_start = first.start + shift;
        if cycle_start >= event_end {
            break;
        }
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
                // The final repetition is truncated at the event's end.
                end: (it.end + shift).min(event_end),
            });
        }
        shift += span;
    }
    out
}

/// The instant a looping event stops, or `None` when it does not loop --
/// no `duration`, or one no longer than the sequence already covers.
fn looping_end(event: &OadrEvent, base: &[TimedInterval<'_>]) -> Option<DateTime<Utc>> {
    let duration = event.duration.as_deref()?;
    let first = base.first()?;
    let last = base.last()?;
    if !first.is_bounded() || !last.is_bounded() {
        return None;
    }
    let secs = crate::common::parse_iso8601_duration_secs(duration);
    let end = first
        .start
        .checked_add_signed(chrono::Duration::seconds(secs))?;
    (end > last.end).then_some(end)
}

/// The interval list exactly as the event declares it, before any looping.
fn base_intervals(event: &OadrEvent) -> Vec<TimedInterval<'_>> {
    let parse_start = |s: Option<&str>| s.and_then(|s| s.parse::<DateTime<Utc>>().ok());
    let event_period = event.intervalPeriod.as_ref();
    let default_duration = event_period.and_then(|p| p.duration.as_deref());
    // Where the next interval starts unless it says otherwise.
    let mut cursor = event_period.and_then(|p| parse_start(p.start.as_deref()));

    event
        .intervals
        .iter()
        .map(|interval| {
            let own = interval.intervalPeriod.as_ref();
            let start = own.and_then(|p| parse_start(p.start.as_deref())).or(cursor);
            let duration = own.and_then(|p| p.duration.as_deref()).or(default_duration);
            let (start, end) = match (start, duration) {
                (None, _) => (OPEN_START, OPEN_END),
                (Some(start), None) => (start, OPEN_END),
                (Some(start), Some(d)) => {
                    let secs = crate::common::parse_iso8601_duration_secs(d);
                    let end = start.checked_add_signed(chrono::Duration::seconds(secs));
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

    fn event(value: serde_json::Value) -> OadrEvent {
        serde_json::from_value(value).unwrap()
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
                {"id": 0, "intervalPeriod": {"duration": "PT15M"}, "payloads": []},
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

    #[test]
    fn timed_intervals_untimed_even_with_an_event_duration() {
        // A duration alone cannot place an interval in time.
        let e = event(json!({
            "id": "dur-no-start", "programID": "p",
            "intervalPeriod": {"duration": "PT1H"},
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
