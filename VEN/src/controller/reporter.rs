/// Controller reporter: builds OpenADR VEN report payloads.
///
/// Two modes:
///   - Measurement reports (timer-driven): one TELEMETRY_USAGE report per active event.
///   - Status reports (event-driven): TELEMETRY_STATUS triggered by PlanCycle.
use chrono::{DateTime, Duration, Utc};
use tracing::debug;

use crate::common::{parse_iso8601_duration_secs, Aggregation, Interpolation, TimeSeries};
use crate::controller::simulator_port::SimSnapshot;
use crate::controller::trace::ControllerEvent;
use crate::controller::vtn_port::{
    OadrEvent, OadrIntervalPeriod, OadrReportBody, OadrReportInterval, OadrReportPayload,
    OadrReportResource,
};
use crate::entities::capacity::OadrReportObligation;
use crate::entities::plan::SiteFlexibilityEnvelope;

// ---------------------------------------------------------------------------
// Domain-side sample type — extracted at the infra boundary by callers.
// No infra types here; callers map HistoryPoint → AssetReportSample.
// ---------------------------------------------------------------------------

/// One per-asset history point, pre-extracted at the infra boundary.
pub struct AssetReportSample {
    pub ts: DateTime<Utc>,
    pub power_kw: f64,
    /// State of charge as fraction 0.0–1.0. None for non-storage assets.
    pub soc: Option<f64>,
}

// ---------------------------------------------------------------------------
// TimeSeries conversion helpers
// ---------------------------------------------------------------------------

/// Build a `TimeSeries` of `power_kw` from a slice of `AssetReportSample`.
fn samples_to_power_ts(samples: &[AssetReportSample], interpolation: Interpolation) -> TimeSeries {
    let pts: Vec<(DateTime<Utc>, f64)> = samples.iter().map(|s| (s.ts, s.power_kw)).collect();
    TimeSeries {
        samples: pts,
        interpolation,
    }
}

// ---------------------------------------------------------------------------
// Interval activity detection
// ---------------------------------------------------------------------------

/// Returns true if `event` has at least one interval that is currently active.
fn event_is_active(event: &OadrEvent, now: DateTime<Utc>) -> bool {
    if event.intervals.is_empty() {
        return false;
    }
    event.intervals.iter().any(|interval| {
        let ip = match interval.intervalPeriod.as_ref() {
            Some(ip) => ip,
            None => return true,
        };
        let start_str = match ip.start.as_deref() {
            Some(s) => s,
            None => return true,
        };
        let interval_start: DateTime<Utc> = match start_str.parse() {
            Ok(dt) => dt,
            Err(_) => return true,
        };
        let duration_secs = ip
            .duration
            .as_deref()
            .map(parse_iso8601_duration_secs)
            .unwrap_or(365 * 24 * 3_600);
        let interval_end = interval_start + Duration::seconds(duration_secs);
        interval_start <= now && now < interval_end
    })
}

// ---------------------------------------------------------------------------
// Measurement report (timer-driven, T046)
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Latest asset power helpers
// ---------------------------------------------------------------------------

/// Sum the most-recent `power_kw` across all assets that are currently importing
/// (positive power). Returns 0.0 if no assets are importing.
fn latest_net_import_kw(snap: &SimSnapshot) -> f64 {
    snap.assets
        .values()
        .map(|a| a.power_kw)
        .filter(|&kw| kw > 0.0)
        .sum()
}

