//! Interval/timeseries builders for obligation-driven reports — extracted
//! from `reporter.rs` (which owns report assembly and dispatch).

use chrono::{DateTime, Duration, Utc};

use crate::common::{Aggregation, Interpolation, TimeSeries};
use crate::controller::reporter::{format_iso8601_duration, AssetReportSample};
use crate::controller::vtn_port::{OadrIntervalPeriod, OadrReportInterval, OadrReportPayload};
use crate::entities::design_vocabulary::AssetHeuristics;
use crate::entities::plan::Plan;

/// Build a `TimeSeries` of `power_kw` from a slice of `AssetReportSample`.
pub(crate) fn samples_to_power_ts(
    samples: &[AssetReportSample],
    interpolation: Interpolation,
) -> TimeSeries {
    let pts: Vec<(DateTime<Utc>, f64)> = samples.iter().map(|s| (s.ts, s.power_kw)).collect();
    TimeSeries {
        samples: pts,
        interpolation,
    }
}

/// Planned net site power per future plan slot at the plan's native slot
/// boundaries, payload-typed as `payload_type`. Empty when no plan is adopted
/// yet (caller re-arms and retries next cycle).
pub(crate) fn build_forecast_intervals(
    active_plan: Option<&Plan>,
    payload_type: &str,
) -> Vec<OadrReportInterval> {
    active_plan
        .map(|plan| {
            plan.slots
                .iter()
                .enumerate()
                .map(|(i, slot)| {
                    let net_w = (slot.net_import_kw - slot.net_export_kw) * 1000.0;
                    let slot_s = (slot.end - slot.start).num_seconds().max(0) as u64;
                    OadrReportInterval {
                        id: i,
                        intervalPeriod: Some(OadrIntervalPeriod {
                            start: Some(slot.start.to_rfc3339()),
                            duration: Some(format_iso8601_duration(slot_s)),
                        }),
                        payloads: vec![OadrReportPayload {
                            r#type: payload_type.to_string(),
                            values: vec![serde_json::Value::from(net_w)],
                        }],
                    }
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Sum all assets' `power_kw` into a single net site power `TimeSeries`.
///
/// Uses LOCF cross-asset aggregation: for each unique timestamp across all
/// asset sample vecs, sums each asset's `power_at(t)`.
pub(crate) fn build_net_site_power_ts(
    asset_samples: &std::collections::HashMap<String, Vec<AssetReportSample>>,
) -> TimeSeries {
    let mut per_asset: Vec<TimeSeries> = asset_samples
        .values()
        .map(|samples| samples_to_power_ts(samples, Interpolation::Step))
        .filter(|ts| !ts.samples.is_empty())
        .collect();

    if per_asset.is_empty() {
        return TimeSeries::empty(Interpolation::Step);
    }
    if per_asset.len() == 1 {
        return per_asset.remove(0);
    }

    // Collect all unique timestamps across all assets, then sum at each point
    let mut all_ts: Vec<DateTime<Utc>> = per_asset
        .iter()
        .flat_map(|s| s.samples.iter().map(|(t, _)| *t))
        .collect();
    all_ts.sort();
    all_ts.dedup();

    let samples: Vec<(DateTime<Utc>, f64)> = all_ts
        .iter()
        .map(|&t| {
            let sum: f64 = per_asset.iter().filter_map(|s| s.interpolate_at(t)).sum();
            (t, sum)
        })
        .collect();

    TimeSeries {
        samples,
        interpolation: Interpolation::Step,
    }
}

/// Build BASELINE report intervals (WP5.4): the heuristic forecast summed
/// across all known assets, sampled on the same interval grid the `USAGE`
/// report would use for this window — the grid's timestamps piggyback on
/// `asset_samples`' own time range (their `power_kw` values are irrelevant
/// here) so a concurrently-submitted BASELINE and USAGE report cover
/// identical windows for `kpi.py` to diff. `AssetHeuristics::sample_kw` is
/// event-blind by construction (see design.md's WP5.4 decision) — it never
/// saw the event, so no extra "subtract the event" step is needed. Quality
/// tag is always `"HEURISTIC"`: this only ever samples `AssetHeuristics`,
/// never the richer multi-source `AssetForecast` pipeline. Empty when there
/// are no asset samples yet (no grid to sample on).
pub(crate) fn build_baseline_report_intervals(
    asset_samples: &std::collections::HashMap<String, Vec<AssetReportSample>>,
    heuristics: &std::collections::HashMap<String, AssetHeuristics>,
    interval_width: Duration,
    duration_iso: &str,
) -> Vec<OadrReportInterval> {
    let net_power_ts = build_net_site_power_ts(asset_samples);
    let grid = net_power_ts.resample_uniform(interval_width, Aggregation::Mean);

    grid.samples
        .iter()
        .enumerate()
        .map(|(i, &(ts, _))| {
            let baseline_kw: f64 = heuristics.values().map(|h| h.sample_kw(ts)).sum();
            OadrReportInterval {
                id: i,
                intervalPeriod: Some(OadrIntervalPeriod {
                    start: Some(ts.to_rfc3339()),
                    duration: Some(duration_iso.to_string()),
                }),
                payloads: vec![
                    OadrReportPayload {
                        r#type: "BASELINE".to_string(),
                        values: vec![serde_json::Value::from(baseline_kw * 1000.0)],
                    },
                    OadrReportPayload {
                        r#type: "DATA_QUALITY".to_string(),
                        values: vec![serde_json::Value::from("HEURISTIC")],
                    },
                ],
            }
        })
        .collect()
}

/// Build SoC intervals using point-in-time sampling at interval ends.
pub(crate) fn build_soc_intervals(
    asset_samples: &std::collections::HashMap<String, Vec<AssetReportSample>>,
    interval_width: Duration,
    duration_iso: &str,
    op_state: &'static str,
) -> Vec<OadrReportInterval> {
    // Build SoC TimeSeries from the "ev" or "battery" sample vec.
    let make_soc_ts = |key: &str| -> Option<TimeSeries> {
        let samples = asset_samples.get(key)?;
        let pts: Vec<(DateTime<Utc>, f64)> = samples
            .iter()
            .filter_map(|s| s.soc.map(|soc| (s.ts, soc)))
            .collect();
        if pts.is_empty() {
            return None;
        }
        Some(TimeSeries {
            samples: pts,
            interpolation: Interpolation::Step,
        })
    };

    // Look for EV or battery SoC timeseries
    let soc_ts = make_soc_ts("ev").or_else(|| make_soc_ts("battery"));

    let soc_ts = match soc_ts {
        Some(ts) => ts,
        None => return Vec::new(),
    };

    // Build interval-end timestamps using the same grid alignment as resample_uniform
    let resampled_uniform = soc_ts.resample_uniform(interval_width, Aggregation::Mean);
    let interval_end_timestamps: Vec<DateTime<Utc>> = resampled_uniform
        .samples
        .iter()
        .map(|(t, _)| *t + interval_width)
        .collect();

    // Sample SoC at interval ends
    let soc_at_ends = soc_ts.resample_to_grid(&interval_end_timestamps);

    resampled_uniform
        .samples
        .iter()
        .enumerate()
        .map(|(i, (ts, _))| {
            let soc_value = soc_at_ends
                .samples
                .iter()
                .find(|(t, _)| *t == *ts + interval_width)
                .map(|(_, v)| *v)
                .unwrap_or(0.0);

            OadrReportInterval {
                id: i,
                intervalPeriod: Some(OadrIntervalPeriod {
                    start: Some(ts.to_rfc3339()),
                    duration: Some(duration_iso.to_string()),
                }),
                payloads: vec![
                    OadrReportPayload {
                        r#type: "STORAGE_CHARGE_LEVEL".to_string(),
                        values: vec![serde_json::Value::from(format!("{:.1}", soc_value * 100.0))],
                    },
                    OadrReportPayload {
                        r#type: "OPERATING_STATE".to_string(),
                        values: vec![serde_json::Value::from(op_state)],
                    },
                ],
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn samples_to_power_ts_basic() {
        let samples: Vec<AssetReportSample> = [(0i64, 1.0f64), (60, 2.0), (120, 3.0)]
            .iter()
            .map(|&(off_s, kw)| AssetReportSample {
                ts: chrono::Utc.timestamp_opt(1_700_000_000 + off_s, 0).unwrap(),
                power_kw: kw,
                soc: None,
            })
            .collect();
        let series = samples_to_power_ts(&samples, Interpolation::Step);
        assert_eq!(series.samples.len(), 3);
        assert!((series.samples[0].1 - 1.0).abs() < 1e-9);
        assert!((series.samples[2].1 - 3.0).abs() < 1e-9);
    }

    // ── build_baseline_report_intervals (WP5.4 BASELINE reports) ────────

    use crate::entities::design_vocabulary::AssetHeuristics;

    fn ts(offset_s: i64) -> DateTime<Utc> {
        chrono::Utc
            .timestamp_opt(1_700_000_000 + offset_s, 0)
            .unwrap()
    }

    /// Flat heuristic (same power every hour) so tests don't depend on the
    /// fixed epoch's weekday/hour bucket.
    fn flat_heuristic(kw: f64) -> AssetHeuristics {
        AssetHeuristics {
            asset_id: "x".to_string(),
            daytime_profile_kw: [vec![kw; 24], vec![kw; 24]],
            seasonal_factor: 1.0,
            last_updated: None,
        }
    }

    fn samples_at(offsets_s: &[i64]) -> Vec<AssetReportSample> {
        offsets_s
            .iter()
            .map(|&off_s| AssetReportSample {
                ts: ts(off_s),
                power_kw: 0.0, // irrelevant for BASELINE — only timestamps define the grid
                soc: None,
            })
            .collect()
    }

    #[test]
    fn build_baseline_report_intervals_uses_heuristic_not_measured_power() {
        let asset_samples: std::collections::HashMap<_, _> =
            [("site".to_string(), samples_at(&[0, 900, 1800]))]
                .into_iter()
                .collect();
        let heuristics: std::collections::HashMap<_, _> =
            [("base_load".to_string(), flat_heuristic(2.0))]
                .into_iter()
                .collect();

        let intervals = build_baseline_report_intervals(
            &asset_samples,
            &heuristics,
            Duration::seconds(900),
            "PT15M",
        );

        assert!(!intervals.is_empty());
        for iv in &intervals {
            let baseline = iv
                .payloads
                .iter()
                .find(|p| p.r#type == "BASELINE")
                .expect("BASELINE payload present");
            let val = baseline.values[0].as_f64().unwrap();
            assert!(
                (val - 2000.0).abs() < 1.0,
                "expected 2000 W (2 kW heuristic), got {val}"
            );
        }
    }

    #[test]
    fn build_baseline_report_intervals_sums_multiple_asset_heuristics() {
        let asset_samples: std::collections::HashMap<_, _> =
            [("site".to_string(), samples_at(&[0, 900]))]
                .into_iter()
                .collect();
        let heuristics: std::collections::HashMap<_, _> = [
            ("base_load".to_string(), flat_heuristic(1.5)),
            ("heater".to_string(), flat_heuristic(0.5)),
        ]
        .into_iter()
        .collect();

        let intervals = build_baseline_report_intervals(
            &asset_samples,
            &heuristics,
            Duration::seconds(900),
            "PT15M",
        );

        let val = intervals[0]
            .payloads
            .iter()
            .find(|p| p.r#type == "BASELINE")
            .unwrap()
            .values[0]
            .as_f64()
            .unwrap();
        assert!(
            (val - 2000.0).abs() < 1.0,
            "expected 2000 W (1.5+0.5 kW), got {val}"
        );
    }

    #[test]
    fn build_baseline_report_intervals_includes_quality_tag() {
        let asset_samples: std::collections::HashMap<_, _> =
            [("site".to_string(), samples_at(&[0, 900]))]
                .into_iter()
                .collect();
        let heuristics: std::collections::HashMap<_, _> =
            [("base_load".to_string(), flat_heuristic(1.0))]
                .into_iter()
                .collect();

        let intervals = build_baseline_report_intervals(
            &asset_samples,
            &heuristics,
            Duration::seconds(900),
            "PT15M",
        );

        let quality = intervals[0]
            .payloads
            .iter()
            .find(|p| p.r#type == "DATA_QUALITY")
            .expect("DATA_QUALITY payload present");
        assert_eq!(quality.values[0].as_str().unwrap(), "HEURISTIC");
    }

    #[test]
    fn build_baseline_report_intervals_empty_without_samples() {
        let asset_samples: std::collections::HashMap<String, Vec<AssetReportSample>> =
            std::collections::HashMap::new();
        let heuristics: std::collections::HashMap<_, _> =
            [("base_load".to_string(), flat_heuristic(1.0))]
                .into_iter()
                .collect();

        let intervals = build_baseline_report_intervals(
            &asset_samples,
            &heuristics,
            Duration::seconds(900),
            "PT15M",
        );
        assert!(
            intervals.is_empty(),
            "no asset samples -> no grid -> no baseline intervals"
        );
    }
}
