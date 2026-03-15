use chrono::{DateTime, Utc};
use serde_json::{json, Value};
use std::collections::HashSet;
use tracing::debug;

use crate::reactor::interval::find_active_intervals;
use crate::simulator::SimState;

/// Build an OpenADR report payload for an active event using actual simulator values.
pub fn build_report(
    event: &Value,
    sim: &SimState,
    reactor_mode: &str,
    ven_name: &str,
) -> Option<Value> {
    let event_id = event.get("id").and_then(|v| v.as_str())?;
    let program_id = event.get("programID").and_then(|v| v.as_str())?;

    let report_name = format!("auto-{}-{}", ven_name, event_id);
    let resource_name = format!("{}-meter", ven_name);

    // Extract the primary payload type from the event's first interval
    let payload_type = event
        .get("intervals")
        .and_then(|v| v.as_array())
        .and_then(|arr| arr.first())
        .and_then(|interval| interval.get("payloads"))
        .and_then(|v| v.as_array())
        .and_then(|arr| arr.first())
        .and_then(|payload| payload.get("type"))
        .and_then(|v| v.as_str())
        .unwrap_or("SIMPLE");

    // Map event payload type → report payload type + actual sim value
    let (report_type, report_value) = match payload_type {
        "IMPORT_CAPACITY_LIMIT" => ("USAGE", sim.grid.import_w),
        "EXPORT_CAPACITY_LIMIT" => ("USAGE", sim.grid.export_w),
        "PRICE" => ("USAGE", sim.grid.net_power_w),
        "SIMPLE" => ("SIMPLE", 1.0),
        _ => ("USAGE", sim.grid.net_power_w),
    };

    // Build resource payloads
    let mut payloads = vec![
        json!({
            "type": report_type,
            "values": [report_value]
        }),
        json!({
            "type": "OPERATING_STATE",
            "values": [reactor_mode]
        }),
    ];

    // Add EV SOC if present
    if let Some(ev) = sim.ev() {
        payloads.push(json!({
            "type": "STORAGE_CHARGE_LEVEL",
            "values": [format!("{:.1}", ev.soc * 100.0)]
        }));
    }

    let report = json!({
        "programID": program_id,
        "eventID": event_id,
        "clientName": ven_name,
        "reportName": report_name,
        "resources": [{
            "resourceName": resource_name,
            "intervals": [{
                "id": 0,
                "payloads": payloads
            }]
        }]
    });

    debug!(report_name, event_id, report_type, report_value, "built auto-report");
    Some(report)
}

/// Build reports for all currently active events, one report per unique event.
pub fn build_reports_for_active_events(
    events: &[Value],
    sim: &SimState,
    reactor_mode: &str,
    ven_name: &str,
    now: DateTime<Utc>,
) -> Vec<Value> {
    let active = find_active_intervals(events, now);
    if active.is_empty() {
        return Vec::new();
    }

    // Deduplicate by event_id (multiple intervals → one report)
    let mut seen = HashSet::new();
    let mut reports = Vec::new();

    for interval in &active {
        if seen.insert(interval.event_id.clone()) {
            // Find the original event JSON for this event_id
            if let Some(event) = events.iter().find(|e| {
                e.get("id").and_then(|v| v.as_str()) == Some(&interval.event_id)
            }) {
                if let Some(report) = build_report(event, sim, reactor_mode, ven_name) {
                    reports.push(report);
                }
            }
        }
    }

    reports
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_event(event_type: &str) -> Value {
        json!({
            "id": "evt-123",
            "programID": "prog-456",
            "eventName": "test-event",
            "priority": 0,
            "intervals": [{
                "id": 0,
                "payloads": [{
                    "type": event_type,
                    "values": [5000.0]
                }]
            }]
        })
    }

    fn make_sim() -> SimState {
        SimState {
            assets: vec![],
            grid: crate::simulator::GridMeter {
                net_power_w: 1200.0,
                import_w: 1500.0,
                export_w: 300.0,
                voltage_v: 230.0,
                import_kwh: 0.0,
                export_kwh: 0.0,
            },
            last_tick: Utc::now(),
        }
    }

    #[test]
    fn import_cap_reports_actual_import() {
        let event = make_event("IMPORT_CAPACITY_LIMIT");
        let sim = make_sim();
        let report = build_report(&event, &sim, "IMPORT_CAP", "ven-1").unwrap();

        assert_eq!(report["reportName"], "auto-ven-1-evt-123");
        assert_eq!(report["programID"], "prog-456");
        assert_eq!(report["clientName"], "ven-1");

        let payload = &report["resources"][0]["intervals"][0]["payloads"][0];
        assert_eq!(payload["type"], "USAGE");
        assert_eq!(payload["values"][0], 1500.0);
    }

    #[test]
    fn export_cap_reports_actual_export() {
        let event = make_event("EXPORT_CAPACITY_LIMIT");
        let sim = make_sim();
        let report = build_report(&event, &sim, "EXPORT_CAP", "ven-1").unwrap();

        let payload = &report["resources"][0]["intervals"][0]["payloads"][0];
        assert_eq!(payload["type"], "USAGE");
        assert_eq!(payload["values"][0], 300.0);
    }

    #[test]
    fn price_reports_net_power() {
        let event = make_event("PRICE");
        let sim = make_sim();
        let report = build_report(&event, &sim, "PRICE", "ven-1").unwrap();

        let payload = &report["resources"][0]["intervals"][0]["payloads"][0];
        assert_eq!(payload["type"], "USAGE");
        assert_eq!(payload["values"][0], 1200.0);
    }

    #[test]
    fn simple_reports_ack() {
        let event = make_event("SIMPLE");
        let sim = make_sim();
        let report = build_report(&event, &sim, "IDLE", "ven-1").unwrap();

        let payload = &report["resources"][0]["intervals"][0]["payloads"][0];
        assert_eq!(payload["type"], "SIMPLE");
        assert_eq!(payload["values"][0], 1.0);
    }

    #[test]
    fn operating_state_included() {
        let event = make_event("PRICE");
        let sim = make_sim();
        let report = build_report(&event, &sim, "PRICE", "ven-1").unwrap();

        let payloads = report["resources"][0]["intervals"][0]["payloads"].as_array().unwrap();
        let os_payload = payloads.iter().find(|p| p["type"] == "OPERATING_STATE").unwrap();
        assert_eq!(os_payload["values"][0], "PRICE");
    }
}
