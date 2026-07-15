//! WP5.2 (BL-14) — learn per-asset behavioral heuristics from history.
//!
//! `learn_asset_heuristics` is a pure `&dyn HistoryPort -> AssetHeuristics`
//! aggregation: two independent EWMA-recency-weighted mean-power-by-hour-of-day
//! passes, one fed by weekday ticks and one by weekend ticks, plus a
//! rolling-30-day seasonal factor. `generate_synthetic_backfill` is
//! the counterpart used both by the on-demand preload route
//! (`routes/debug.rs`) and this module's own tests, so the "looks like 4
//! weeks of real history" demonstration and the test assertions can never
//! silently diverge into two different algorithms.

use chrono::{DateTime, Datelike, Duration, Timelike, Utc};

use crate::assets::base_load::BaseLoad;
use crate::controller::HistoryPort;
use crate::entities::design_vocabulary::AssetHeuristics;
use crate::entities::history::TickSample;
use crate::entities::DomainError;

pub struct HeuristicsConfig {
    /// How far back to query history. Uses whatever's actually available
    /// when less exists (e.g. a freshly preloaded 4-week backfill).
    pub rolling_window_days: u32,
    /// EWMA half-life for recency weighting — a sample this many days old
    /// contributes half the weight of a sample from `now`.
    pub ewma_halflife_days: f64,
    /// Cold-start gate: fewer ticks than this and the job declines to
    /// produce a heuristic (`Ok(None)`), leaving LAST_KNOWN/flat fallback
    /// in place rather than fitting noise.
    pub min_samples_for_confidence: usize,
}

impl Default for HeuristicsConfig {
    fn default() -> Self {
        Self {
            rolling_window_days: 42,
            ewma_halflife_days: 14.0,
            min_samples_for_confidence: 100,
        }
    }
}

fn ewma_weight(age_days: f64, halflife_days: f64) -> f64 {
    0.5_f64.powf(age_days / halflife_days)
}

/// Learn a 2-bucket `daytime_profile_kw` (`[0]`=weekday, `[1]`=weekend) and
/// `seasonal_factor` for `asset_id` from `rolling_window_days` of history.
/// `Ok(None)` when fewer than `min_samples_for_confidence` ticks are
/// available (cold-start).
pub fn learn_asset_heuristics(
    history: &dyn HistoryPort,
    asset_id: &str,
    now: DateTime<Utc>,
    cfg: &HeuristicsConfig,
) -> Result<Option<AssetHeuristics>, DomainError> {
    let from = now - Duration::days(cfg.rolling_window_days as i64);
    let ticks = history.query_ticks(from, now, Some(asset_id))?;
    if ticks.len() < cfg.min_samples_for_confidence {
        return Ok(None);
    }

    // [0] = weekday (Mon-Fri) bucket, [1] = weekend (Sat/Sun) bucket.
    let mut hour_sum = [[0.0_f64; 24]; 2];
    let mut hour_weight = [[0.0_f64; 24]; 2];
    let mut overall_sum = 0.0_f64;
    let mut overall_weight = 0.0_f64;

    for t in &ticks {
        let age_days = (now - t.ts).num_seconds() as f64 / 86_400.0;
        let w = ewma_weight(age_days, cfg.ewma_halflife_days);
        let hour = t.ts.hour() as usize;
        let bucket = if matches!(t.ts.weekday(), chrono::Weekday::Sat | chrono::Weekday::Sun) {
            1
        } else {
            0
        };

        hour_sum[bucket][hour] += t.power_kw * w;
        hour_weight[bucket][hour] += w;
        overall_sum += t.power_kw * w;
        overall_weight += w;
    }

    let overall_mean = if overall_weight > 0.0 {
        overall_sum / overall_weight
    } else {
        0.0
    };

    let daytime_profile_kw: [Vec<f64>; 2] = std::array::from_fn(|bucket| {
        (0..24)
            .map(|h| {
                if hour_weight[bucket][h] > 0.0 {
                    hour_sum[bucket][h] / hour_weight[bucket][h]
                } else {
                    overall_mean
                }
            })
            .collect()
    });

    // Rolling 30-day mean vs. the full window's mean — with only 4 weeks of
    // seeded data this sits near 1.0, an honest limitation of short history.
    let thirty_days_ago = now - Duration::days(30);
    let (recent_sum, recent_weight) =
        ticks
            .iter()
            .filter(|t| t.ts >= thirty_days_ago)
            .fold((0.0_f64, 0.0_f64), |(s, w), t| {
                let age_days = (now - t.ts).num_seconds() as f64 / 86_400.0;
                let weight = ewma_weight(age_days, cfg.ewma_halflife_days);
                (s + t.power_kw * weight, w + weight)
            });
    let recent_mean = if recent_weight > 0.0 {
        recent_sum / recent_weight
    } else {
        overall_mean
    };
    let seasonal_factor = if overall_mean.abs() > 1e-9 {
        recent_mean / overall_mean
    } else {
        1.0
    };

    Ok(Some(AssetHeuristics {
        asset_id: asset_id.to_string(),
        daytime_profile_kw,
        seasonal_factor,
        last_updated: Some(now),
    }))
}

