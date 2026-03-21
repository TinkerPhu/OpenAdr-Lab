/// Controller reporter: builds OpenADR VEN report payloads.
///
/// Two modes:
///   - Measurement reports (timer-driven): one TELEMETRY_USAGE report per active event.
///   - Status reports (event-driven): TELEMETRY_STATUS triggered by PlanCycle/PacketTransition.
use chrono::{DateTime, Duration, Utc};
use serde_json::{json, Value};
use std::collections::HashMap;
use tracing::debug;

use crate::common::{parse_iso8601_duration_secs, Aggregation, Interpolation, TimeSeries};
use crate::controller::trace::{AssetHistoryBuffer, ControllerEvent};
use crate::entities::capacity::OadrReportObligation;

// ---------------------------------------------------------------------------
// AssetHistoryBuffer → TimeSeries conversion
// ---------------------------------------------------------------------------

/// Extract a single named column from an `AssetHistoryBuffer` into a scalar
/// `TimeSeries`, skipping rows where the value is NaN (missing data).
fn history_to_timeseries(
    buf: &AssetHistoryBuffer,
    column: &str,
    interpolation: Interpolation,
    window: Option<(DateTime<Utc>, DateTime<Utc>)>,
) -> TimeSeries {
    let points = buf.to_timeline(window);
    let samples: Vec<(DateTime<Utc>, f64)> = points
        .iter()
        .filter_map(|p| {
            let v = p.values.get(column).copied()?;
            if v.is_nan() {
                None
            } else {
                Some((p.ts, v))
            }
        })
        .collect();
    TimeSeries {
        samples,
        interpolation,
    }
}

// ---------------------------------------------------------------------------
// Interval activity detection
// ---------------------------------------------------------------------------

/// Returns true if `event` has at least one interval that is currently active.
///
/// An interval is active when:
///   - It has no `intervalPeriod` field (treat as always-active), OR
///   - Its `intervalPeriod.start` is absent (treat as always-active), OR
///   - `start <= now < start + duration`
fn event_is_active(event: &Value, now: DateTime<Utc>) -> bool {
    let intervals = match event.get("intervals").and_then(|v| v.as_array()) {
        Some(arr) if !arr.is_empty() => arr,
        _ => return false,
    };

    intervals.iter().any(|interval| {
        let ip = match interval.get("intervalPeriod") {
            Some(ip) => ip,
            None => return true, // no intervalPeriod = always-active
        };
        let start_str = match ip.get("start").and_then(|v| v.as_str()) {
            Some(s) => s,
            None => return true, // no start = always-active
        };
        let interval_start: DateTime<Utc> = match start_str.parse() {
            Ok(dt) => dt,
            Err(_) => return true, // unparseable = treat as active
        };
        // If no duration, use a very long window (1 year)
        let duration_secs = ip
            .get("duration")
            .and_then(|v| v.as_str())
            .map(parse_iso8601_duration_secs)
            .unwrap_or(365 * 24 * 3_600);
        let interval_end = interval_start + Duration::seconds(duration_secs);
        interval_start <= now && now < interval_end
    })
}

// ---------------------------------------------------------------------------
// Measurement report (timer-driven, T046)
// ---------------------------------------------------------------------------

