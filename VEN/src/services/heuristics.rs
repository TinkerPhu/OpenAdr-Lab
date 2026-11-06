//! WP5.2 (BL-14) — learn per-asset behavioral heuristics from history.
//!
//! `learn_asset_heuristics` is a pure `&dyn HistoryPort -> AssetHeuristics`
//! aggregation: seven independent EWMA-recency-weighted mean-power-by-hour-of-day
//! passes, one per day of the week (`chrono::Weekday::num_days_from_monday()`),
//! each shrunk toward the flat overall mean in proportion to how much data
//! that bucket/hour cell actually has, plus a rolling-30-day seasonal factor.
//! `generate_synthetic_backfill` is the test-only counterpart that feeds this module's
//! tests a "looks like 4 weeks of real history" series.

use chrono::{DateTime, Duration, Utc};

#[cfg(test)]
use crate::assets::base_load::BaseLoad;
use crate::controller::HistoryPort;
use crate::entities::design_vocabulary::AssetHeuristics;
#[cfg(test)]
use crate::entities::history::TickSample;
use crate::entities::DomainError;
use lab_forecast::base_load_learner::{learn_profile, LearnConfig, LearnedProfile, LoadSample};

/// The assets this pipeline learns a heuristic for. Base load is the site's
/// whole unmetered-consumption story (see
/// `docs/architecture/forecasting_model.md`); PV forecasting is WP5.3's job,
/// not this pipeline's.
///
/// One list, not one per caller: a second caller would otherwise declare a second.
pub const HEURISTIC_ASSET_IDS: [&str; 1] = [crate::ids::ASSET_BASE_LOAD];

#[derive(Clone, Copy)]
pub struct HeuristicsConfig {
    /// How far back to query history. Uses whatever's actually available
    /// when less exists (e.g. a freshly preloaded 4-week backfill). Kept
    /// well under ~91 days (one season) — the 30-day-vs-window
    /// `seasonal_factor` split below assumes the whole window is roughly
    /// one season; a longer window would blend seasons together.
    pub rolling_window_days: u32,
    /// EWMA half-life for recency weighting — a sample this many days old
    /// contributes half the weight of a sample from `now`.
    pub ewma_halflife_days: f64,
    /// Shrinkage strength, in day-equivalents of data, for blending a
    /// bucket/hour cell's own mean toward the flat `overall_mean`: a cell
    /// with `effective_days == shrinkage_k_days` of data is blended 50/50;
    /// far more data than this and the cell is mostly trusted on its own;
    /// far less (including zero) and it leans mostly on `overall_mean`. See
    /// `learn_asset_heuristics`'s profile-build step for the formula.
    pub shrinkage_k_days: f64,
    /// Cold-start gate: fewer ticks than this and the job declines to
    /// produce a heuristic (`Ok(None)`), leaving LAST_KNOWN/flat fallback
    /// in place rather than fitting noise. Global, not per-bucket — a
    /// specific day-of-week bucket can still be far sparser than the
    /// overall average without tripping this gate; thin buckets are instead
    /// protected by `shrinkage_k_days` above.
    pub min_samples_for_confidence: usize,
}

impl HeuristicsConfig {
    fn to_learn_config(self) -> LearnConfig {
        LearnConfig {
            rolling_window_days: self.rolling_window_days,
            ewma_halflife_days: self.ewma_halflife_days,
            shrinkage_k_days: self.shrinkage_k_days,
            min_samples_for_confidence: self.min_samples_for_confidence,
        }
    }
}

impl Default for HeuristicsConfig {
    fn default() -> Self {
        Self {
            rolling_window_days: 56,
            ewma_halflife_days: 28.0,
            shrinkage_k_days: 2.5,
            min_samples_for_confidence: 100,
        }
    }
}

