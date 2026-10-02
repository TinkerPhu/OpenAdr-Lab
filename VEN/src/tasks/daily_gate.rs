//! "Has a new UTC day started since I last asked?" — the once-per-day gate
//! the daily background jobs share.
//!
//! Existed twice as `day_boundary_crossed`, byte-identical, in
//! `tasks::history_sampler` and `tasks::heuristics_job`; the second one's own
//! doc comment said it mirrored the first. A third daily job would have
//! written a third.

use chrono::{DateTime, Utc};

const SECONDS_PER_DAY: i64 = 86_400;

/// Fires `true` exactly once per UTC calendar day, on the first call for that
/// day — including the very first call of the process.
///
/// That startup fire is deliberate and both existing callers want it:
/// retention pruning is idempotent so an immediate prune is desirable, and a
/// freshly preloaded history should get a heuristic on the next check rather
/// than after a full day's wait. A job that must *not* fire at startup (the
/// monthly ledger rollover) uses its own boundary check, not this.
#[derive(Default)]
pub(crate) struct DailyGate {
    last_day: Option<i64>,
}

impl DailyGate {
    pub(crate) fn crossed(&mut self, now: DateTime<Utc>) -> bool {
        let day = now.timestamp().div_euclid(SECONDS_PER_DAY);
        if self.last_day == Some(day) {
            false
        } else {
            self.last_day = Some(day);
            true
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn at(y: i32, m: u32, d: u32, h: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(y, m, d, h, 0, 0).unwrap()
    }

    #[test]
    fn fires_on_the_very_first_call() {
        assert!(DailyGate::default().crossed(at(2026, 5, 5, 13)));
    }

    #[test]
    fn does_not_fire_twice_within_one_utc_day() {
        let mut gate = DailyGate::default();
        assert!(gate.crossed(at(2026, 5, 5, 0)));
        assert!(!gate.crossed(at(2026, 5, 5, 12)));
        assert!(!gate.crossed(at(2026, 5, 5, 23)));
    }

    #[test]
    fn fires_again_on_the_next_utc_day() {
        let mut gate = DailyGate::default();
        assert!(gate.crossed(at(2026, 5, 5, 23)));
        assert!(gate.crossed(at(2026, 5, 6, 0)));
    }

    /// The day index is `floor(ts / 86400)`, which must stay correct for
    /// pre-epoch instants — `div_euclid`, not integer division.
    #[test]
    fn handles_a_pre_epoch_instant() {
        let mut gate = DailyGate::default();
        assert!(gate.crossed(at(1969, 12, 31, 23)));
        assert!(!gate.crossed(at(1969, 12, 31, 1)));
        assert!(gate.crossed(at(1970, 1, 1, 0)));
    }
}