/// Build a TELEMETRY_USAGE measurement report for a single active OpenADR event.
///
/// The report includes:
///   - Net site import power (grid_net_import_kw, pre-computed by the caller).
///   - OPERATING_STATE = "ACTIVE".
///   - STORAGE_CHARGE_LEVEL (EV SoC %) if EV samples are available.
///
/// Returns None if the event has no id or programID.
pub fn build_measurement_report(
    event: &OadrEvent,
    asset_samples: &std::collections::HashMap<String, Vec<AssetReportSample>>,
    grid_net_import_kw: f64,
    grid_net_export_kw: f64,
    ven_name: &str,
) -> Option<OadrReportBody> {
    let event_id = &event.id;
    let program_id = &event.programID;

    let report_name = format!("auto-{}-{}", ven_name, event_id);
    let resource_name = format!("{}-meter", ven_name);

    let net_import_w = grid_net_import_kw * 1000.0;

    // Extract the primary payload type from the event's first interval
    let payload_type = event
        .intervals
        .first()
        .and_then(|iv| iv.payloads.first())
        .map(|p| p.r#type.as_str())
        .unwrap_or("SIMPLE");

    let (report_type, report_value) = match payload_type {
        "IMPORT_CAPACITY_LIMIT" => ("USAGE", net_import_w),
        "EXPORT_CAPACITY_LIMIT" => {
            // grid_net_export_kw is only consumed in this arm
            ("USAGE", grid_net_export_kw * 1000.0)
        }
        "PRICE" => ("USAGE", net_import_w),
        "SIMPLE" => ("SIMPLE", 1.0),
        _ => ("USAGE", net_import_w),
    };

    let mut payloads = vec![
        OadrReportPayload {
            r#type: report_type.to_string(),
            values: vec![serde_json::Value::from(report_value)],
        },
        OadrReportPayload {
            r#type: "OPERATING_STATE".to_string(),
            values: vec![serde_json::Value::from("ACTIVE")],
        },
    ];

    // Add EV SoC if available
    if let Some(soc) = asset_samples
        .get("ev")
        .and_then(|v| v.last())
        .and_then(|s| s.soc)
    {
        payloads.push(OadrReportPayload {
            r#type: "STORAGE_CHARGE_LEVEL".to_string(),
            values: vec![serde_json::Value::from(format!("{:.1}", soc * 100.0))],
        });
    }

    let report = OadrReportBody {
        programID: program_id.clone(),
        eventID: Some(event_id.clone()),
        clientName: ven_name.to_string(),
        reportName: Some(report_name),
        resources: vec![OadrReportResource {
            resourceName: resource_name,
            intervals: vec![OadrReportInterval {
                id: 0,
                intervalPeriod: None,
                payloads,
            }],
        }],
    };

    debug!(
        report_name = report.reportName.as_deref().unwrap_or(""),
        event_id, report_type, report_value, "built measurement report"
    );
    Some(report)
}

/// Build measurement reports for all currently active events (timer-driven entry point).
pub fn build_measurement_reports_for_active_events(
    events: &[OadrEvent],
    asset_samples: &std::collections::HashMap<String, Vec<AssetReportSample>>,
    grid_net_import_kw: f64,
    grid_net_export_kw: f64,
    ven_name: &str,
    now: DateTime<Utc>,
) -> Vec<OadrReportBody> {
    let mut seen = std::collections::HashSet::new();
    let mut reports = Vec::new();

    for event in events {
        if !event_is_active(event, now) {
            continue;
        }
        // Skip events with reportDescriptors — those are handled by the obligation loop
        let has_descriptors = event
            .reportDescriptors
            .as_ref()
            .is_some_and(|arr| !arr.is_empty());
        if has_descriptors {
            debug!(
                event_id = %event.id,
                "timer-driven: skipping event with reportDescriptors"
            );
            continue;
        }
        if seen.insert(event.id.clone()) {
            debug!(event_id = %event.id, "timer-driven: building single-interval report");
            if let Some(report) = build_measurement_report(
                event,
                asset_samples,
                grid_net_import_kw,
                grid_net_export_kw,
                ven_name,
            ) {
                reports.push(report);
            }
        }
    }

    reports
}

// ---------------------------------------------------------------------------
// Obligation-driven measurement report (multi-interval, RF-05e)
// ---------------------------------------------------------------------------

/// Build a multi-interval measurement report for a single report obligation.
///
/// Resamples asset history onto obligation-interval boundaries using
/// `TimeSeries::resample_uniform()`. Each resampled bucket becomes one
/// interval entry in the report.
///
/// Payload type routing:
///   - USAGE / PRICE / IMPORT_CAPACITY_LIMIT → net site import power (time-weighted mean)
///   - EXPORT_CAPACITY_LIMIT → net site export power (time-weighted mean, absolute)
///   - STORAGE_CHARGE_STATE / STORAGE_CHARGE_LEVEL → EV SoC point-in-time at interval end
///
/// Returns None if the obligation has no event_id or program_id.
pub fn build_measurement_report_for_obligation(
    obligation: &OadrReportObligation,
    asset_samples: &std::collections::HashMap<String, Vec<AssetReportSample>>,
    ven_name: &str,
    site_envelope: Option<&SiteFlexibilityEnvelope>,
) -> Option<OadrReportBody> {
    let program_id = obligation.program_id.as_deref()?;
    let event_id = &obligation.event_id;

    let report_name = format!("ob-{}-{}-{}", ven_name, event_id, obligation.payload_type);
    let resource_name = format!("{}-meter", ven_name);
    let interval_width = Duration::seconds(obligation.interval_duration_s as i64);
    let duration_iso = format_iso8601_duration(obligation.interval_duration_s);

    // Build net site power TimeSeries (sum all assets' power_kw)
    let net_power_ts = build_net_site_power_ts(asset_samples);

    let payload_type = &obligation.payload_type;

    let intervals: Vec<OadrReportInterval> = match payload_type.as_str() {
        "STORAGE_CHARGE_STATE" | "STORAGE_CHARGE_LEVEL" => {
            build_soc_intervals(asset_samples, interval_width, &duration_iso)
        }
        "IMPORT_CAPACITY_RESERVATION" => {
            let up_w = site_envelope.map(|e| e.up_kw * 1000.0).unwrap_or(0.0);
            vec![OadrReportInterval {
                id: 0,
                intervalPeriod: None,
                payloads: vec![
                    OadrReportPayload {
                        r#type: "IMPORT_CAPACITY_RESERVATION".to_string(),
                        values: vec![serde_json::Value::from(up_w)],
                    },
                    OadrReportPayload {
                        r#type: "OPERATING_STATE".to_string(),
                        values: vec![serde_json::Value::from("ACTIVE")],
                    },
                ],
            }]
        }
        "EXPORT_CAPACITY_RESERVATION" => {
            let down_w = site_envelope.map(|e| e.down_kw * 1000.0).unwrap_or(0.0);
            vec![OadrReportInterval {
                id: 0,
                intervalPeriod: None,
                payloads: vec![
                    OadrReportPayload {
                        r#type: "EXPORT_CAPACITY_RESERVATION".to_string(),
                        values: vec![serde_json::Value::from(down_w)],
                    },
                    OadrReportPayload {
                        r#type: "OPERATING_STATE".to_string(),
                        values: vec![serde_json::Value::from("ACTIVE")],
                    },
                ],
            }]
        }
        _ => {
            let resampled = net_power_ts.resample_uniform(interval_width, Aggregation::Mean);
            resampled
                .samples
                .iter()
                .enumerate()
                .map(|(i, &(ts, value_kw))| {
                    let value_w = match payload_type.as_str() {
                        "EXPORT_CAPACITY_LIMIT" => (-value_kw).max(0.0) * 1000.0,
                        "IMPORT_CAPACITY_LIMIT" => value_kw.max(0.0) * 1000.0,
                        _ => value_kw.max(0.0) * 1000.0, // USAGE, PRICE, SIMPLE, etc.
                    };
                    let report_type = if payload_type == "SIMPLE" {
                        "SIMPLE"
                    } else {
                        "USAGE"
                    };
                    OadrReportInterval {
                        id: i,
                        intervalPeriod: Some(OadrIntervalPeriod {
                            start: Some(ts.to_rfc3339()),
                            duration: Some(duration_iso.clone()),
                        }),
                        payloads: vec![
                            OadrReportPayload {
                                r#type: report_type.to_string(),
                                values: vec![serde_json::Value::from(value_w)],
                            },
                            OadrReportPayload {
                                r#type: "OPERATING_STATE".to_string(),
                                values: vec![serde_json::Value::from("ACTIVE")],
                            },
                        ],
                    }
                })
                .collect()
        }
    };

    if intervals.is_empty() {
        return None;
    }

    let interval_count = intervals.len();
    let report = OadrReportBody {
        programID: program_id.to_string(),
        eventID: Some(event_id.clone()),
        clientName: ven_name.to_string(),
        reportName: Some(report_name),
        resources: vec![OadrReportResource {
            resourceName: resource_name,
            intervals,
        }],
    };

    debug!(
        report_name = report.reportName.as_deref().unwrap_or(""),
        event_id, interval_count, "built obligation measurement report"
    );
    Some(report)
}

/// Sum all assets' `power_kw` into a single net site power `TimeSeries`.
///
/// Uses LOCF cross-asset aggregation: for each unique timestamp across all
/// asset sample vecs, sums each asset's `power_at(t)`.
fn build_net_site_power_ts(
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

/// Build SoC intervals using point-in-time sampling at interval ends.
fn build_soc_intervals(
    asset_samples: &std::collections::HashMap<String, Vec<AssetReportSample>>,
    interval_width: Duration,
    duration_iso: &str,
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
                        values: vec![serde_json::Value::from("ACTIVE")],
                    },
                ],
            }
        })
        .collect()
}

