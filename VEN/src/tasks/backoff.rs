//! Phase 2 (WP2.1, BL-03) — exponential backoff with jitter for the VTN poll
//! loops. On success the delay resets to `base_s`; on failure it doubles up
//! to `max_s`. Jitter is ±10% of the pre-jitter delay, drawn from a seeded
//! RNG so tests are exact (determinism rule: no unseeded randomness).
use std::time::Duration;

use chrono::{DateTime, Utc};
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};

use crate::state::AppState;

pub(crate) struct Backoff {
    base_s: u64,
    max_s: u64,
    current_s: u64,
    rng: StdRng,
}

impl Backoff {
    pub(crate) fn new(base_s: u64, max_s: u64, seed: u64) -> Self {
        Self {
            base_s,
            max_s,
            current_s: base_s,
            rng: StdRng::seed_from_u64(seed),
        }
    }

    /// Reset to the base interval after a successful poll.
    pub(crate) fn on_success(&mut self) {
        self.current_s = self.base_s;
    }

    /// Return the jittered delay for the *current* interval, then double the
    /// interval (capped at `max_s`) for the next failure.
    pub(crate) fn on_failure(&mut self) -> Duration {
        let delay = self.jittered(self.current_s);
        self.current_s = self.current_s.saturating_mul(2).min(self.max_s);
        delay
    }

    fn jittered(&mut self, base_s: u64) -> Duration {
        let jitter_frac: f64 = self.rng.gen_range(-0.1..=0.1);
        let secs = base_s as f64 * (1.0 + jitter_frac);
        Duration::from_secs_f64(secs.max(0.0))
    }
}

/// WP-T1 (`docs/history/project_journal.md, search "WP-T"`): `on_success`/`on_failure` plus
/// recording the outcome on `AppState` — kept here (not in `poll_events.rs`'s loop
/// body) so that file stays under the `tasks/` file-size cap.
pub(crate) async fn record_success(backoff: &mut Backoff, state: &AppState, now: DateTime<Utc>) {
    backoff.on_success();
    state.record_vtn_poll_success(now).await;
}

/// `on_failure` + recording the outcome + the retry sleep, all in one call so
/// callers don't need a separate `let delay = ...` binding.
///
/// WP-T4: also records an Event Log entry (`category: "vtn_connection"`) —
/// a sibling call, not a merged responsibility (see design.md D2); it lives
/// here rather than at the `poll_events.rs` call site because that file has
/// zero file-size-cap headroom.
pub(crate) async fn record_fail_sleep(
    backoff: &mut Backoff,
    state: &AppState,
    now: DateTime<Utc>,
    error: impl std::fmt::Display,
) {
    let delay = backoff.on_failure();
    let message = error.to_string();
    state
        .record_vtn_poll_failure(now, message.clone(), delay.as_secs_f64())
        .await;
    state.record_event(now, "vtn_connection", message).await;
    tokio::time::sleep(delay).await;
}

#[cfg(test)]
mod tests {
    use super::*;

    // WP-T4 (docs/history/project_journal.md, search "WP-T"): record_fail_sleep's sibling
    // event-log call.
    #[tokio::test]
    async fn record_fail_sleep_records_connection_failure_and_event_log_entry() {
        let mut b = Backoff::new(0, 1, 0);
        let state = AppState::new();
        let now = Utc::now();

        record_fail_sleep(&mut b, &state, now, "connection refused").await;

        let vtn = state.vtn_connection_status().await;
        assert_eq!(vtn.last_error, Some("connection refused".to_string()));

        let log = state.event_log_snapshot().await;
        assert_eq!(log.len(), 1);
        assert_eq!(log[0].category, "vtn_connection");
        assert_eq!(log[0].message, "connection refused");
    }

    #[test]
    fn test_backoff_doubles_on_consecutive_failures() {
        let mut b = Backoff::new(30, 900, 42);
        // Jitter is ±10%, so compare against the un-jittered doubling sequence
        // with tolerance rather than exact values.
        let expected = [30.0, 60.0, 120.0, 240.0, 480.0, 900.0, 900.0];
        for exp in expected {
            let d = b.on_failure().as_secs_f64();
            let tolerance = exp * 0.1 + 1e-9;
            assert!((d - exp).abs() <= tolerance, "expected ~{exp}s, got {d}s");
        }
    }

    #[test]
    fn test_backoff_resets_on_success() {
        let mut b = Backoff::new(30, 900, 1);
        b.on_failure();
        b.on_failure();
        b.on_failure();
        b.on_success();
        let d = b.on_failure().as_secs_f64();
        assert!(
            (d - 30.0).abs() <= 3.0,
            "expected ~30s after reset, got {d}s"
        );
    }

    #[test]
    fn test_backoff_caps_at_max() {
        let mut b = Backoff::new(30, 100, 7);
        for _ in 0..10 {
            b.on_failure();
        }
        let d = b.on_failure().as_secs_f64();
        assert!(d <= 110.0, "expected capped near 100s, got {d}s");
    }

    #[test]
    fn test_backoff_jitter_within_10_percent() {
        let mut b = Backoff::new(100, 900, 99);
        for _ in 0..50 {
            b.current_s = 100; // pin base so only jitter varies
            let d = b.jittered(100).as_secs_f64();
            assert!(
                (90.0..=110.0).contains(&d),
                "jittered delay {d} outside ±10% of 100s"
            );
        }
    }
}
