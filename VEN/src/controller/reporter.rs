/// Controller reporter: builds OpenADR VEN report payloads.
///
/// Two modes:
///   - Timer-driven measurement reports: one TELEMETRY_USAGE report per active event.
///   - Obligation-driven measurement reports: multi-interval reports resampled onto a
///     report obligation's interval grid.
use chrono::{DateTime, Duration, Utc};
use tracing::debug;

use crate::common::{parse_iso8601_duration_secs, Aggregation};
use crate::controller::report_intervals::{
    build_forecast_intervals, build_net_site_power_ts, build_soc_intervals,
};
use crate::controller::vtn_port::{
    OadrEvent, OadrIntervalPeriod, OadrReportBody, OadrReportInterval, OadrReportPayload,
    OadrReportResource,
};
use crate::entities::capacity::OadrReportObligation;
use crate::entities::plan::{Plan, SiteFlexibilityEnvelope};

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

/// Site samples younger than this count as "device communicating normally".
/// Two minutes covers several report/sim ticks in every profile.
const OPERATING_STATE_STALENESS_S: i64 = 120;

/// Derive the site-level OPERATING_STATE from sample freshness (WP3.6 —
/// replaces the previously hardcoded "ACTIVE"): any sample within
/// `OPERATING_STATE_STALENESS_S` of `now` → "ACTIVE"; samples exist but all
/// stale → "UNRESPONSIVE"; no samples at all → "OFFLINE". Site-granularity
/// mirror of the per-device `DeviceResponsiveness` vocabulary
/// (`entities/asset.rs`).
fn operating_state(
    asset_samples: &std::collections::HashMap<String, Vec<AssetReportSample>>,
    now: DateTime<Utc>,
) -> &'static str {
    let newest = asset_samples
        .values()
        .flat_map(|v| v.iter().map(|s| s.ts))
        .max();
    match newest {
        Some(ts) if (now - ts).num_seconds() <= OPERATING_STATE_STALENESS_S => "ACTIVE",
        Some(_) => "UNRESPONSIVE",
        None => "OFFLINE",
    }
}

/// Build a TELEMETRY_USAGE measurement report for a single active OpenADR event.
///
/// The report includes:
///   - Net site import power (grid_net_import_kw, pre-computed by the caller).
///   - OPERATING_STATE derived from sample freshness (see `operating_state`).
///   - STORAGE_CHARGE_LEVEL (EV SoC %) if EV samples are available.
///
/// Returns None if the event has no id or programID.
pub fn build_measurement_report(
    event: &OadrEvent,
    asset_samples: &std::collections::HashMap<String, Vec<AssetReportSample>>,
    grid_net_import_kw: f64,
    grid_net_export_kw: f64,
    ven_name: &str,
    now: DateTime<Utc>,
) -> Option<OadrReportBody> {
    let event_id = &event.id;
    let program_id = &event.programID;
    let op_state = operating_state(asset_samples, now);

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
            values: vec![serde_json::Value::from(op_state)],
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
                now,
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
///   - USAGE_FORECAST (WP3.6, §8.8) → planned net site power per future plan
///     slot, straight from the active plan (None if no plan adopted yet)
///
/// Returns None if the obligation has no event_id or program_id.
pub fn build_measurement_report_for_obligation(
    obligation: &OadrReportObligation,
    asset_samples: &std::collections::HashMap<String, Vec<AssetReportSample>>,
    ven_name: &str,
    site_envelope: Option<&SiteFlexibilityEnvelope>,
    active_plan: Option<&Plan>,
    now: DateTime<Utc>,
) -> Option<OadrReportBody> {
    let op_state = operating_state(asset_samples, now);
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
            build_soc_intervals(asset_samples, interval_width, &duration_iso, op_state)
        }
        // WP3.6 (§8.8): planned net site power per future plan slot, at the
        // plan's own native slot boundaries (not the obligation's interval
        // width — a forecast has no meaning resampled onto history buckets).
        // No adopted plan yet → empty → caller re-arms and retries next cycle.
        "USAGE_FORECAST" => build_forecast_intervals(active_plan, "USAGE_FORECAST"),
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
                        values: vec![serde_json::Value::from(op_state)],
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
                        values: vec![serde_json::Value::from(op_state)],
                    },
                ],
            }]
        }
        // R-15: `reportDescriptor.historical: false` asks for a forecast of the
        // requested payload — serve plan slots instead of measured history.
        _ if !obligation.historical => build_forecast_intervals(active_plan, payload_type),
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
                                values: vec![serde_json::Value::from(op_state)],
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