/// Build a TELEMETRY_USAGE measurement report for a single active OpenADR event.
///
/// The report includes:
///   - Net site import power (sum of positive asset power_kw from latest history row).
///   - OPERATING_STATE = "ACTIVE".
///   - STORAGE_CHARGE_LEVEL (EV SoC %) if EV history is available.
///
/// Returns None if the event has no id or programID.
pub fn build_measurement_report(
    event: &Value,
    asset_history: &HashMap<String, AssetHistoryBuffer>,
    ven_name: &str,
) -> Option<Value> {
    let event_id = event.get("id").and_then(|v| v.as_str())?;
    let program_id = event.get("programID").and_then(|v| v.as_str())?;

    let report_name = format!("auto-{}-{}", ven_name, event_id);
    let resource_name = format!("{}-meter", ven_name);

    // Compute net site import power from latest asset history rows
    let net_import_kw: f64 = asset_history
        .values()
        .filter_map(|buf| {
            let pts = buf.to_timeline(None);
            pts.last().map(|p| p.values.get("power_kw").copied().unwrap_or(0.0))
        })
        .filter(|&kw| kw > 0.0) // only import contributions
        .sum();
    let net_import_w = net_import_kw * 1000.0;

    // Extract the primary payload type from the event's first interval
    let payload_type = event
        .get("intervals")
        .and_then(|v| v.as_array())
        .and_then(|arr| arr.first())
        .and_then(|iv| iv.get("payloads"))
        .and_then(|v| v.as_array())
        .and_then(|arr| arr.first())
        .and_then(|p| p.get("type"))
        .and_then(|v| v.as_str())
        .unwrap_or("SIMPLE");

    let (report_type, report_value) = match payload_type {
        "IMPORT_CAPACITY_LIMIT" => ("USAGE", net_import_w),
        "EXPORT_CAPACITY_LIMIT" => {
            let export_kw: f64 = asset_history
                .values()
                .filter_map(|buf| {
                    let pts = buf.to_timeline(None);
                    pts.last().map(|p| p.values.get("power_kw").copied().unwrap_or(0.0))
                })
                .filter(|&kw| kw < 0.0)
                .map(|kw| -kw)
                .sum();
            ("USAGE", export_kw * 1000.0)
        }
        "PRICE" => ("USAGE", net_import_w),
        "SIMPLE" => ("SIMPLE", 1.0),
        _ => ("USAGE", net_import_w),
    };

    let mut payloads = vec![
        json!({ "type": report_type, "values": [report_value] }),
        json!({ "type": "OPERATING_STATE", "values": ["ACTIVE"] }),
    ];

    // Add EV SoC if history available
    if let Some(ev_buf) = asset_history.get("ev") {
        let pts = ev_buf.to_timeline(None);
        if let Some(last) = pts.last() {
            if let Some(&soc) = last.values.get("soc") {
                if !soc.is_nan() {
                    payloads.push(json!({
                        "type": "STORAGE_CHARGE_LEVEL",
                        "values": [format!("{:.1}", soc * 100.0)]
                    }));
                }
            }
        }
    }

    let report = json!({
        "programID": program_id,
        "eventID": event_id,
        "clientName": ven_name,
        "reportName": report_name,
        "resources": [{
            "resourceName": resource_name,
            "intervals": [{"id": 0, "payloads": payloads}]
        }]
    });

    debug!(report_name, event_id, report_type, report_value, "built measurement report");
    Some(report)
}

