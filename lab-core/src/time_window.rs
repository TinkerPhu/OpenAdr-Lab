//! When is a window in force — the one answer for every window-shaped thing in
//! this VEN (R-83). Alert/SIMPLE/dispatch windows, capacity and tariff
//! segments, an event's timed intervals and a plan's slots all span
//! `[start, end)`, and every one of them used to spell out `start <= t && t <
//! end` (or its `start < to && from < end` overlap form) at each call site.
//! Those copies drifted: three identical `is_ended` impls, an instant lookup
//! written once per consumer, and two slot-overlap forms in the planner.
//!
//! Implement `TimeWindow` on the type and the rules come with it, so a new
//! window kind cannot be asked "are you in force?" in a new way.

use chrono::{DateTime, Utc};

/// A half-open `[start, end)` span of time.
///
/// Half-open is the project-wide convention: a window is in force at its start
/// instant and not at its end instant, so back-to-back segments cover every
/// instant exactly once.
pub trait TimeWindow {
    fn start(&self) -> DateTime<Utc>;
    fn end(&self) -> DateTime<Utc>;

    /// In force at the instant `t` — `start <= t < end`.
    fn covers(&self, t: DateTime<Utc>) -> bool {
        self.start() <= t && t < self.end()
    }

    /// Shares any time with `[from, to)`. A zero-length span (`from == to`) is
    /// the instant `from`, so `overlaps(t, t) == covers(t)` — this is what lets
    /// a planner slot and a "right now" lookup ask the same question.
    fn overlaps(&self, from: DateTime<Utc>, to: DateTime<Utc>) -> bool {
        if from == to {
            self.covers(from)
        } else {
            self.start() < to && from < self.end()
        }
    }

    /// Over at `now` — `now >= end`. An OpenADR event is a permanent record,
    /// so a window stays in state long after its span has passed; consumers
    /// showing "current" signals must drop the ended ones themselves.
    fn is_ended(&self, now: DateTime<Utc>) -> bool {
        now >= self.end()
    }
}

/// The first window covering `t`, if any.
pub fn covering<W: TimeWindow>(windows: &[W], t: DateTime<Utc>) -> Option<&W> {
    windows.iter().find(|w| w.covers(t))
}

/// Whether any window covers `t`.
pub fn any_covering<W: TimeWindow>(windows: &[W], t: DateTime<Utc>) -> bool {
    windows.iter().any(|w| w.covers(t))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    struct Win {
        start: DateTime<Utc>,
        end: DateTime<Utc>,
        tag: &'static str,
    }

    impl TimeWindow for Win {
        fn start(&self) -> DateTime<Utc> {
            self.start
        }
        fn end(&self) -> DateTime<Utc> {
            self.end
        }
    }

    fn at(h: u32, m: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 9, 16, h, m, 0).unwrap()
    }

    fn win(from: (u32, u32), to: (u32, u32), tag: &'static str) -> Win {
        Win {
            start: at(from.0, from.1),
            end: at(to.0, to.1),
            tag,
        }
    }

    #[test]
    fn covers_is_half_open() {
        let w = win((10, 0), (11, 0), "w");
        assert!(w.covers(at(10, 0)), "in force at its start");
        assert!(w.covers(at(10, 59)));
        assert!(!w.covers(at(11, 0)), "not in force at its end");
        assert!(!w.covers(at(9, 59)));
    }

    #[test]
    fn is_ended_is_the_complement_of_covers_at_the_end() {
        let w = win((10, 0), (11, 0), "w");
        assert!(!w.is_ended(at(10, 0)));
        assert!(!w.is_ended(at(10, 59)));
        assert!(w.is_ended(at(11, 0)), "ended exactly at end");
        assert!(w.is_ended(at(12, 0)));
    }

    #[test]
    fn overlaps_a_span_shares_any_time_with_it() {
        let w = win((10, 0), (11, 0), "w");
        assert!(w.overlaps(at(9, 0), at(10, 30)), "straddles the start");
        assert!(w.overlaps(at(10, 30), at(12, 0)), "straddles the end");
        assert!(w.overlaps(at(9, 0), at(12, 0)), "contains the window");
        assert!(w.overlaps(at(10, 10), at(10, 20)), "inside the window");
    }

    #[test]
    fn overlaps_excludes_touching_spans() {
        let w = win((10, 0), (11, 0), "w");
        assert!(
            !w.overlaps(at(9, 0), at(10, 0)),
            "ends where the window starts"
        );
        assert!(
            !w.overlaps(at(11, 0), at(12, 0)),
            "starts where the window ends"
        );
    }

    #[test]
    fn overlaps_a_zero_length_span_is_the_instant() {
        let w = win((10, 0), (11, 0), "w");
        for t in [at(10, 0), at(10, 30), at(10, 59), at(11, 0), at(9, 59)] {
            assert_eq!(
                w.overlaps(t, t),
                w.covers(t),
                "overlaps(t, t) must agree with covers(t) at {t}"
            );
        }
    }

    #[test]
    fn covering_finds_the_window_in_force() {
        let windows = [win((10, 0), (11, 0), "a"), win((11, 0), (12, 0), "b")];
        assert_eq!(covering(&windows, at(10, 30)).unwrap().tag, "a");
        assert_eq!(
            covering(&windows, at(11, 0)).unwrap().tag,
            "b",
            "the boundary belongs to the later window"
        );
        assert!(covering(&windows, at(12, 0)).is_none());
        assert!(covering::<Win>(&[], at(10, 30)).is_none());
    }

    #[test]
    fn any_covering_answers_without_the_window() {
        let windows = [win((10, 0), (11, 0), "a")];
        assert!(any_covering(&windows, at(10, 0)));
        assert!(!any_covering(&windows, at(11, 0)));
        assert!(!any_covering::<Win>(&[], at(10, 0)));
    }
}