/// Generate synthetic backdated `TickSample`s for `asset_id`, at 1-minute
/// resolution (matching `history_sampler`'s production downsample grain),
/// sampling `power_kw_at` at each timestamp. Shared by the
/// `/debug/heuristics/preload` route and this module's own tests. Generic
/// over the power function rather than hardcoding `BaseLoad`'s formula so
/// callers can seed e.g. `site-residual` with its own (flat 0, per R-20)
/// model instead of silently reusing base_load's.
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
        });
        ts += Duration::minutes(1);
    }
    samples
}

/// `generate_synthetic_backfill`'s `power_kw_at` for `base_load` itself:
/// static baseline plus the configured appliance-noise model.
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
        let result = learn_asset_heuristics(&port, "base_load", now(), &cfg).unwrap();
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
        // R-20: site-residual has no independent meter-noise source in the
        // simulator, so its synthetic backfill must be exactly flat 0 —
        // not silently reuse base_load's appliance-noise formula.
        let rows =
            generate_synthetic_backfill("site-residual", now() - Duration::hours(1), now(), |_| {
                0.0
            });
        assert_eq!(rows.len(), 60);
        assert!(rows.iter().all(|r| r.power_kw == 0.0));
        assert!(rows.iter().all(|r| r.asset_id == "site-residual"));
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
        let heuristics = learn_asset_heuristics(&port, "base_load", end, &cfg)
            .unwrap()
            .expect("4 weeks of 1-min samples must clear the cold-start gate");

        assert_eq!(heuristics.asset_id, "base_load");
        assert_eq!(heuristics.daytime_profile_kw[0].len(), 24);
        assert_eq!(heuristics.daytime_profile_kw[1].len(), 24);

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
        // weekday bucket must peak near dinner and stay quiet at brunch
        // time, and vice versa for the weekend bucket.
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
        let start = end - Duration::days(28);
        let rows = generate_synthetic_backfill("base_load", start, end, base_load_power_kw_at(&bl));
        let port = MockHistoryPort::new();
        port.append_tick_samples(&rows).unwrap();

        let cfg = HeuristicsConfig::default();
        let heuristics = learn_asset_heuristics(&port, "base_load", end, &cfg)
            .unwrap()
            .expect("4 weeks of 1-min samples must clear the cold-start gate");

        let weekday_at_18 = heuristics.daytime_profile_kw[0][18];
        let weekday_at_10 = heuristics.daytime_profile_kw[0][10];
        let weekend_at_18 = heuristics.daytime_profile_kw[1][18];
        let weekend_at_10 = heuristics.daytime_profile_kw[1][10];

        assert!(
            weekday_at_18 > 1.0,
            "weekday bucket should show the dinner bump at 18:00, got {weekday_at_18}"
        );
        assert!(
            weekday_at_10 < 0.5,
            "weekday bucket should stay quiet at 10:00 (no brunch on weekdays), got {weekday_at_10}"
        );
        assert!(
            weekend_at_10 > 1.0,
            "weekend bucket should show the brunch bump at 10:00, got {weekend_at_10}"
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
        let heuristics = learn_asset_heuristics(&port, "base_load", end, &cfg)
            .unwrap()
            .unwrap();
        assert!(
            (heuristics.seasonal_factor - 1.0).abs() < 0.3,
            "seasonal_factor should stay near 1.0 for a stationary pattern, got {}",
            heuristics.seasonal_factor
        );
    }
}
