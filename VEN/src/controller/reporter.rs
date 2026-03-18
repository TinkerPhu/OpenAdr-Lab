/// Controller reporter: builds OpenADR VEN report payloads.
///
/// Two modes:
///   - Measurement reports (timer-driven): one TELEMETRY_USAGE report per active event.
///   - Status reports (event-driven): TELEMETRY_STATUS triggered by PlanCycle/PacketTransition.
use chrono::{DateTime, Duration, Utc};
use serde_json::{json, Value};
use std::collections::HashMap;
use tracing::debug;

use crate::controller::trace::{AssetHistoryBuffer, ControllerEvent};

// ---------------------------------------------------------------------------
// Interval activity detection
// ---------------------------------------------------------------------------

/// Parse a minimal ISO 8601 duration string (PTxH / PTxM / PTxS combos).
fn parse_duration_secs(s: &str) -> i64 {
    let s = s.trim();
    if !s.starts_with('P') {
        return 3_600; // 1-hour fallback
    }
    let rest = &s[1..];
    let (_, time_part) = if let Some(t) = rest.find('T') {
        (&rest[..t], &rest[t + 1..])
    } else {
        (rest, "")
    };
    let mut total: i64 = 0;
    let mut buf = String::new();
    for ch in time_part.chars() {
        if ch.is_ascii_digit() || ch == '.' {
            buf.push(ch);
        } else if ch == 'H' {
            total += buf.parse::<f64>().unwrap_or(0.0) as i64 * 3_600;
            buf.clear();
        } else if ch == 'M' {
            total += buf.parse::<f64>().unwrap_or(0.0) as i64 * 60;
            buf.clear();
        } else if ch == 'S' {
            total += buf.parse::<f64>().unwrap_or(0.0) as i64;
            buf.clear();
        }
    }
    if total <= 0 { 3_600 } else { total }
}

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
            .map(parse_duration_secs)
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
        if let Some(event_id) = event.get("id").and_then(|v| v.as_str()) {
            if seen.insert(event_id.to_string()) {
                if let Some(report) = build_measurement_report(event, asset_history, ven_name) {
                    reports.push(report);
                }
            }
        }
    }

    reports
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
