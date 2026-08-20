//! GB-09 — per-VEN VTN poll cadence + startup jitter — split out of
//! `schema.rs` to keep that file under the file-size cap.

use serde::Deserialize;

/// GB-09: per-VEN VTN poll cadence + startup jitter.
#[derive(Debug, Clone, Deserialize)]
pub struct PollConfig {
    #[serde(default = "super::defaults::default_poll_events_secs")]
    pub events_secs: u64,
    #[serde(default = "super::defaults::default_poll_programs_secs")]
    pub programs_secs: u64,
    #[serde(default = "super::defaults::default_poll_reports_secs")]
    pub reports_secs: u64,
    /// Deterministic startup-jitter component, as a percentage of
    /// `events_secs` — same value every time this VEN boots.
    #[serde(default)]
    pub startup_jitter_fixed_pct: f64,
    /// Upper bound for the randomized startup-jitter component, as a
    /// percentage of `events_secs` — actual value is a fresh uniform draw in
    /// [0, this] on every boot, so a fleet of identical profiles still
    /// desyncs.
    #[serde(default)]
    pub startup_jitter_random_max_pct: f64,
}
