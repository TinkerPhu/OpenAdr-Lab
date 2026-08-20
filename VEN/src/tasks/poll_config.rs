//! GB-09 — resolve per-VEN poll cadence + startup jitter from `Profile`
//! (the source of truth; real VENs are deployed one profile per instance),
//! with `Config`'s env vars as a test-only override on top.

use crate::config::Config;
use crate::profile::Profile;

pub(crate) struct ResolvedPollConfig {
    pub events_secs: u64,
    pub programs_secs: u64,
    pub reports_secs: u64,
    pub startup_jitter_fixed_pct: f64,
    pub startup_jitter_random_max_pct: f64,
}

pub(crate) fn resolve(cfg: &Config, profile: &Profile) -> ResolvedPollConfig {
    ResolvedPollConfig {
        events_secs: cfg
            .poll_events_secs_override
            .unwrap_or(profile.polling.events_secs),
        programs_secs: cfg
            .poll_programs_secs_override
            .unwrap_or(profile.polling.programs_secs),
        reports_secs: cfg
            .poll_reports_secs_override
            .unwrap_or(profile.polling.reports_secs),
        startup_jitter_fixed_pct: cfg
            .poll_startup_jitter_fixed_pct_override
            .unwrap_or(profile.polling.startup_jitter_fixed_pct),
        startup_jitter_random_max_pct: cfg
            .poll_startup_jitter_random_max_pct_override
            .unwrap_or(profile.polling.startup_jitter_random_max_pct),
    }
}

/// One-time startup delay (s), referenced to `events_secs` — all three poll
/// loops (events/programs/reports) share this single desync window, sized to
/// the more frequent VTN traffic. `fixed_pct` is deterministic; the random
/// component is a fresh uniform draw in `[0, random_max_pct]` from `rng`, so
/// a fleet of identical profiles still desyncs.
pub(crate) fn compute_startup_jitter_s(
    events_secs: u64,
    fixed_pct: f64,
    random_max_pct: f64,
    rng: &mut impl rand::Rng,
) -> f64 {
    let random_pct = if random_max_pct > 0.0 {
        rng.gen_range(0.0..=random_max_pct)
    } else {
        0.0
    };
    events_secs as f64 * (fixed_pct + random_pct) / 100.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::rngs::StdRng;
    use rand::SeedableRng;

    fn cfg_no_overrides() -> Config {
        Config {
            listen_addr: "0.0.0.0:8080".into(),
            vtn_base_url: "http://vtn".into(),
            client_id: "id".into(),
            client_secret: "secret".into(),
            ven_name: "ven-1".into(),
            poll_events_secs_override: None,
            poll_programs_secs_override: None,
            poll_reports_secs_override: None,
            poll_startup_jitter_fixed_pct_override: None,
            poll_startup_jitter_random_max_pct_override: None,
            persist_path: None,
            profile_path: None,
        }
    }

    #[test]
    fn resolve_uses_profile_values_when_no_override_set() {
        let cfg = cfg_no_overrides();
        let mut profile = Profile::default();
        profile.polling.events_secs = 900;
        profile.polling.programs_secs = 900;
        profile.polling.reports_secs = 900;
        profile.polling.startup_jitter_fixed_pct = 5.0;
        profile.polling.startup_jitter_random_max_pct = 10.0;

        let resolved = resolve(&cfg, &profile);
        assert_eq!(resolved.events_secs, 900);
        assert_eq!(resolved.programs_secs, 900);
        assert_eq!(resolved.reports_secs, 900);
        assert_eq!(resolved.startup_jitter_fixed_pct, 5.0);
        assert_eq!(resolved.startup_jitter_random_max_pct, 10.0);
    }

    #[test]
    fn resolve_override_wins_over_profile() {
        let mut cfg = cfg_no_overrides();
        cfg.poll_events_secs_override = Some(15);
        let mut profile = Profile::default();
        profile.polling.events_secs = 900;

        let resolved = resolve(&cfg, &profile);
        assert_eq!(resolved.events_secs, 15);
    }

    #[test]
    fn resolve_defaults_match_todays_behavior_when_nothing_set() {
        let cfg = cfg_no_overrides();
        let profile = Profile::default();

        let resolved = resolve(&cfg, &profile);
        assert_eq!(resolved.events_secs, 30);
        assert_eq!(resolved.programs_secs, 30);
        assert_eq!(resolved.reports_secs, 60);
        assert_eq!(resolved.startup_jitter_fixed_pct, 0.0);
        assert_eq!(resolved.startup_jitter_random_max_pct, 0.0);
    }

    #[test]
    fn compute_startup_jitter_s_fixed_only_is_exact() {
        let mut rng = StdRng::seed_from_u64(1);
        let delay = compute_startup_jitter_s(900, 5.0, 0.0, &mut rng);
        assert!((delay - 45.0).abs() < 1e-9, "got {delay}");
    }

    #[test]
    fn compute_startup_jitter_s_zero_zero_is_zero() {
        let mut rng = StdRng::seed_from_u64(1);
        let delay = compute_startup_jitter_s(900, 0.0, 0.0, &mut rng);
        assert_eq!(delay, 0.0);
    }

    #[test]
    fn compute_startup_jitter_s_random_component_stays_within_bound() {
        for seed in 0..50u64 {
            let mut rng = StdRng::seed_from_u64(seed);
            let delay = compute_startup_jitter_s(1000, 0.0, 10.0, &mut rng);
            // fixed_pct=0, so delay is entirely the random component in [0, 10]% of 1000s.
            assert!(
                (0.0..=100.0).contains(&delay),
                "delay {delay} outside [0, 100]s bound for events_secs=1000, random_max_pct=10"
            );
        }
    }

    #[test]
    fn compute_startup_jitter_s_scales_linearly_with_events_secs() {
        let mut rng = StdRng::seed_from_u64(1);
        let delay_900 = compute_startup_jitter_s(900, 5.0, 0.0, &mut rng);
        let mut rng = StdRng::seed_from_u64(1);
        let delay_1800 = compute_startup_jitter_s(1800, 5.0, 0.0, &mut rng);
        assert!((delay_1800 - delay_900 * 2.0).abs() < 1e-9);
    }
}