/// Format seconds as minimal ISO 8601 duration string.
pub(crate) fn format_iso8601_duration(secs: u64) -> String {
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

#[cfg(test)]
mod tests {
    use super::*;
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
            historical: true,
        }
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
        let report = build_measurement_report_for_obligation(
            &ob,
            &asset_samples,
            "ven-1",
            None,
            None,
            ts(1800),
        )
        .unwrap();

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
            historical: true,
        };
        let empty: HashMap<String, Vec<AssetReportSample>> = HashMap::new();
        assert!(build_measurement_report_for_obligation(
            &ob,
            &empty,
            "ven-1",
            None,
            None,
            Utc::now()
        )
        .is_none());
    }

    #[test]
    fn obligation_report_empty_history_returns_none() {
        let ob = make_obligation("e1", "p1", "USAGE", 900);
        let empty: HashMap<String, Vec<AssetReportSample>> = HashMap::new();
        assert!(build_measurement_report_for_obligation(
            &ob,
            &empty,
            "ven-1",
            None,
            None,
            Utc::now()
        )
        .is_none());
    }

    // ── import/export split ────────────────────────────────────────

    #[test]
    fn obligation_report_import_clamps_negative_to_zero() {
        let asset_samples: HashMap<_, _> = [make_samples("pv", &[(0, -5.0), (900, -5.0)])]
            .into_iter()
            .collect();
        let ob = make_obligation("e1", "p1", "IMPORT_CAPACITY_LIMIT", 900);
        let report = build_measurement_report_for_obligation(
            &ob,
            &asset_samples,
            "ven-1",
            None,
            None,
            ts(1800),
        )
        .unwrap();
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
        let report = build_measurement_report_for_obligation(
            &ob,
            &asset_samples,
            "ven-1",
            None,
            None,
            ts(1800),
        )
        .unwrap();
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
        let report = build_measurement_report_for_obligation(
            &ob,
            &asset_samples,
            "ven-1",
            None,
            None,
            ts(1800),
        )
        .unwrap();
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
                (20.0..=80.0).contains(&soc_pct),
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
        let report = build_measurement_report_for_obligation(
            &ob,
            &empty,
            "ven-test",
            Some(&env),
            None,
            Utc::now(),
        )
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
        let report = build_measurement_report_for_obligation(
            &ob,
            &empty,
            "ven-test",
            Some(&env),
            None,
            Utc::now(),
        )
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
        let report = build_measurement_report_for_obligation(
            &ob,
            &empty,
            "ven-test",
            None,
            None,
            Utc::now(),
        )
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
            createdDateTime: None,
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
        let report =
            build_measurement_report(&event, &asset_samples, 3.0, 0.0, "ven-1", Utc::now())
                .unwrap();
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
            createdDateTime: None,
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
        let report =
            build_measurement_report(&event, &asset_samples, 0.0, 0.0, "ven-1", Utc::now())
                .unwrap();
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
            createdDateTime: None,
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
                historical: None,
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

    // ── SC-004: build_measurement_report callable without SimState ─

    #[test]
    fn build_measurement_report_domain_only() {
        use crate::controller::vtn_port::{OadrInterval, OadrPayload};
        let event = OadrEvent {
            id: "evt-sc004".to_string(),
            programID: "prog-001".to_string(),
            eventName: None,
            priority: None,
            createdDateTime: None,
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
        let report =
            build_measurement_report(&event, &asset_samples, 3.0, 0.0, "ven-1", Utc::now());
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

    // ── USAGE_FORECAST (WP3.6, §8.8) ────────────────────────────────

    fn make_minimal_plan_two_slots() -> Plan {
        use crate::entities::plan::{
            CostBreakdown, PlanSummary, PlanTimeSlot, PlanZone, PlanningHorizon,
        };
        let now = Utc::now();
        let slot = |i: usize, net_import_kw: f64, net_export_kw: f64| PlanTimeSlot {
            slot_index: i,
            start: now + Duration::seconds(i as i64 * 300),
            end: now + Duration::seconds((i as i64 + 1) * 300),
            import_tariff_eur_kwh: 0.2,
            export_tariff_eur_kwh: 0.05,
            co2_g_kwh: 300.0,
            grid_effective_cost: 0.26,
            rate_estimated: false,
            import_cap_kw: 10.0,
            export_cap_kw: 5.0,
            baseline_kw: 0.5,
            pv_forecast_kw: 0.0,
            surplus_available_kw: 0.0,
            allocations: vec![],
            net_import_kw,
            net_export_kw,
            import_flexibility_kw: 0.0,
            export_flexibility_kw: 0.0,
            bat_charge_kw: 0.0,
            bat_discharge_kw: 0.0,
            planned_kw_by_asset: std::collections::HashMap::new(),
            planned_state_by_asset: std::collections::HashMap::new(),
        };
        Plan {
            id: Uuid::new_v4(),
            created_at: now,
            trigger: crate::entities::asset::PlanTrigger::Periodic,
            horizon: PlanningHorizon {
                start_time: now,
                end_time: now + Duration::seconds(600),
                step_size_s: 300,
                num_steps: 2,
                far_horizon: now + Duration::seconds(600),
                zones: vec![PlanZone {
                    step_s: 300,
                    slots: 2,
                }],
            },
            slots: vec![slot(0, 2.0, 0.0), slot(1, 0.0, 1.5)],
            summary: PlanSummary::default(),
            envelopes: vec![],
            warnings: vec![],
            soc_trajectory_kwh: vec![],
            objective: crate::entities::PlannerObjective::MinCost,
            objective_eur: 0.0,
            friction_eur: 0.0,
            cost_breakdown: CostBreakdown::default(),
            solve_status: crate::entities::plan::SolveStatus::Optimal,
        }
    }

    #[test]
    fn test_usage_forecast_obligation_reports_plan_slots() {
        let ob = make_obligation("evt-f", "prog-f", "USAGE_FORECAST", 900);
        let plan = make_minimal_plan_two_slots();
        let report = build_measurement_report_for_obligation(
            &ob,
            &std::collections::HashMap::new(),
            "ven-1",
            None,
            Some(&plan),
            Utc::now(),
        )
        .expect("plan present -> report built");

        let intervals = &report.resources[0].intervals;
        assert_eq!(intervals.len(), 2, "one interval per plan slot");
        let v0 = intervals[0].payloads[0].values[0].as_f64().unwrap();
        let v1 = intervals[1].payloads[0].values[0].as_f64().unwrap();
        assert!((v0 - 2000.0).abs() < 1.0, "slot 0 net 2 kW import = 2000 W");
        assert!(
            (v1 - (-1500.0)).abs() < 1.0,
            "slot 1 net 1.5 kW export = -1500 W"
        );
        assert_eq!(intervals[0].payloads[0].r#type, "USAGE_FORECAST");
        assert!(
            intervals[0]
                .intervalPeriod
                .as_ref()
                .unwrap()
                .start
                .is_some(),
            "forecast intervals carry the plan slot's own start time"
        );
    }

    #[test]
    fn usage_obligation_with_historical_false_serves_forecast_slots() {
        // R-15: `reportDescriptor.historical: false` on a plain USAGE payload
        // is a forecast request — plan slots, typed as the requested payload.
        let mut ob = make_obligation("evt-h", "prog-h", "USAGE", 900);
        ob.historical = false;
        let plan = make_minimal_plan_two_slots();
        let report = build_measurement_report_for_obligation(
            &ob,
            &std::collections::HashMap::new(),
            "ven-1",
            None,
            Some(&plan),
            Utc::now(),
        )
        .expect("plan present -> forecast report built");

        let intervals = &report.resources[0].intervals;
        assert_eq!(intervals.len(), 2, "one interval per plan slot");
        assert_eq!(intervals[0].payloads[0].r#type, "USAGE");
        let v0 = intervals[0].payloads[0].values[0].as_f64().unwrap();
        assert!((v0 - 2000.0).abs() < 1.0, "forecast value from plan slot 0");
    }

    #[test]
    fn test_usage_forecast_obligation_without_plan_returns_none() {
        let ob = make_obligation("evt-f", "prog-f", "USAGE_FORECAST", 900);
        let report = build_measurement_report_for_obligation(
            &ob,
            &std::collections::HashMap::new(),
            "ven-1",
            None,
            None,
            Utc::now(),
        );
        assert!(report.is_none(), "no adopted plan -> no forecast report");
    }

    // ── operating_state (WP3.6) ─────────────────────────────────────

    #[test]
    fn test_operating_state_fresh_samples_active() {
        let now = ts(1800);
        let mut samples = std::collections::HashMap::new();
        samples.insert(
            "ev".to_string(),
            vec![AssetReportSample {
                ts: ts(1750),
                power_kw: 1.0,
                soc: None,
            }],
        );
        assert_eq!(operating_state(&samples, now), "ACTIVE");
    }

    #[test]
    fn test_operating_state_stale_samples_unresponsive() {
        let now = ts(1800);
        let mut samples = std::collections::HashMap::new();
        samples.insert(
            "ev".to_string(),
            vec![AssetReportSample {
                ts: ts(0),
                power_kw: 1.0,
                soc: None,
            }],
        );
        assert_eq!(operating_state(&samples, now), "UNRESPONSIVE");
    }

    #[test]
    fn test_operating_state_no_samples_offline() {
        let samples = std::collections::HashMap::new();
        assert_eq!(operating_state(&samples, ts(0)), "OFFLINE");
    }
}