/// Learn the `AssetHeuristics` of `asset_id` from `rolling_window_days` of history: the VEN's adapter over
/// `lab_forecast::base_load_learner::learn_profile`, the one implementation the forecaster also calls.
/// `Ok(None)` when fewer than `min_samples_for_confidence` ticks are available (cold-start).
///
/// `previous` (R-60): the heuristic in effect before this run, used for `recent_mean_abs_error_kw`.
pub fn learn_asset_heuristics(
    history: &dyn HistoryPort,
    asset_id: &str,
    now: DateTime<Utc>,
    cfg: &HeuristicsConfig,
    previous: Option<&AssetHeuristics>,
) -> Result<Option<AssetHeuristics>, DomainError> {
    let from = now - Duration::days(cfg.rolling_window_days as i64);
    let samples: Vec<LoadSample> = history
        .query_ticks(from, now, Some(asset_id))?
        .iter()
        .map(|t| LoadSample {
            ts: t.ts,
            power_kw: t.power_kw,
        })
        .collect();
    let previous = previous.map(|p| LearnedProfile {
        daytime_profile_kw: p.daytime_profile_kw.clone(),
        seasonal_factor: p.seasonal_factor,
        last_updated: p.last_updated,
        recent_mean_abs_error_kw: p.recent_mean_abs_error_kw,
    });
    Ok(
        learn_profile(&samples, now, &cfg.to_learn_config(), previous.as_ref()).map(|p| {
            AssetHeuristics {
                asset_id: asset_id.to_string(),
                daytime_profile_kw: p.daytime_profile_kw,
                seasonal_factor: p.seasonal_factor,
                last_updated: p.last_updated,
                recent_mean_abs_error_kw: p.recent_mean_abs_error_kw,
            }
        }),
    )
}

/// Generate synthetic backdated `TickSample`s for `asset_id`, at 1-minute
/// resolution (matching `history_sampler`'s production downsample grain),
/// sampling `power_kw_at` at each timestamp. Test-only fixture for this module's own tests
/// (the `/debug/heuristics/preload` route that also used it is gone). Generic
/// over the power function rather than hardcoding `BaseLoad`'s formula so
/// callers can seed any asset with its own model instead of silently
/// reusing base_load's.
#[cfg(test)]
pub fn generate_synthetic_backfill(
    asset_id: &str,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
    power_kw_at: impl Fn(DateTime<Utc>) -> f64,
) -> Vec<TickSample> {
    let mut samples = Vec::new();
    let mut ts = from;
    while ts < to {
        samples.push(TickSample {
            ts,
            asset_id: asset_id.to_string(),
            power_kw: power_kw_at(ts),
            soc_pct: None,
            temperature_c: None,
            generation_limit_kw: None,
            curtailment_source: None,
            plugged: None,
        });
        ts += Duration::minutes(1);
    }
    samples
}