/// Format seconds as minimal ISO 8601 duration string.
fn format_iso8601_duration(secs: u64) -> String {
    let h = secs / 3600;
    let m = (secs % 3600) / 60;
    let s = secs % 60;
    let mut result = "PT".to_string();
    if h > 0 {
        result.push_str(&format!("{h}H"));
    }
    if m > 0 {
        result.push_str(&format!("{m}M"));
    }
    if s > 0 || (h == 0 && m == 0) {
        result.push_str(&format!("{s}S"));
    }
    result
}

// ---------------------------------------------------------------------------
// Status report (event-driven, T047)
// ---------------------------------------------------------------------------

/// Build a TELEMETRY_STATUS report triggered by a `ControllerEvent`.
///
/// Only emits for the `PlanCycle` variant.
/// Returns None when no active OpenADR event provides a `programID` (VTN requires it)
/// or for unhandled event types.
pub fn build_status_report(
    event: &ControllerEvent,
    snap: &SimSnapshot,
    ven_name: &str,
    program_id: Option<&str>,
    _now: DateTime<Utc>,
) -> Option<OadrReportBody> {
    let program_id = program_id?;

    let (description, asset_id_opt): (String, Option<String>) = match event {
        ControllerEvent::PlanCycle {
            trigger_reason,
            total_slots,
            ..
        } => (
            format!("PlanCycle trigger={} slots={}", trigger_reason, total_slots),
            None,
        ),
        _ => return None,
    };

    let net_import_kw = latest_net_import_kw(snap);

    let resource_name = asset_id_opt
        .as_deref()
        .map(|id| format!("{}-{}", ven_name, id))
        .unwrap_or_else(|| format!("{}-site", ven_name));

    let report = OadrReportBody {
        programID: program_id.to_string(),
        eventID: None,
        clientName: ven_name.to_string(),
        reportName: Some(format!("status-{}", ven_name)),
        resources: vec![OadrReportResource {
            resourceName: resource_name,
            intervals: vec![OadrReportInterval {
                id: 0,
                intervalPeriod: None,
                payloads: vec![
                    OadrReportPayload {
                        r#type: "TELEMETRY_STATUS".to_string(),
                        values: vec![serde_json::Value::from(description)],
                    },
                    OadrReportPayload {
                        r#type: "USAGE".to_string(),
                        values: vec![serde_json::Value::from(net_import_kw * 1000.0)],
                    },
                ],
            }],
        }],
    };

    Some(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::controller::simulator_port::{AssetSnapshot, GridSnapshot};
    use std::collections::HashMap;
    use uuid::Uuid;

    // Fixed-epoch timestamp helper for deterministic tests.
    fn ts(offset_s: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(1_700_000_000 + offset_s, 0).unwrap()
    }

    /// Build `(id, Vec<AssetReportSample>)` from `(offset_s, power_kw)` pairs.
    fn make_samples(id: &str, rows: &[(i64, f64)]) -> (String, Vec<AssetReportSample>) {
        let samples = rows
            .iter()
            .map(|&(offset_s, power_kw)| AssetReportSample {
                ts: ts(offset_s),
                power_kw,
                soc: None,
            })
            .collect();
        (id.to_string(), samples)
    }

    /// Build `(id, Vec<AssetReportSample>)` from `(offset_s, power_kw, soc)` triples.
    fn make_ev_samples(id: &str, rows: &[(i64, f64, f64)]) -> (String, Vec<AssetReportSample>) {
        let samples = rows
            .iter()
            .map(|&(offset_s, power_kw, soc)| AssetReportSample {
                ts: ts(offset_s),
                power_kw,
                soc: Some(soc),
            })
            .collect();
        (id.to_string(), samples)
    }

    /// Build a `SimSnapshot` with given `(id, power_kw)` asset pairs.
    fn make_snap(assets: &[(&str, f64)]) -> SimSnapshot {
        SimSnapshot {
            ts: Utc::now(),
            grid: GridSnapshot {
                net_power_w: 0.0,
                voltage_v: 0.0,
                import_kwh: 0.0,
                export_kwh: 0.0,
            },
            assets: assets
                .iter()
                .map(|&(id, power_kw)| {
                    (
                        id.to_string(),
                        AssetSnapshot {
                            power_kw,
                            asset_type: "base_load".to_string(),
                            cap_max_import_kw: power_kw.max(0.0),
                            cap_max_export_kw: (-power_kw).max(0.0),
                            available_discharge_kwh: None,
                            available_charge_kwh: None,
                            default_setpoint_kw: power_kw,
                            setpoint_kw: power_kw,
                            values: HashMap::new(),
                        },
                    )
                })
                .collect(),
        }
    }

    fn make_obligation(
        event_id: &str,
        program_id: &str,
        payload_type: &str,
        interval_duration_s: u64,
    ) -> OadrReportObligation {
        OadrReportObligation {
            id: Uuid::new_v4(),
            event_id: event_id.to_string(),
            program_id: Some(program_id.to_string()),
            payload_type: payload_type.to_string(),
            reading_type: "DIRECT_READ".to_string(),
            resource_name: None,
            due_at: Utc::now(),
            interval_duration_s,
            fulfilled: false,
            created_at: Utc::now(),
        }
    }

    // ── samples_to_power_ts ───────────────────────────────────────

    #[test]
    fn samples_to_power_ts_basic() {
        let (_, samples) = make_samples("site", &[(0, 1.0), (60, 2.0), (120, 3.0)]);
        let series = samples_to_power_ts(&samples, Interpolation::Step);
        assert_eq!(series.samples.len(), 3);
        assert!((series.samples[0].1 - 1.0).abs() < 1e-9);
        assert!((series.samples[2].1 - 3.0).abs() < 1e-9);
    }

    // ── format_iso8601_duration ────────────────────────────────────

    #[test]
    fn format_duration_15min() {
        assert_eq!(format_iso8601_duration(900), "PT15M");
    }

    #[test]
    fn format_duration_1h() {
        assert_eq!(format_iso8601_duration(3600), "PT1H");
    }

    #[test]
    fn format_duration_1h30m() {
        assert_eq!(format_iso8601_duration(5400), "PT1H30M");
    }

    #[test]
    fn format_duration_0s() {
        assert_eq!(format_iso8601_duration(0), "PT0S");
    }

    // ── build_measurement_report_for_obligation ────────────────────

    #[test]
    fn obligation_report_multi_interval() {
        let asset_samples: HashMap<_, _> = [make_samples(
            "site",
            &[(0, 2.0), (900, 3.0), (1800, 4.0), (2700, 5.0)],
        )]
        .into_iter()
        .collect();

        let ob = make_obligation("ev1", "prog1", "USAGE", 900);
        let report =
            build_measurement_report_for_obligation(&ob, &asset_samples, "ven-1", None).unwrap();

        let intervals = &report.resources[0].intervals;
        assert!(
            intervals.len() >= 2,
            "expected multiple intervals, got {}",
            intervals.len()
        );
        for (i, iv) in intervals.iter().enumerate() {
            assert_eq!(iv.id, i);
            assert!(iv.intervalPeriod.as_ref().unwrap().start.is_some());
            assert_eq!(
                iv.intervalPeriod.as_ref().unwrap().duration.as_deref(),
                Some("PT15M")
            );
        }
        let payloads = &intervals[0].payloads;
        assert!(payloads.iter().any(|p| p.r#type == "USAGE"));
        assert!(payloads.iter().any(|p| p.r#type == "OPERATING_STATE"));
    }

    #[test]
    fn obligation_report_no_program_id_returns_none() {
        let ob = OadrReportObligation {
            id: Uuid::new_v4(),
            event_id: "e1".to_string(),
            program_id: None,
            payload_type: "USAGE".to_string(),
            reading_type: "DIRECT_READ".to_string(),
            resource_name: None,
            due_at: Utc::now(),
            interval_duration_s: 900,
            fulfilled: false,
            created_at: Utc::now(),
        };
        let empty: HashMap<String, Vec<AssetReportSample>> = HashMap::new();
        assert!(build_measurement_report_for_obligation(&ob, &empty, "ven-1", None).is_none());
    }

    #[test]
    fn obligation_report_empty_history_returns_none() {
        let ob = make_obligation("e1", "p1", "USAGE", 900);
        let empty: HashMap<String, Vec<AssetReportSample>> = HashMap::new();
        assert!(build_measurement_report_for_obligation(&ob, &empty, "ven-1", None).is_none());
    }

    // ── import/export split ────────────────────────────────────────

    #[test]
    fn obligation_report_import_clamps_negative_to_zero() {
        let asset_samples: HashMap<_, _> = [make_samples("pv", &[(0, -5.0), (900, -5.0)])]
            .into_iter()
            .collect();
        let ob = make_obligation("e1", "p1", "IMPORT_CAPACITY_LIMIT", 900);
        let report =
            build_measurement_report_for_obligation(&ob, &asset_samples, "ven-1", None).unwrap();
        let intervals = &report.resources[0].intervals;
        for iv in intervals {
            let usage = iv.payloads.iter().find(|p| p.r#type == "USAGE").unwrap();
            let val = usage.values[0].as_f64().unwrap();
            assert!(
                (val - 0.0).abs() < 1e-9,
                "import should be 0 for export power, got {val}"
            );
        }
    }

    #[test]
    fn obligation_report_export_uses_absolute_negative() {
        let asset_samples: HashMap<_, _> = [make_samples("pv", &[(0, -3.0), (900, -3.0)])]
            .into_iter()
            .collect();
        let ob = make_obligation("e1", "p1", "EXPORT_CAPACITY_LIMIT", 900);
        let report =
            build_measurement_report_for_obligation(&ob, &asset_samples, "ven-1", None).unwrap();
        let intervals = &report.resources[0].intervals;
        for iv in intervals {
            let usage = iv.payloads.iter().find(|p| p.r#type == "USAGE").unwrap();
            let val = usage.values[0].as_f64().unwrap();
            assert!(
                (val - 3000.0).abs() < 1.0,
                "export should be ~3000 W, got {val}"
            );
        }
    }

    // ── SoC point-in-time ──────────────────────────────────────────

    #[test]
    fn obligation_report_soc_point_in_time() {
        let asset_samples: HashMap<_, _> = [make_ev_samples(
            "ev",
            &[
                (0, 7.0, 0.2),
                (900, 7.0, 0.4),
                (1800, 7.0, 0.6),
                (2700, 7.0, 0.8),
            ],
        )]
        .into_iter()
        .collect();
        let ob = make_obligation("e1", "p1", "STORAGE_CHARGE_LEVEL", 900);
        let report =
            build_measurement_report_for_obligation(&ob, &asset_samples, "ven-1", None).unwrap();
        let intervals = &report.resources[0].intervals;
        assert!(!intervals.is_empty());
        for iv in intervals {
            let soc_payload = iv
                .payloads
                .iter()
                .find(|p| p.r#type == "STORAGE_CHARGE_LEVEL")
                .unwrap();
            let soc_pct: f64 = soc_payload.values[0].as_str().unwrap().parse().unwrap();
            assert!(
                soc_pct >= 20.0 && soc_pct <= 80.0,
                "SoC {soc_pct} out of range"
            );
        }
    }

    // ── build_net_site_power_ts ────────────────────────────────────

    #[test]
    fn net_site_power_sums_assets() {
        let asset_samples: HashMap<_, _> = [
            make_samples("ev", &[(0, 2.0), (60, 3.0)]),
            make_samples("pv", &[(0, -1.0), (60, 1.0)]),
        ]
        .into_iter()
        .collect();
        let net = build_net_site_power_ts(&asset_samples);
        assert_eq!(net.samples.len(), 2);
        assert!((net.samples[0].1 - 1.0).abs() < 1e-9); // 2 + (-1) = 1
        assert!((net.samples[1].1 - 4.0).abs() < 1e-9); // 3 + 1 = 4
    }

    #[test]
    fn net_site_power_empty_history() {
        let empty: HashMap<String, Vec<AssetReportSample>> = HashMap::new();
        let net = build_net_site_power_ts(&empty);
        assert!(net.samples.is_empty());
    }

    // ── latest_net_import_kw ───────────────────────────────────────

    #[test]
    fn latest_net_import_kw_sums_positive_assets() {
        let snap = make_snap(&[("a", 3.0), ("b", -2.0)]);
        assert!((latest_net_import_kw(&snap) - 3.0).abs() < 1e-9);
    }

    #[test]
    fn latest_net_import_kw_empty_history_returns_zero() {
        let snap = make_snap(&[]);
        assert_eq!(latest_net_import_kw(&snap), 0.0);
    }

    #[test]
    fn latest_net_import_kw_all_exporting_returns_zero() {
        let snap = make_snap(&[("pv", -5.0)]);
        assert_eq!(latest_net_import_kw(&snap), 0.0);
    }

    // ── IMPORT/EXPORT_CAPACITY_RESERVATION ────────────────────────

    #[test]
    fn test_reporter_import_capacity_reservation_from_envelope() {
        let empty: HashMap<String, Vec<AssetReportSample>> = HashMap::new();
        let env = crate::entities::plan::SiteFlexibilityEnvelope {
            ts: Utc::now(),
            up_kw: 5.0,
            down_kw: 3.0,
            up_duration_s: None,
            down_duration_s: None,
        };
        let ob = make_obligation("e1", "p1", "IMPORT_CAPACITY_RESERVATION", 900);
        let report = build_measurement_report_for_obligation(&ob, &empty, "ven-test", Some(&env))
            .expect("should return Some");
        let iv = &report.resources[0].intervals[0];
        let val = iv.payloads[0].values[0].as_f64().unwrap();
        assert!((val - 5000.0).abs() < 1.0, "expected 5000 W, got {val}");
        assert_eq!(iv.payloads[0].r#type, "IMPORT_CAPACITY_RESERVATION");
    }

    #[test]
    fn test_reporter_export_capacity_reservation_from_envelope() {
        let empty: HashMap<String, Vec<AssetReportSample>> = HashMap::new();
        let env = crate::entities::plan::SiteFlexibilityEnvelope {
            ts: Utc::now(),
            up_kw: 5.0,
            down_kw: 3.0,
            up_duration_s: None,
            down_duration_s: None,
        };
        let ob = make_obligation("e1", "p1", "EXPORT_CAPACITY_RESERVATION", 900);
        let report = build_measurement_report_for_obligation(&ob, &empty, "ven-test", Some(&env))
            .expect("should return Some");
        let iv = &report.resources[0].intervals[0];
        let val = iv.payloads[0].values[0].as_f64().unwrap();
        assert!((val - 3000.0).abs() < 1.0, "expected 3000 W, got {val}");
        assert_eq!(iv.payloads[0].r#type, "EXPORT_CAPACITY_RESERVATION");
    }

    #[test]
    fn test_reporter_capacity_reservation_no_envelope_returns_zero() {
        let empty: HashMap<String, Vec<AssetReportSample>> = HashMap::new();
        let ob = make_obligation("e1", "p1", "IMPORT_CAPACITY_RESERVATION", 900);
        let report = build_measurement_report_for_obligation(&ob, &empty, "ven-test", None)
            .expect("should return Some even with no envelope");
        let val = report.resources[0].intervals[0].payloads[0].values[0]
            .as_f64()
            .unwrap();
        assert_eq!(val, 0.0, "expected 0.0 W when no envelope");
    }

    // ── build_measurement_report ────────────────────────────────────

    #[test]
    fn measurement_report_fields_match_event() {
        use crate::controller::vtn_port::{OadrInterval, OadrPayload};
        let event = OadrEvent {
            id: "evt-001".to_string(),
            programID: "prog-001".to_string(),
            eventName: None,
            priority: None,
            intervalPeriod: None,
            intervals: vec![OadrInterval {
                intervalPeriod: None,
                payloads: vec![OadrPayload {
                    r#type: "USAGE".to_string(),
                    values: vec![],
                }],
            }],
            reportDescriptors: None,
        };
        let asset_samples: HashMap<_, _> =
            [make_samples("site", &[(0, 3.0)])].into_iter().collect();
        let report = build_measurement_report(&event, &asset_samples, 3.0, 0.0, "ven-1").unwrap();
        assert_eq!(report.programID, "prog-001");
        assert_eq!(report.eventID.as_deref(), Some("evt-001"));
        assert_eq!(report.clientName, "ven-1");
        assert_eq!(report.reportName.as_deref(), Some("auto-ven-1-evt-001"));
        assert_eq!(report.resources[0].resourceName, "ven-1-meter");
        let iv = &report.resources[0].intervals[0];
        assert_eq!(iv.id, 0);
        assert!(iv.intervalPeriod.is_none());
        let usage = iv.payloads.iter().find(|p| p.r#type == "USAGE").unwrap();
        let val = usage.values[0].as_f64().unwrap();
        assert!((val - 3000.0).abs() < 1.0, "expected 3000 W, got {val}");
        assert!(iv.payloads.iter().any(|p| p.r#type == "OPERATING_STATE"));
    }

    #[test]
    fn measurement_report_includes_ev_soc_when_available() {
        use crate::controller::vtn_port::{OadrInterval, OadrPayload};
        let event = OadrEvent {
            id: "evt-002".to_string(),
            programID: "prog-001".to_string(),
            eventName: None,
            priority: None,
            intervalPeriod: None,
            intervals: vec![OadrInterval {
                intervalPeriod: None,
                payloads: vec![OadrPayload {
                    r#type: "USAGE".to_string(),
                    values: vec![],
                }],
            }],
            reportDescriptors: None,
        };
        let asset_samples: HashMap<_, _> = [make_ev_samples("ev", &[(0, 7.0, 0.5)])]
            .into_iter()
            .collect();
        let report = build_measurement_report(&event, &asset_samples, 0.0, 0.0, "ven-1").unwrap();
        let iv = &report.resources[0].intervals[0];
        let soc_payload = iv
            .payloads
            .iter()
            .find(|p| p.r#type == "STORAGE_CHARGE_LEVEL");
        assert!(soc_payload.is_some(), "expected SoC payload for EV");
        let soc_str = soc_payload.unwrap().values[0].as_str().unwrap();
        let soc_pct: f64 = soc_str.parse().unwrap();
        assert!((soc_pct - 50.0).abs() < 0.2, "expected ~50%, got {soc_pct}");
    }

    // ── build_measurement_reports_for_active_events ────────────────

    #[test]
    fn active_events_returns_empty_for_no_events() {
        let empty: HashMap<String, Vec<AssetReportSample>> = HashMap::new();
        let reports =
            build_measurement_reports_for_active_events(&[], &empty, 0.0, 0.0, "ven-1", Utc::now());
        assert!(reports.is_empty());
    }

    #[test]
    fn active_events_skips_events_with_report_descriptors() {
        use crate::controller::vtn_port::{OadrInterval, OadrPayload, OadrReportDescriptor};
        let event = OadrEvent {
            id: "evt-003".to_string(),
            programID: "prog-001".to_string(),
            eventName: None,
            priority: None,
            intervalPeriod: None,
            intervals: vec![OadrInterval {
                intervalPeriod: None,
                payloads: vec![OadrPayload {
                    r#type: "USAGE".to_string(),
                    values: vec![],
                }],
            }],
            reportDescriptors: Some(vec![OadrReportDescriptor {
                payloadType: "USAGE".to_string(),
                readingType: None,
                frequency: Some(900),
            }]),
        };
        let empty: HashMap<String, Vec<AssetReportSample>> = HashMap::new();
        let reports = build_measurement_reports_for_active_events(
            &[event],
            &empty,
            0.0,
            0.0,
            "ven-1",
            Utc::now(),
        );
        assert!(
            reports.is_empty(),
            "events with reportDescriptors should be skipped"
        );
    }

    // ── build_status_report ────────────────────────────────────────

    #[test]
    fn status_report_plan_cycle_fields() {
        use crate::controller::trace::ControllerEvent;
        let event = ControllerEvent::PlanCycle {
            ts: Utc::now(),
            trigger_reason: "Periodic".to_string(),
            total_slots: 288,
        };
        let snap = make_snap(&[("site", 2.0)]);
        let report =
            build_status_report(&event, &snap, "ven-1", Some("prog-001"), Utc::now()).unwrap();
        assert_eq!(report.programID, "prog-001");
        assert!(report.eventID.is_none(), "status report must omit eventID");
        assert_eq!(report.clientName, "ven-1");
        assert_eq!(report.reportName.as_deref(), Some("status-ven-1"));
        assert_eq!(report.resources[0].resourceName, "ven-1-site");
        let iv = &report.resources[0].intervals[0];
        assert_eq!(iv.id, 0);
        let status_payload = iv
            .payloads
            .iter()
            .find(|p| p.r#type == "TELEMETRY_STATUS")
            .unwrap();
        let desc = status_payload.values[0].as_str().unwrap();
        assert!(
            desc.contains("PlanCycle"),
            "expected PlanCycle in description, got: {desc}"
        );
        assert!(iv.payloads.iter().any(|p| p.r#type == "USAGE"));
    }

    #[test]
    fn status_report_no_program_id_returns_none() {
        use crate::controller::trace::ControllerEvent;
        let event = ControllerEvent::PlanCycle {
            ts: Utc::now(),
            trigger_reason: "Periodic".to_string(),
            total_slots: 288,
        };
        let snap = make_snap(&[]);
        assert!(build_status_report(&event, &snap, "ven-1", None, Utc::now()).is_none());
    }

    // ── SC-004: build_measurement_report callable without SimState ─

    #[test]
    fn build_measurement_report_domain_only() {
        use crate::controller::vtn_port::{OadrInterval, OadrPayload};
        let event = OadrEvent {
            id: "evt-sc004".to_string(),
            programID: "prog-001".to_string(),
            eventName: None,
            priority: None,
            intervalPeriod: None,
            intervals: vec![OadrInterval {
                intervalPeriod: None,
                payloads: vec![OadrPayload {
                    r#type: "USAGE".to_string(),
                    values: vec![],
                }],
            }],
            reportDescriptors: None,
        };
        let asset_samples: HashMap<_, _> = [make_samples("site", &[(0, 1.0), (60, 3.0)])]
            .into_iter()
            .collect();
        let report = build_measurement_report(&event, &asset_samples, 3.0, 0.0, "ven-1");
        assert!(report.is_some(), "expected Some(report)");
        let report = report.unwrap();
        assert_eq!(report.programID, "prog-001");
        assert_eq!(report.clientName, "ven-1");
        let usage = report.resources[0].intervals[0]
            .payloads
            .iter()
            .find(|p| p.r#type == "USAGE")
            .unwrap();
        let val = usage.values[0].as_f64().unwrap();
        assert!((val - 3000.0).abs() < 1.0, "expected 3000 W, got {val}");
    }

    // ── SC-005: build_status_report callable without SimState ─────

    #[test]
    fn build_status_report_domain_only() {
        use crate::controller::trace::ControllerEvent;
        let event = ControllerEvent::PlanCycle {
            ts: Utc::now(),
            trigger_reason: "Periodic".to_string(),
            total_slots: 288,
        };
        let snap = make_snap(&[("site", 2.0)]);
        let report = build_status_report(&event, &snap, "ven-1", Some("prog-001"), Utc::now());
        assert!(report.is_some(), "expected Some(report)");
        let report = report.unwrap();
        assert_eq!(report.programID, "prog-001");
        assert_eq!(report.clientName, "ven-1");
        let iv = &report.resources[0].intervals[0];
        let status_payload = iv
            .payloads
            .iter()
            .find(|p| p.r#type == "TELEMETRY_STATUS")
            .unwrap();
        assert!(
            status_payload.values[0]
                .as_str()
                .unwrap()
                .contains("PlanCycle"),
            "expected PlanCycle in status description"
        );
        let usage_payload = iv.payloads.iter().find(|p| p.r#type == "USAGE").unwrap();
        let val = usage_payload.values[0].as_f64().unwrap();
        assert!((val - 2000.0).abs() < 1.0, "expected 2000 W, got {val}");
    }
}