/// Build measurement reports for all currently active events (timer-driven entry point).
pub fn build_measurement_reports_for_active_events(
    events: &[Value],
    asset_history: &HashMap<String, AssetHistoryBuffer>,
    ven_name: &str,
    now: DateTime<Utc>,
) -> Vec<Value> {
    let mut seen = std::collections::HashSet::new();
    let mut reports = Vec::new();

    for event in events {
        if !event_is_active(event, now) {
            continue;
        }
        // Skip events with reportDescriptors — those are handled by the obligation loop
        let descriptors = event
            .get("reportDescriptors")
            .and_then(|v| v.as_array());
        let has_descriptors = descriptors
            .map_or(false, |arr| !arr.is_empty());
        if has_descriptors {
            let event_id = event.get("id").and_then(|v| v.as_str()).unwrap_or("?");
            debug!(event_id, "timer-driven: skipping event with reportDescriptors");
            continue;
        }
        if let Some(event_id) = event.get("id").and_then(|v| v.as_str()) {
            if seen.insert(event_id.to_string()) {
                debug!(event_id, "timer-driven: building single-interval report");
                if let Some(report) = build_measurement_report(event, asset_history, ven_name) {
                    reports.push(report);
                }
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
    asset_history: &HashMap<String, AssetHistoryBuffer>,
    ven_name: &str,
) -> Option<Value> {
    let program_id = obligation.program_id.as_deref()?;
    let event_id = &obligation.event_id;

    let report_name = format!("ob-{}-{}-{}", ven_name, event_id, obligation.payload_type);
    let resource_name = format!("{}-meter", ven_name);
    let interval_width = Duration::seconds(obligation.interval_duration_s as i64);
    let duration_iso = format_iso8601_duration(obligation.interval_duration_s);

    // Build net site power TimeSeries (sum all assets' power_kw)
    let net_power_ts = build_net_site_power_ts(asset_history);

    let payload_type = &obligation.payload_type;

    let intervals: Vec<Value> = match payload_type.as_str() {
        "STORAGE_CHARGE_STATE" | "STORAGE_CHARGE_LEVEL" => {
            build_soc_intervals(asset_history, interval_width, &duration_iso)
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
                    json!({
                        "id": i,
                        "intervalPeriod": {
                            "start": ts.to_rfc3339(),
                            "duration": duration_iso
                        },
                        "payloads": [
                            {"type": report_type, "values": [value_w]},
                            {"type": "OPERATING_STATE", "values": ["ACTIVE"]}
                        ]
                    })
                })
                .collect()
        }
    };

    if intervals.is_empty() {
        return None;
    }

    let report = json!({
        "programID": program_id,
        "eventID": event_id,
        "clientName": ven_name,
        "reportName": report_name,
        "resources": [{
            "resourceName": resource_name,
            "intervals": intervals
        }]
    });

    debug!(
        report_name,
        event_id,
        interval_count = intervals.len(),
        "built obligation measurement report"
    );
    Some(report)
}

/// Sum all assets' `power_kw` columns into a single net site power TimeSeries.
fn build_net_site_power_ts(
    asset_history: &HashMap<String, AssetHistoryBuffer>,
) -> TimeSeries {
    let mut per_asset: Vec<TimeSeries> = asset_history
        .values()
        .map(|buf| history_to_timeseries(buf, "power_kw", Interpolation::Step, None))
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
        .filter_map(|&t| {
            let sum: f64 = per_asset
                .iter()
                .filter_map(|s| s.interpolate_at(t))
                .sum();
            Some((t, sum))
        })
        .collect();

    TimeSeries {
        samples,
        interpolation: Interpolation::Step,
    }
}

/// Build SoC intervals using point-in-time sampling at interval ends.
fn build_soc_intervals(
    asset_history: &HashMap<String, AssetHistoryBuffer>,
    interval_width: Duration,
    duration_iso: &str,
) -> Vec<Value> {
    // Look for EV or battery SoC
    let soc_ts = asset_history
        .get("ev")
        .map(|buf| history_to_timeseries(buf, "soc", Interpolation::Step, None))
        .filter(|ts| !ts.samples.is_empty())
        .or_else(|| {
            asset_history
                .get("battery")
                .map(|buf| history_to_timeseries(buf, "soc", Interpolation::Step, None))
                .filter(|ts| !ts.samples.is_empty())
        });

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

            json!({
                "id": i,
                "intervalPeriod": {
                    "start": ts.to_rfc3339(),
                    "duration": duration_iso
                },
                "payloads": [
                    {
                        "type": "STORAGE_CHARGE_LEVEL",
                        "values": [format!("{:.1}", soc_value * 100.0)]
                    },
                    {"type": "OPERATING_STATE", "values": ["ACTIVE"]}
                ]
            })
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
/// Only emits for `PlanCycle` and `PacketTransition` variants.
/// Returns None for all other event types.
pub fn build_status_report(
    event: &ControllerEvent,
    asset_history: &HashMap<String, AssetHistoryBuffer>,
    ven_name: &str,
    _now: DateTime<Utc>,
) -> Option<Value> {
    let (description, asset_id_opt) = match event {
        ControllerEvent::PlanCycle {
            trigger_reason,
            firm_slots,
            flexible_slots,
            ..
        } => (
            format!(
                "PlanCycle trigger={} firm={} flex={}",
                trigger_reason, firm_slots, flexible_slots
            ),
            None,
        ),
        ControllerEvent::PacketTransition {
            asset_id,
            from_status,
            to_status,
            ..
        } => (
            format!("PacketTransition {} → {}", from_status, to_status),
            Some(asset_id.clone()),
        ),
        _ => return None,
    };

    // Compute site-level net import for the status snapshot
    let net_import_kw: f64 = asset_history
        .values()
        .filter_map(|buf| {
            let pts = buf.to_timeline(None);
            pts.last().map(|p| p.values.get("power_kw").copied().unwrap_or(0.0))
        })
        .filter(|&kw| kw > 0.0)
        .sum();

    let resource_name = asset_id_opt
        .as_deref()
        .map(|id| format!("{}-{}", ven_name, id))
        .unwrap_or_else(|| format!("{}-site", ven_name));

    let report = json!({
        "clientName": ven_name,
        "reportName": format!("status-{}", ven_name),
        "resources": [{
            "resourceName": resource_name,
            "intervals": [{
                "id": 0,
                "payloads": [
                    {"type": "TELEMETRY_STATUS", "values": [description]},
                    {"type": "USAGE", "values": [net_import_kw * 1000.0]}
                ]
            }]
        }]
    });

    Some(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    fn ts(offset_s: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(1_700_000_000 + offset_s, 0).unwrap()
    }

    fn make_buf(rows: &[(i64, &[(&str, f64)])]) -> AssetHistoryBuffer {
        let mut buf = AssetHistoryBuffer::new(3600);
        for &(offset, values) in rows {
            let map: HashMap<String, f64> =
                values.iter().map(|(k, v)| (k.to_string(), *v)).collect();
            buf.push(ts(offset), map);
        }
        buf
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

    // ── history_to_timeseries ──────────────────────────────────────

    #[test]
    fn history_to_ts_extracts_column() {
        let buf = make_buf(&[
            (0, &[("power_kw", 1.0), ("soc", 0.5)]),
            (60, &[("power_kw", 2.0), ("soc", 0.6)]),
            (120, &[("power_kw", 3.0), ("soc", 0.7)]),
        ]);
        let series = history_to_timeseries(&buf, "power_kw", Interpolation::Step, None);
        assert_eq!(series.samples.len(), 3);
        assert!((series.samples[0].1 - 1.0).abs() < 1e-9);
        assert!((series.samples[2].1 - 3.0).abs() < 1e-9);
    }

    #[test]
    fn history_to_ts_skips_nan() {
        let mut buf = AssetHistoryBuffer::new(10);
        buf.push(ts(0), [("power_kw".into(), 1.0)].into());
        buf.push(ts(60), HashMap::new()); // power_kw will be NaN
        buf.push(ts(120), [("power_kw".into(), 3.0)].into());

        let series = history_to_timeseries(&buf, "power_kw", Interpolation::Step, None);
        assert_eq!(series.samples.len(), 2); // NaN row skipped
        assert!((series.samples[0].1 - 1.0).abs() < 1e-9);
        assert!((series.samples[1].1 - 3.0).abs() < 1e-9);
    }

    #[test]
    fn history_to_ts_empty_buffer() {
        let buf = AssetHistoryBuffer::new(10);
        let series = history_to_timeseries(&buf, "power_kw", Interpolation::Step, None);
        assert!(series.samples.is_empty());
    }

    #[test]
    fn history_to_ts_nonexistent_column() {
        let buf = make_buf(&[(0, &[("power_kw", 1.0)])]);
        let series = history_to_timeseries(&buf, "no_such_col", Interpolation::Step, None);
        assert!(series.samples.is_empty());
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
        // 4 rows at 15-min intervals, each with constant power
        let buf = make_buf(&[
            (0, &[("power_kw", 2.0)]),
            (900, &[("power_kw", 3.0)]),
            (1800, &[("power_kw", 4.0)]),
            (2700, &[("power_kw", 5.0)]),
        ]);
        let mut history = HashMap::new();
        history.insert("site".to_string(), buf);

        let ob = make_obligation("ev1", "prog1", "USAGE", 900);
        let report =
            build_measurement_report_for_obligation(&ob, &history, "ven-1").unwrap();

        let intervals = report["resources"][0]["intervals"].as_array().unwrap();
        assert!(
            intervals.len() >= 2,
            "expected multiple intervals, got {}",
            intervals.len()
        );

        // Verify sequential IDs
        for (i, iv) in intervals.iter().enumerate() {
            assert_eq!(iv["id"], i as u64);
            assert!(iv["intervalPeriod"]["start"].is_string());
            assert_eq!(iv["intervalPeriod"]["duration"], "PT15M");
        }

        // Verify payloads structure
        let payloads = intervals[0]["payloads"].as_array().unwrap();
        assert!(payloads.iter().any(|p| p["type"] == "USAGE"));
        assert!(payloads.iter().any(|p| p["type"] == "OPERATING_STATE"));
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
        let history = HashMap::new();
        assert!(
            build_measurement_report_for_obligation(&ob, &history, "ven-1").is_none()
        );
    }

    #[test]
    fn obligation_report_empty_history_returns_none() {
        let ob = make_obligation("e1", "p1", "USAGE", 900);
        let history = HashMap::new();
        assert!(
            build_measurement_report_for_obligation(&ob, &history, "ven-1").is_none()
        );
    }

    // ── import/export split ────────────────────────────────────────

    #[test]
    fn obligation_report_import_clamps_negative_to_zero() {
        let buf = make_buf(&[
            (0, &[("power_kw", -5.0)]),
            (900, &[("power_kw", -5.0)]),
        ]);
        let mut history = HashMap::new();
        history.insert("pv".to_string(), buf);

        let ob = make_obligation("e1", "p1", "IMPORT_CAPACITY_LIMIT", 900);
        let report =
            build_measurement_report_for_obligation(&ob, &history, "ven-1").unwrap();
        let intervals = report["resources"][0]["intervals"].as_array().unwrap();

        for iv in intervals {
            let usage = iv["payloads"]
                .as_array()
                .unwrap()
                .iter()
                .find(|p| p["type"] == "USAGE")
                .unwrap();
            let val = usage["values"][0].as_f64().unwrap();
            assert!(
                (val - 0.0).abs() < 1e-9,
                "import should be 0 for export power, got {val}"
            );
        }
    }

    #[test]
    fn obligation_report_export_uses_absolute_negative() {
        let buf = make_buf(&[
            (0, &[("power_kw", -3.0)]),
            (900, &[("power_kw", -3.0)]),
        ]);
        let mut history = HashMap::new();
        history.insert("pv".to_string(), buf);

        let ob = make_obligation("e1", "p1", "EXPORT_CAPACITY_LIMIT", 900);
        let report =
            build_measurement_report_for_obligation(&ob, &history, "ven-1").unwrap();
        let intervals = report["resources"][0]["intervals"].as_array().unwrap();

        for iv in intervals {
            let usage = iv["payloads"]
                .as_array()
                .unwrap()
                .iter()
                .find(|p| p["type"] == "USAGE")
                .unwrap();
            let val = usage["values"][0].as_f64().unwrap();
            assert!(
                (val - 3000.0).abs() < 1e-9,
                "export should be 3000 W, got {val}"
            );
        }
    }

    // ── SoC point-in-time ──────────────────────────────────────────

    #[test]
    fn obligation_report_soc_point_in_time() {
        let buf = make_buf(&[
            (0, &[("soc", 0.2), ("power_kw", 7.0)]),
            (900, &[("soc", 0.4), ("power_kw", 7.0)]),
            (1800, &[("soc", 0.6), ("power_kw", 7.0)]),
            (2700, &[("soc", 0.8), ("power_kw", 7.0)]),
        ]);
        let mut history = HashMap::new();
        history.insert("ev".to_string(), buf);

        let ob = make_obligation("e1", "p1", "STORAGE_CHARGE_LEVEL", 900);
        let report =
            build_measurement_report_for_obligation(&ob, &history, "ven-1").unwrap();
        let intervals = report["resources"][0]["intervals"].as_array().unwrap();

        assert!(!intervals.is_empty());
        for iv in intervals {
            let soc_payload = iv["payloads"]
                .as_array()
                .unwrap()
                .iter()
                .find(|p| p["type"] == "STORAGE_CHARGE_LEVEL")
                .unwrap();
            let soc_str = soc_payload["values"][0].as_str().unwrap();
            let soc_pct: f64 = soc_str.parse().unwrap();
            assert!(
                soc_pct >= 20.0 && soc_pct <= 80.0,
                "SoC {soc_pct} out of range"
            );
        }
    }

    // ── build_net_site_power_ts ────────────────────────────────────

    #[test]
    fn net_site_power_sums_assets() {
        let buf1 = make_buf(&[
            (0, &[("power_kw", 2.0)]),
            (60, &[("power_kw", 3.0)]),
        ]);
        let buf2 = make_buf(&[
            (0, &[("power_kw", -1.0)]),
            (60, &[("power_kw", 1.0)]),
        ]);
        let mut history = HashMap::new();
        history.insert("ev".to_string(), buf1);
        history.insert("pv".to_string(), buf2);

        let net = build_net_site_power_ts(&history);
        assert_eq!(net.samples.len(), 2);
        assert!((net.samples[0].1 - 1.0).abs() < 1e-9); // 2 + (-1) = 1
        assert!((net.samples[1].1 - 4.0).abs() < 1e-9); // 3 + 1 = 4
    }

    #[test]
    fn net_site_power_empty_history() {
        let history = HashMap::new();
        let net = build_net_site_power_ts(&history);
        assert!(net.samples.is_empty());
    }
}