/// `generate_synthetic_backfill`'s `power_kw_at` for `base_load` itself:
/// static baseline plus the configured appliance-noise model.
#[cfg(test)]
pub fn base_load_power_kw_at(base_load: &BaseLoad) -> impl Fn(DateTime<Utc>) -> f64 + '_ {
    move |ts| base_load.baseline_kw_profile + base_load.appliance_noise_kw(ts)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entities::asset_params::{ApplianceSpikeParams, BaseLoadParams};
    use crate::services::test_support::mock_history_port::MockHistoryPort;
    use chrono::TimeZone;

    fn coffee_spike() -> ApplianceSpikeParams {
        ApplianceSpikeParams {
            center_hour: 8.0,
            jitter_h: 0.05,
            amplitude_kw: 1.2,
            duration_h: 0.25,
            ramp_h: 0.03,
            probability: 1.0,
            weekdays: vec![],
        }
    }

    fn base_load_with_coffee() -> BaseLoad {
        BaseLoad::from_params(&BaseLoadParams {
            baseline_kw: 0.3,
            spikes: vec![coffee_spike()],
            ..BaseLoadParams::default()
        })
    }

    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 7, 14, 12, 0, 0).unwrap()
    }

    #[test]
    fn learn_asset_heuristics_cold_start_returns_none() {
        let port = MockHistoryPort::new();
        let cfg = HeuristicsConfig::default();
        let result = learn_asset_heuristics(&port, "base_load", now(), &cfg, None).unwrap();
        assert!(
            result.is_none(),
            "no history at all must decline (cold-start)"
        );
    }

    #[test]
    fn generate_synthetic_backfill_row_count_and_formula() {
        let bl = base_load_with_coffee();
        let from = now() - Duration::hours(1);
        let to = now();
        let rows = generate_synthetic_backfill("base_load", from, to, base_load_power_kw_at(&bl));

        assert_eq!(rows.len(), 60, "1 hour at 1-minute resolution = 60 rows");
        for r in &rows {
            let expected = bl.baseline_kw_profile + bl.appliance_noise_kw(r.ts);
            assert!(
                (r.power_kw - expected).abs() < 1e-12,
                "row power_kw must match appliance_noise_kw exactly, no duplicated formula"
            );
            assert_eq!(r.asset_id, "base_load");
        }
    }

    #[test]
    fn generate_synthetic_backfill_supports_a_flat_zero_power_fn() {
        // The power function is a caller-supplied parameter, not a hardcoded
        // reuse of base_load's appliance-noise formula — a flat-0 model must
        // round-trip exactly.
        let rows =
            generate_synthetic_backfill("flat_asset", now() - Duration::hours(1), now(), |_| 0.0);
        assert_eq!(rows.len(), 60);
        assert!(rows.iter().all(|r| r.power_kw == 0.0));
        assert!(rows.iter().all(|r| r.asset_id == "flat_asset"));
    }

    #[test]
    fn learn_asset_heuristics_converges_to_coffee_peak_from_synthetic_backfill() {
        let bl = base_load_with_coffee();
        let end = now();
        let start = end - Duration::days(28);
        let rows = generate_synthetic_backfill("base_load", start, end, base_load_power_kw_at(&bl));

        let port = MockHistoryPort::new();
        port.append_tick_samples(&rows).unwrap();

        let cfg = HeuristicsConfig::default();
        let heuristics = learn_asset_heuristics(&port, "base_load", end, &cfg, None)
            .unwrap()
            .expect("4 weeks of 1-min samples must clear the cold-start gate");

        assert_eq!(heuristics.asset_id, "base_load");
        for bucket in 0..7 {
            assert_eq!(heuristics.daytime_profile_kw[bucket].len(), 24);
        }

        // BL-14's own verify condition: the learned profile shows a clear
        // peak near the configured spike hour vs. a quiet hour. The coffee
        // spike fires every day (weekdays: []), so check the weekday bucket.
        let coffee_hour_kw = heuristics.daytime_profile_kw[0][8];
        let quiet_hour_kw = heuristics.daytime_profile_kw[0][3];
        assert!(
            coffee_hour_kw > quiet_hour_kw,
            "hour 8 ({coffee_hour_kw}) should exceed hour 3 ({quiet_hour_kw})"
        );
        // The 15-min-wide coffee pulse is centered exactly at 8:00, so only
        // its right half falls inside the [8:00, 9:00) hour bucket the
        // learned profile buckets by — the analytic bucket average is
        // baseline (0.3) + half the spike's energy over 1h (~0.13 kW), i.e.
        // ~0.44 kW, not the full-pulse-amplitude figure a wider (old
        // Gaussian) shape would have produced.
        assert!(
            coffee_hour_kw > 0.35,
            "hour 8 should show a real bump above the 0.3 kW static baseline, got {coffee_hour_kw}"
        );
        assert!(
            quiet_hour_kw < 0.4,
            "hour 3 should sit close to the 0.3 kW static baseline, got {quiet_hour_kw}"
        );
    }

    #[test]
    fn learn_asset_heuristics_captures_distinct_weekday_and_weekend_shapes() {
        // Weekday-only dinner at 18:00, weekend-only brunch at 10:30 — a
        // shape a single scaled curve could never represent. The learned
        // Monday bucket ([0]) must peak near dinner and stay quiet at brunch
        // time, and vice versa for the Saturday bucket ([5]).
        let weekday_dinner = ApplianceSpikeParams {
            center_hour: 18.0,
            jitter_h: 0.05,
            amplitude_kw: 2.5,
            duration_h: 0.75,
            ramp_h: 0.08,
            probability: 1.0,
            weekdays: vec![0, 1, 2, 3, 4],
        };
        let weekend_brunch = ApplianceSpikeParams {
            center_hour: 10.5,
            jitter_h: 0.05,
            amplitude_kw: 2.2,
            duration_h: 1.0,
            ramp_h: 0.1,
            probability: 1.0,
            weekdays: vec![5, 6],
        };
        let bl = BaseLoad::from_params(&BaseLoadParams {
            baseline_kw: 0.3,
            spikes: vec![weekday_dinner, weekend_brunch],
            ..BaseLoadParams::default()
        });

        let end = now();
        // 8 weeks, not 4: with the 7-way weekday split each individual
        // weekday bucket (e.g. Monday) only gets one instance per week of
        // history, so 4 weeks (4 Mondays) is too thin against the default
        // rolling_window_days=56/shrinkage_k_days=2.5 to clear shrinkage's
        // pull toward overall_mean and show a clean peak.
        let start = end - Duration::days(56);
        let rows = generate_synthetic_backfill("base_load", start, end, base_load_power_kw_at(&bl));
        let port = MockHistoryPort::new();
        port.append_tick_samples(&rows).unwrap();

        let cfg = HeuristicsConfig::default();
        let heuristics = learn_asset_heuristics(&port, "base_load", end, &cfg, None)
            .unwrap()
            .expect("8 weeks of 1-min samples must clear the cold-start gate");

        let weekday_at_18 = heuristics.daytime_profile_kw[0][18]; // Monday
        let weekday_at_10 = heuristics.daytime_profile_kw[0][10]; // Monday
        let weekend_at_18 = heuristics.daytime_profile_kw[5][18]; // Saturday
        let weekend_at_10 = heuristics.daytime_profile_kw[5][10]; // Saturday

        // Relative/shape checks, not absolute floors: with the 7-way split
        // a single weekday's bucket (e.g. just Mondays) has far fewer
        // effective samples than the old pooled Mon-Fri "weekday" bucket
        // did, so `shrinkage_blend` (further tempered by EWMA recency
        // weighting) deliberately pulls it toward `overall_mean` more than
        // the old discrete fallback would have — an absolute `> 1.0` floor
        // on the peak hour no longer holds even though the shape is
        // correctly learned. Same pattern as
        // `learn_asset_heuristics_distinguishes_individual_weekdays` above.
        assert!(
            weekday_at_18 > weekday_at_10,
            "weekday bucket should show the dinner bump at 18:00 above its own quiet 10:00, \
             got {weekday_at_18} vs {weekday_at_10}"
        );
        assert!(
            weekday_at_18 > weekend_at_18,
            "weekday bucket's 18:00 dinner bump should exceed the weekend bucket's quiet 18:00, \
             got {weekday_at_18} vs {weekend_at_18}"
        );
        assert!(
            weekday_at_10 < 0.5,
            "weekday bucket should stay quiet at 10:00 (no brunch on weekdays), got {weekday_at_10}"
        );
        assert!(
            weekend_at_10 > weekend_at_18,
            "weekend bucket should show the brunch bump at 10:00 above its own quiet 18:00, \
             got {weekend_at_10} vs {weekend_at_18}"
        );
        assert!(
            weekend_at_10 > weekday_at_10,
            "weekend bucket's 10:00 brunch bump should exceed the weekday bucket's quiet 10:00, \
             got {weekend_at_10} vs {weekday_at_10}"
        );
        assert!(
            weekend_at_18 < 0.5,
            "weekend bucket should stay quiet at 18:00 (no dinner spike configured on weekends here), got {weekend_at_18}"
        );
    }

    #[test]
    fn learn_asset_heuristics_seasonal_factor_near_one_for_stationary_pattern() {
        // The synthetic model has no long-run trend, so a stationary pattern
        // over 4 weeks should produce a seasonal_factor close to 1.0.
        let bl = base_load_with_coffee();
        let end = now();
        let start = end - Duration::days(28);
        let rows = generate_synthetic_backfill("base_load", start, end, base_load_power_kw_at(&bl));
        let port = MockHistoryPort::new();
        port.append_tick_samples(&rows).unwrap();

        let cfg = HeuristicsConfig::default();
        let heuristics = learn_asset_heuristics(&port, "base_load", end, &cfg, None)
            .unwrap()
            .unwrap();
        assert!(
            (heuristics.seasonal_factor - 1.0).abs() < 0.3,
            "seasonal_factor should stay near 1.0 for a stationary pattern, got {}",
            heuristics.seasonal_factor
        );
    }

    // ── R-60: recent_mean_abs_error_kw ──────────────────────────────────

    #[test]
    fn learn_asset_heuristics_recent_error_is_low_for_a_stationary_pattern() {
        let bl = base_load_with_coffee();
        let end1 = now();
        let start1 = end1 - Duration::days(28);
        let rows1 =
            generate_synthetic_backfill("base_load", start1, end1, base_load_power_kw_at(&bl));
        let port1 = MockHistoryPort::new();
        port1.append_tick_samples(&rows1).unwrap();
        let cfg = HeuristicsConfig::default();
        let heuristics1 = learn_asset_heuristics(&port1, "base_load", end1, &cfg, None)
            .unwrap()
            .expect("first run must clear cold-start");

        // Second run one day later, same stationary model.
        let end2 = end1 + Duration::days(1);
        let start2 = end2 - Duration::days(28);
        let rows2 =
            generate_synthetic_backfill("base_load", start2, end2, base_load_power_kw_at(&bl));
        let port2 = MockHistoryPort::new();
        port2.append_tick_samples(&rows2).unwrap();
        let heuristics2 =
            learn_asset_heuristics(&port2, "base_load", end2, &cfg, Some(&heuristics1))
                .unwrap()
                .expect("second run must clear cold-start");

        let err = heuristics2
            .recent_mean_abs_error_kw
            .expect("a previous heuristic was supplied, error must be computed");
        assert!(
            err < 0.2,
            "stationary pattern should yield a low recent error near the synthetic \
             model's own noise floor, got {err}"
        );
    }

    #[test]
    fn learn_asset_heuristics_recent_error_is_higher_after_a_step_change() {
        let bl_low = base_load_with_coffee(); // baseline 0.3
        let end1 = now();
        let start1 = end1 - Duration::days(28);
        let rows1 =
            generate_synthetic_backfill("base_load", start1, end1, base_load_power_kw_at(&bl_low));
        let port1 = MockHistoryPort::new();
        port1.append_tick_samples(&rows1).unwrap();
        let cfg = HeuristicsConfig::default();
        let heuristics1 = learn_asset_heuristics(&port1, "base_load", end1, &cfg, None)
            .unwrap()
            .expect("first run must clear cold-start");

        // Second run: same window length/config, but the site's baseline
        // load stepped up materially (e.g. a new always-on appliance) — the
        // heuristic learned on the older, lower-baseline portion should
        // predict poorly against it.
        let bl_high = BaseLoad::from_params(&BaseLoadParams {
            baseline_kw: 3.0,
            spikes: vec![coffee_spike()],
            ..BaseLoadParams::default()
        });
        let end2 = end1 + Duration::days(1);
        let start2 = end2 - Duration::days(28);
        let rows2 =
            generate_synthetic_backfill("base_load", start2, end2, base_load_power_kw_at(&bl_high));
        let port2 = MockHistoryPort::new();
        port2.append_tick_samples(&rows2).unwrap();
        let heuristics2 =
            learn_asset_heuristics(&port2, "base_load", end2, &cfg, Some(&heuristics1))
                .unwrap()
                .expect("second run must clear cold-start");

        // Upper bound asserted for the stationary case above — a step
        // change must clear it by a wide margin, not just nominally.
        let stationary_case_upper_bound_kw = 0.2;
        let err = heuristics2
            .recent_mean_abs_error_kw
            .expect("a previous heuristic was supplied, error must be computed");
        assert!(
            err > stationary_case_upper_bound_kw,
            "a step change in baseline load should yield a measurably higher recent \
             error than the stationary case, got {err}"
        );
    }

    #[test]
    fn sample_kw_output_unaffected_by_recent_mean_abs_error_kw_field() {
        // R-60's field is purely additive instrumentation — it must not
        // change `sample_kw`'s output for any existing input.
        let base = AssetHeuristics {
            asset_id: "base_load".to_string(),
            daytime_profile_kw: std::array::from_fn(|_| vec![0.5; 24]),
            seasonal_factor: 1.2,
            last_updated: None,
            recent_mean_abs_error_kw: None,
        };
        let with_error = AssetHeuristics {
            recent_mean_abs_error_kw: Some(1.5),
            ..base.clone()
        };
        let t = now();
        assert_eq!(base.sample_kw(t), with_error.sample_kw(t));
    }

    // ── 7-day-of-week buckets + continuous shrinkage ────────────────────

    #[test]
    fn learn_asset_heuristics_distinguishes_individual_weekdays() {
        // Tuesday-only vs Wednesday-only spikes at different hours — a
        // shape the old weekday/weekend split couldn't represent at all
        // (both would land in the same "weekday" bucket). Uses an 8-week
        // backfill so each individual weekday bucket gets enough instances.
        let tuesday_only = ApplianceSpikeParams {
            center_hour: 14.0,
            jitter_h: 0.05,
            amplitude_kw: 2.0,
            duration_h: 0.5,
            ramp_h: 0.05,
            probability: 1.0,
            weekdays: vec![1], // Tuesday
        };
        let wednesday_only = ApplianceSpikeParams {
            center_hour: 20.0,
            jitter_h: 0.05,
            amplitude_kw: 2.0,
            duration_h: 0.5,
            ramp_h: 0.05,
            probability: 1.0,
            weekdays: vec![2], // Wednesday
        };
        let bl = BaseLoad::from_params(&BaseLoadParams {
            baseline_kw: 0.3,
            spikes: vec![tuesday_only, wednesday_only],
            ..BaseLoadParams::default()
        });

        let end = now();
        let start = end - Duration::days(56);
        let rows = generate_synthetic_backfill("base_load", start, end, base_load_power_kw_at(&bl));
        let port = MockHistoryPort::new();
        port.append_tick_samples(&rows).unwrap();

        let cfg = HeuristicsConfig::default();
        let heuristics = learn_asset_heuristics(&port, "base_load", end, &cfg, None)
            .unwrap()
            .expect("8 weeks of 1-min samples must clear the cold-start gate");

        let tuesday_at_14 = heuristics.daytime_profile_kw[1][14];
        let wednesday_at_14 = heuristics.daytime_profile_kw[2][14];
        let tuesday_at_20 = heuristics.daytime_profile_kw[1][20];
        let wednesday_at_20 = heuristics.daytime_profile_kw[2][20];

        assert!(
            tuesday_at_14 > wednesday_at_14,
            "Tuesday bucket should show its own 14:00 spike, Wednesday shouldn't: \
             {tuesday_at_14} vs {wednesday_at_14}"
        );
        assert!(
            wednesday_at_20 > tuesday_at_20,
            "Wednesday bucket should show its own 20:00 spike, Tuesday shouldn't: \
             {wednesday_at_20} vs {tuesday_at_20}"
        );
    }
}
