use chrono::{DateTime, Duration, Utc};
use uuid::Uuid;

use crate::common::parse_iso8601_duration_secs;
use crate::controller::vtn_port::OadrEvent;
use crate::entities::capacity::{
    AlertWindow, DispatchWindow, OadrCapacityState, OadrReportObligation, SimpleWindow,
};

// Rate/capacity-schedule parsing lives in `rate_schedule.rs` (split out to stay under the
// VEN/src/ 500-production-line cap) — re-exported here so call sites and this file's own
// tests can keep referencing `openadr_interface::{parse_rate_snapshots, parse_capacity_schedule}`.
pub use crate::controller::rate_schedule::{parse_capacity_schedule, parse_rate_snapshots};

// ---------------------------------------------------------------------------
// Capacity state parsing
// ---------------------------------------------------------------------------

/// Parse capacity limits from the CURRENT set of active events.
/// Computed from scratch on each call — reflects the live VTN state.
/// Strictest limit wins (lowest value when multiple events specify same field).
pub fn parse_capacity_state(events: &[OadrEvent], now: DateTime<Utc>) -> OadrCapacityState {
    let mut existing = OadrCapacityState::default();
    let mut import_limit: Option<(f64, String)> = None;
    let mut export_limit: Option<(f64, String)> = None;
    let mut import_sub: Option<f64> = None;
    let mut import_res: Option<f64> = None;
    let mut export_sub: Option<f64> = None;
    let mut export_res: Option<f64> = None;
    let mut found_any = false;

    for event in events {
        let event_id = event.id.clone();

        for interval in &event.intervals {
            for payload in &interval.payloads {
                let payload_type = payload.r#type.as_str();
                let value = payload.values.first().and_then(|v| v.as_f64());

                match payload_type {
                    "IMPORT_CAPACITY_LIMIT" => {
                        if let Some(v) = value {
                            found_any = true;
                            import_limit = Some(match import_limit {
                                None => (v, event_id.clone()),
                                Some((cur, ref eid)) => {
                                    if v < cur {
                                        (v, event_id.clone())
                                    } else {
                                        (cur, eid.clone())
                                    }
                                }
                            });
                        }
                    }
                    "EXPORT_CAPACITY_LIMIT" => {
                        if let Some(v) = value {
                            found_any = true;
                            export_limit = Some(match export_limit {
                                None => (v, event_id.clone()),
                                Some((cur, ref eid)) => {
                                    if v < cur {
                                        (v, event_id.clone())
                                    } else {
                                        (cur, eid.clone())
                                    }
                                }
                            });
                        }
                    }
                    "IMPORT_CAPACITY_SUBSCRIPTION" => {
                        if let Some(v) = value {
                            found_any = true;
                            import_sub = Some(match import_sub {
                                None => v,
                                Some(cur) => cur.min(v),
                            });
                        }
                    }
                    "IMPORT_CAPACITY_RESERVATION" => {
                        if let Some(v) = value {
                            found_any = true;
                            import_res = Some(match import_res {
                                None => v,
                                Some(cur) => cur.min(v),
                            });
                        }
                    }
                    // WP3.3: export-side subscription/reservation (strictest wins,
                    // matching the import side).
                    "EXPORT_CAPACITY_SUBSCRIPTION" => {
                        if let Some(v) = value {
                            found_any = true;
                            export_sub = Some(match export_sub {
                                None => v,
                                Some(cur) => cur.min(v),
                            });
                        }
                    }
                    "EXPORT_CAPACITY_RESERVATION" => {
                        if let Some(v) = value {
                            found_any = true;
                            export_res = Some(match export_res {
                                None => v,
                                Some(cur) => cur.min(v),
                            });
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    if found_any {
        existing.import_limit_kw = import_limit.as_ref().map(|(v, _)| *v);
        existing.import_limit_event_id = import_limit.map(|(_, eid)| eid);
        existing.export_limit_kw = export_limit.as_ref().map(|(v, _)| *v);
        existing.export_limit_event_id = export_limit.map(|(_, eid)| eid);
        existing.import_subscription_kw = import_sub;
        existing.import_reservation_kw = import_res;
        existing.export_subscription_kw = export_sub;
        existing.export_reservation_kw = export_res;
        existing.last_updated = Some(now);
    }

    existing
}

// ---------------------------------------------------------------------------
// Grid alert parsing (WP3.1, BL-04)
// ---------------------------------------------------------------------------

/// Extract grid-alert windows (ALERT_GRID_EMERGENCY / ALERT_BLACK_START) from
/// active events. The window comes from the interval's own `intervalPeriod`,
/// falling back to the event-level one (User Guide Example 8.1-1 puts it at
/// event level with a bare interval). Intervals without any resolvable start
/// are skipped. The payload value is the spec's human-readable message.
pub fn parse_alert_windows(events: &[OadrEvent]) -> Vec<AlertWindow> {
    let mut out = Vec::new();
    for event in events {
        for interval in &event.intervals {
            for payload in &interval.payloads {
                let alert_type = payload.r#type.as_str();
                if !matches!(alert_type, "ALERT_GRID_EMERGENCY" | "ALERT_BLACK_START") {
                    continue;
                }
                let Some(ip) = interval
                    .intervalPeriod
                    .as_ref()
                    .or(event.intervalPeriod.as_ref())
                else {
                    continue;
                };
                let Some(start) = ip
                    .start
                    .as_deref()
                    .and_then(|s| s.parse::<DateTime<Utc>>().ok())
                else {
                    continue;
                };
                let duration_s =
                    parse_iso8601_duration_secs(ip.duration.as_deref().unwrap_or("PT1H"));
                let message = payload
                    .values
                    .first()
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                out.push(AlertWindow {
                    alert_type: alert_type.to_string(),
                    start,
                    end: start + Duration::seconds(duration_s),
                    event_id: event.id.clone(),
                    message,
                });
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------
// SIMPLE level parsing (WP3.2)
// ---------------------------------------------------------------------------

/// Extract SIMPLE load-shed windows (levels 1–3) from active events. Window
/// resolution matches `parse_alert_windows` (interval-level `intervalPeriod`,
/// event-level fallback). Level 0 ("normal") windows are dropped here — they
/// constrain nothing. Non-numeric or out-of-range values are skipped.
pub fn parse_simple_windows(events: &[OadrEvent]) -> Vec<SimpleWindow> {
    let mut out = Vec::new();
    for event in events {
        for interval in &event.intervals {
            for payload in &interval.payloads {
                if payload.r#type != "SIMPLE" {
                    continue;
                }
                let Some(level) = payload
                    .values
                    .first()
                    .and_then(|v| v.as_f64())
                    .filter(|v| (0.0..=3.0).contains(v))
                    .map(|v| v as u8)
                else {
                    continue;
                };
                if level == 0 {
                    continue;
                }
                let Some(ip) = interval
                    .intervalPeriod
                    .as_ref()
                    .or(event.intervalPeriod.as_ref())
                else {
                    continue;
                };
                let Some(start) = ip
                    .start
                    .as_deref()
                    .and_then(|s| s.parse::<DateTime<Utc>>().ok())
                else {
                    continue;
                };
                let duration_s =
                    parse_iso8601_duration_secs(ip.duration.as_deref().unwrap_or("PT1H"));
                out.push(SimpleWindow {
                    level,
                    start,
                    end: start + Duration::seconds(duration_s),
                    event_id: event.id.clone(),
                });
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Direct setpoints (WP3.4 — BL-06/BL-24)
// ---------------------------------------------------------------------------

/// Extract DISPATCH_SETPOINT windows. Window resolution matches the alert/
/// SIMPLE parsers (interval-level `intervalPeriod`, event-level fallback);
/// the payload value is the commanded net site setpoint in kW.
pub fn parse_dispatch_windows(events: &[OadrEvent]) -> Vec<DispatchWindow> {
    let mut out = Vec::new();
    for event in events {
        for interval in &event.intervals {
            for payload in &interval.payloads {
                if payload.r#type != "DISPATCH_SETPOINT" {
                    continue;
                }
                let Some(setpoint_kw) = payload.values.first().and_then(|v| v.as_f64()) else {
                    continue;
                };
                let Some(ip) = interval
                    .intervalPeriod
                    .as_ref()
                    .or(event.intervalPeriod.as_ref())
                else {
                    continue;
                };
                let Some(start) = ip
                    .start
                    .as_deref()
                    .and_then(|s| s.parse::<DateTime<Utc>>().ok())
                else {
                    continue;
                };
                let duration_s =
                    parse_iso8601_duration_secs(ip.duration.as_deref().unwrap_or("PT1H"));
                out.push(DispatchWindow {
                    setpoint_kw,
                    start,
                    end: start + Duration::seconds(duration_s),
                    event_id: event.id.clone(),
                });
            }
        }
    }
    out
}

/// Extract the first CHARGE_STATE_SETPOINT from active events (WP3.4):
/// `(target_soc 0.0–1.0, window_end, event_id)`. Values > 1 are read as
/// percent (80 → 0.8); out-of-range results are dropped.
pub fn parse_charge_state_setpoint(events: &[OadrEvent]) -> Option<(f64, DateTime<Utc>, String)> {
    for event in events {
        for interval in &event.intervals {
            for payload in &interval.payloads {
                if payload.r#type != "CHARGE_STATE_SETPOINT" {
                    continue;
                }
                let raw = payload.values.first().and_then(|v| v.as_f64())?;
                let target_soc = if raw > 1.0 { raw / 100.0 } else { raw };
                if !(0.0..=1.0).contains(&target_soc) {
                    continue;
                }
                let ip = interval
                    .intervalPeriod
                    .as_ref()
                    .or(event.intervalPeriod.as_ref())?;
                let start = ip.start.as_deref()?.parse::<DateTime<Utc>>().ok()?;
                let duration_s =
                    parse_iso8601_duration_secs(ip.duration.as_deref().unwrap_or("PT1H"));
                return Some((
                    target_soc,
                    start + Duration::seconds(duration_s),
                    event.id.clone(),
                ));
            }
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Report obligation extraction
// ---------------------------------------------------------------------------

/// Extract report obligations from event reportDescriptors.
/// Deduplicates by (event_id, payload_type).
pub fn extract_report_obligations(
    events: &[OadrEvent],
    now: DateTime<Utc>,
    existing: &[OadrReportObligation],
) -> Vec<OadrReportObligation> {
    let mut result: Vec<OadrReportObligation> = Vec::new();

    for event in events {
        let event_id = event.id.clone();
        let program_id = Some(event.programID.clone());

        let descriptors = match event.reportDescriptors.as_ref() {
            Some(arr) if !arr.is_empty() => arr,
            _ => continue,
        };

        for descriptor in descriptors {
            let payload_type = descriptor.payloadType.clone();

            // Skip if already tracked
            let already_exists = existing
                .iter()
                .any(|ob| ob.event_id == event_id && ob.payload_type == payload_type)
                || result
                    .iter()
                    .any(|ob| ob.event_id == event_id && ob.payload_type == payload_type);

            if already_exists {
                continue;
            }

            let reading_type = descriptor
                .readingType
                .as_deref()
                .unwrap_or("DIRECT_READ")
                .to_string();

            // interval duration: from descriptor.frequency (seconds) or default 3600
            let interval_duration_s: u64 = descriptor
                .frequency
                .filter(|&f| f > 0)
                .map(|f| f as u64)
                .unwrap_or(3600);

            let due_at = now + Duration::seconds(interval_duration_s as i64);

            result.push(OadrReportObligation {
                id: Uuid::new_v4(),
                event_id: event_id.clone(),
                program_id: program_id.clone(),
                payload_type,
                reading_type,
                resource_name: None,
                due_at,
                interval_duration_s,
                fulfilled: false,
                created_at: now,
                historical: descriptor.historical.unwrap_or(true),
            });
        }
    }

    result
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::controller::vtn_port::OadrEvent;
    use chrono::TimeZone;
    use serde_json::json;

    // ── parse_alert_windows (WP3.1, BL-04) ──────────────────────────────────

    #[test]
    fn test_parse_alert_windows_event_level_period_fallback() {
        // Shape of User Guide Example 8.1-1: intervalPeriod at event level,
        // interval itself has only the payload.
        let events = json!([{
            "id": "alert-1",
            "programID": "prog-1",
            "eventName": "alertEvent",
            "intervalPeriod": { "start": "2026-03-14T00:00:00Z", "duration": "PT4H" },
            "intervals": [{
                "id": 0,
                "payloads": [{
                    "type": "ALERT_GRID_EMERGENCY",
                    "values": ["The grid is currently under emergency conditions"]
                }]
            }]
        }]);
        let alerts =
            parse_alert_windows(&serde_json::from_value::<Vec<OadrEvent>>(events).unwrap());
        assert_eq!(alerts.len(), 1);
        assert_eq!(alerts[0].alert_type, "ALERT_GRID_EMERGENCY");
        assert_eq!(alerts[0].event_id, "alert-1");
        assert_eq!(alerts[0].start.to_rfc3339(), "2026-03-14T00:00:00+00:00");
        assert_eq!((alerts[0].end - alerts[0].start).num_hours(), 4);
        assert!(alerts[0].message.contains("emergency"));
    }

    #[test]
    fn test_parse_alert_windows_interval_level_period_wins() {
        let events = json!([{
            "id": "alert-2",
            "programID": "prog-1",
            "intervalPeriod": { "start": "2026-03-14T00:00:00Z", "duration": "PT4H" },
            "intervals": [{
                "id": 0,
                "intervalPeriod": { "start": "2026-03-14T02:00:00Z", "duration": "PT30M" },
                "payloads": [{ "type": "ALERT_BLACK_START", "values": ["restoring"] }]
            }]
        }]);
        let alerts =
            parse_alert_windows(&serde_json::from_value::<Vec<OadrEvent>>(events).unwrap());
        assert_eq!(alerts.len(), 1);
        assert_eq!(alerts[0].alert_type, "ALERT_BLACK_START");
        assert_eq!(alerts[0].start.to_rfc3339(), "2026-03-14T02:00:00+00:00");
        assert_eq!((alerts[0].end - alerts[0].start).num_minutes(), 30);
    }

    #[test]
    fn test_parse_alert_windows_ignores_non_alert_events() {
        let events = json!([{
            "id": "evt-1",
            "programID": "prog-1",
            "intervals": [{
                "id": 0,
                "intervalPeriod": { "start": "2026-03-14T00:00:00Z", "duration": "PT1H" },
                "payloads": [{ "type": "PRICE", "values": [0.25] }]
            }]
        }]);
        let alerts =
            parse_alert_windows(&serde_json::from_value::<Vec<OadrEvent>>(events).unwrap());
        assert!(alerts.is_empty());
    }

    // ── parse_dispatch_windows / parse_charge_state_setpoint (WP3.4) ───────

    #[test]
    fn test_parse_dispatch_windows_extracts_setpoint_and_window() {
        let events = json!([{
            "id": "disp-1",
            "programID": "prog-1",
            "intervalPeriod": { "start": "2026-03-14T00:00:00Z", "duration": "PT15M" },
            "intervals": [{ "id": 0, "payloads": [{ "type": "DISPATCH_SETPOINT", "values": [1.5] }] }]
        }]);
        let w = parse_dispatch_windows(&serde_json::from_value::<Vec<OadrEvent>>(events).unwrap());
        assert_eq!(w.len(), 1);
        assert_eq!(w[0].setpoint_kw, 1.5);
        assert_eq!((w[0].end - w[0].start).num_minutes(), 15);
        assert_eq!(w[0].event_id, "disp-1");
    }

    #[test]
    fn test_parse_charge_state_setpoint_fraction_and_percent() {
        let make = |val: serde_json::Value| {
            json!([{
                "id": "cs-1",
                "programID": "prog-1",
                "intervalPeriod": { "start": "2026-03-14T00:00:00Z", "duration": "PT2H" },
                "intervals": [{ "id": 0, "payloads": [{ "type": "CHARGE_STATE_SETPOINT", "values": [val] }] }]
            }])
        };
        let parse = |v| {
            parse_charge_state_setpoint(&serde_json::from_value::<Vec<OadrEvent>>(make(v)).unwrap())
        };
        let (soc, end, eid) = parse(json!(0.9)).expect("fraction accepted");
        assert!((soc - 0.9).abs() < 1e-9);
        assert_eq!(eid, "cs-1");
        assert_eq!(end.to_rfc3339(), "2026-03-14T02:00:00+00:00");

        let (soc, _, _) = parse(json!(85)).expect("percent accepted");
        assert!((soc - 0.85).abs() < 1e-9);

        assert!(parse(json!("full")).is_none(), "non-numeric dropped");
    }

    // ── parse_capacity_state export subscription/reservation (WP3.3) ───────

    #[test]
    fn test_parse_capacity_state_export_subscription_and_reservation() {
        let events = json!([{
            "id": "cap-1",
            "programID": "prog-1",
            "intervals": [{
                "id": 0,
                "intervalPeriod": { "start": "2026-03-14T00:00:00Z", "duration": "PT1H" },
                "payloads": [
                    { "type": "EXPORT_CAPACITY_SUBSCRIPTION", "values": [4.0] },
                    { "type": "EXPORT_CAPACITY_RESERVATION", "values": [2.0] }
                ]
            }]
        }]);
        let cap = parse_capacity_state(
            &serde_json::from_value::<Vec<OadrEvent>>(events).unwrap(),
            Utc::now(),
        );
        assert_eq!(cap.export_subscription_kw, Some(4.0));
        assert_eq!(cap.export_reservation_kw, Some(2.0));
    }

    // ── parse_simple_windows (WP3.2) ────────────────────────────────────────

    #[test]
    fn test_parse_simple_windows_extracts_levels_and_window() {
        let events = json!([{
            "id": "simple-1",
            "programID": "prog-1",
            "intervalPeriod": { "start": "2026-03-14T00:00:00Z", "duration": "PT30M" },
            "intervals": [{ "id": 0, "payloads": [{ "type": "SIMPLE", "values": [2] }] }]
        }]);
        let windows =
            parse_simple_windows(&serde_json::from_value::<Vec<OadrEvent>>(events).unwrap());
        assert_eq!(windows.len(), 1);
        assert_eq!(windows[0].level, 2);
        assert_eq!(windows[0].event_id, "simple-1");
        assert_eq!((windows[0].end - windows[0].start).num_minutes(), 30);
    }

    #[test]
    fn test_parse_simple_windows_drops_level_zero_and_out_of_range() {
        let events = json!([{
            "id": "simple-2",
            "programID": "prog-1",
            "intervalPeriod": { "start": "2026-03-14T00:00:00Z", "duration": "PT1H" },
            "intervals": [
                { "id": 0, "payloads": [{ "type": "SIMPLE", "values": [0] }] },
                { "id": 1, "payloads": [{ "type": "SIMPLE", "values": [7] }] },
                { "id": 2, "payloads": [{ "type": "SIMPLE", "values": ["high"] }] }
            ]
        }]);
        let windows =
            parse_simple_windows(&serde_json::from_value::<Vec<OadrEvent>>(events).unwrap());
        assert!(windows.is_empty());
    }

    #[test]
    fn test_parse_simple_windows_ignores_non_simple_payloads() {
        let events = json!([{
            "id": "evt-1",
            "programID": "prog-1",
            "intervals": [{
                "id": 0,
                "intervalPeriod": { "start": "2026-03-14T00:00:00Z", "duration": "PT1H" },
                "payloads": [{ "type": "PRICE", "values": [0.25] }]
            }]
        }]);
        let windows =
            parse_simple_windows(&serde_json::from_value::<Vec<OadrEvent>>(events).unwrap());
        assert!(windows.is_empty());
    }

    #[test]
    fn test_parse_alert_windows_skips_unresolvable_start() {
        // No intervalPeriod anywhere — window can't be resolved, alert skipped
        // rather than guessed.
        let events = json!([{
            "id": "alert-3",
            "programID": "prog-1",
            "intervals": [{
                "id": 0,
                "payloads": [{ "type": "ALERT_GRID_EMERGENCY", "values": ["no window"] }]
            }]
        }]);
        let alerts =
            parse_alert_windows(&serde_json::from_value::<Vec<OadrEvent>>(events).unwrap());
        assert!(alerts.is_empty());
    }

    #[test]
    fn test_parse_rate_snapshots_price() {
        let events = json!([
            {
                "id": "evt-1",
                "programID": "prog-1",
                "eventName": "price-event",
                "intervals": [
                    {
                        "id": 0,
                        "intervalPeriod": {
                            "start": "2025-01-01T14:00:00Z",
                            "duration": "PT1H"
                        },
                        "payloads": [
                            {"type": "PRICE", "values": [0.25]}
                        ]
                    },
                    {
                        "id": 1,
                        "intervalPeriod": {
                            "start": "2025-01-01T15:00:00Z",
                            "duration": "PT1H"
                        },
                        "payloads": [
                            {"type": "PRICE", "values": [0.30]}
                        ]
                    },
                    {
                        "id": 2,
                        "intervalPeriod": {
                            "start": "2025-01-01T16:00:00Z",
                            "duration": "PT1H"
                        },
                        "payloads": [
                            {"type": "PRICE", "values": [0.35]}
                        ]
                    }
                ]
            }
        ]);
        let snapshots = parse_rate_snapshots(
            &serde_json::from_value::<Vec<OadrEvent>>(events).unwrap(),
            Utc::now(),
        );
        assert_eq!(snapshots.len(), 3);
        assert_eq!(snapshots[0].import_tariff_eur_kwh, Some(0.25));
        assert_eq!(snapshots[1].import_tariff_eur_kwh, Some(0.30));
        assert_eq!(snapshots[2].import_tariff_eur_kwh, Some(0.35));
    }

    #[test]
    fn test_parse_rate_snapshots_ghg() {
        let events = json!([
            {
                "id": "evt-ghg",
                "programID": "prog-1",
                "intervals": [
                    {
                        "id": 0,
                        "intervalPeriod": {
                            "start": "2025-01-01T10:00:00Z",
                            "duration": "PT1H"
                        },
                        "payloads": [
                            {"type": "GHG", "values": [200.0]}
                        ]
                    }
                ]
            }
        ]);
        let snapshots = parse_rate_snapshots(
            &serde_json::from_value::<Vec<OadrEvent>>(events).unwrap(),
            Utc::now(),
        );
        assert_eq!(snapshots.len(), 1);
        assert_eq!(snapshots[0].co2_g_kwh, Some(200.0));
    }

    #[test]
    fn test_parse_rate_snapshots_export_price() {
        let events = json!([
            {
                "id": "evt-export",
                "programID": "prog-1",
                "intervals": [
                    {
                        "id": 0,
                        "intervalPeriod": {
                            "start": "2025-01-01T12:00:00Z",
                            "duration": "PT1H"
                        },
                        "payloads": [
                            {"type": "EXPORT_PRICE", "values": [0.10]}
                        ]
                    }
                ]
            }
        ]);
        let snapshots = parse_rate_snapshots(
            &serde_json::from_value::<Vec<OadrEvent>>(events).unwrap(),
            Utc::now(),
        );
        assert_eq!(snapshots.len(), 1);
        assert_eq!(snapshots[0].export_tariff_eur_kwh, Some(0.10));
    }

    #[test]
    fn test_parse_capacity_schedule_keeps_per_interval_limits() {
        let events = json!([
            {
                "id": "evt-cap-sched",
                "programID": "prog-1",
                "intervals": [
                    {
                        "id": 0,
                        "intervalPeriod": {
                            "start": "2025-01-01T10:00:00Z",
                            "duration": "PT1H"
                        },
                        "payloads": [
                            {"type": "IMPORT_CAPACITY_LIMIT", "values": [5.0]}
                        ]
                    },
                    {
                        "id": 1,
                        "intervalPeriod": {
                            "start": "2025-01-01T11:00:00Z",
                            "duration": "PT1H"
                        },
                        "payloads": [
                            {"type": "IMPORT_CAPACITY_LIMIT", "values": [3.0]},
                            {"type": "EXPORT_CAPACITY_LIMIT", "values": [2.0]}
                        ]
                    }
                ]
            }
        ]);
        let snapshots = parse_capacity_schedule(
            &serde_json::from_value::<Vec<OadrEvent>>(events).unwrap(),
            Utc::now(),
        );
        // Unlike parse_capacity_state (which collapses to the strictest single value),
        // the schedule keeps both intervals with their own distinct limits.
        assert_eq!(snapshots.len(), 2);
        assert_eq!(snapshots[0].import_limit_kw, Some(5.0));
        assert_eq!(snapshots[0].export_limit_kw, None);
        assert_eq!(snapshots[1].import_limit_kw, Some(3.0));
        assert_eq!(snapshots[1].export_limit_kw, Some(2.0));
    }

    #[test]
    fn test_parse_capacity_schedule_ignores_non_capacity_payloads() {
        let events = json!([
            {
                "id": "evt-price-only",
                "programID": "prog-1",
                "intervals": [
                    {
                        "id": 0,
                        "intervalPeriod": {
                            "start": "2025-01-01T10:00:00Z",
                            "duration": "PT1H"
                        },
                        "payloads": [
                            {"type": "PRICE", "values": [0.25]}
                        ]
                    }
                ]
            }
        ]);
        let snapshots = parse_capacity_schedule(
            &serde_json::from_value::<Vec<OadrEvent>>(events).unwrap(),
            Utc::now(),
        );
        assert!(snapshots.is_empty());
    }

    #[test]
    fn test_parse_capacity_state_import_limit() {
        let events = json!([
            {
                "id": "evt-cap",
                "programID": "prog-1",
                "intervals": [
                    {
                        "id": 0,
                        "intervalPeriod": {
                            "start": "2025-01-01T10:00:00Z",
                            "duration": "PT1H"
                        },
                        "payloads": [
                            {"type": "IMPORT_CAPACITY_LIMIT", "values": [5.0]}
                        ]
                    }
                ]
            }
        ]);
        let now = Utc.with_ymd_and_hms(2025, 1, 1, 9, 0, 0).unwrap();
        let cap = parse_capacity_state(
            &serde_json::from_value::<Vec<OadrEvent>>(events).unwrap(),
            now,
        );
        assert_eq!(cap.import_limit_kw, Some(5.0));
        assert_eq!(cap.import_limit_event_id, Some("evt-cap".to_string()));
        assert_eq!(
            cap.last_updated,
            Some(now),
            "last_updated must equal the injected clock, not wall-clock Utc::now()"
        );
    }

    #[test]
    fn test_parse_capacity_state_strictest_wins() {
        let events = json!([
            {
                "id": "evt-a",
                "programID": "prog-1",
                "intervals": [{
                    "id": 0,
                    "intervalPeriod": {"start": "2025-01-01T10:00:00Z", "duration": "PT1H"},
                    "payloads": [{"type": "IMPORT_CAPACITY_LIMIT", "values": [10.0]}]
                }]
            },
            {
                "id": "evt-b",
                "programID": "prog-1",
                "intervals": [{
                    "id": 0,
                    "intervalPeriod": {"start": "2025-01-01T10:00:00Z", "duration": "PT1H"},
                    "payloads": [{"type": "IMPORT_CAPACITY_LIMIT", "values": [3.0]}]
                }]
            }
        ]);
        let cap = parse_capacity_state(
            &serde_json::from_value::<Vec<OadrEvent>>(events).unwrap(),
            Utc::now(),
        );
        assert_eq!(cap.import_limit_kw, Some(3.0));
        assert_eq!(cap.import_limit_event_id, Some("evt-b".to_string()));
    }

    #[test]
    fn test_extract_report_obligations_empty_when_no_descriptors() {
        let events = json!([
            {
                "id": "evt-1",
                "programID": "prog-1",
                "intervals": []
            }
        ]);
        let now = Utc::now();
        let obligations = extract_report_obligations(
            &serde_json::from_value::<Vec<OadrEvent>>(events).unwrap(),
            now,
            &[],
        );
        assert!(obligations.is_empty());
    }

    #[test]
    fn test_extract_report_obligations_with_descriptor() {
        let events = json!([
            {
                "id": "evt-1",
                "programID": "prog-1",
                "reportDescriptors": [
                    {
                        "payloadType": "USAGE",
                        "readingType": "DIRECT_READ"
                    }
                ],
                "intervals": []
            }
        ]);
        let now = Utc::now();
        let obligations = extract_report_obligations(
            &serde_json::from_value::<Vec<OadrEvent>>(events).unwrap(),
            now,
            &[],
        );
        assert_eq!(obligations.len(), 1);
        assert_eq!(obligations[0].payload_type, "USAGE");
        assert_eq!(obligations[0].reading_type, "DIRECT_READ");
        assert!(!obligations[0].fulfilled);
        assert!(
            obligations[0].historical,
            "absent reportDescriptor.historical defaults to true per spec"
        );
    }

    #[test]
    fn extract_report_obligations_parses_historical_false() {
        // R-15: `historical: false` marks the obligation as a forecast request.
        let events = json!([{
            "id": "evt-h",
            "programID": "prog-1",
            "reportDescriptors": [
                {"payloadType": "USAGE", "frequency": 900, "historical": false}
            ]
        }]);
        let obligations = extract_report_obligations(
            &serde_json::from_value::<Vec<OadrEvent>>(events).unwrap(),
            Utc::now(),
            &[],
        );
        assert_eq!(obligations.len(), 1);
        assert!(!obligations[0].historical);
    }

    #[test]
    fn test_parse_rate_snapshots_no_loop_when_duration_equals_cycle() {
        // event.intervalPeriod.duration == sum of intervals → no looping
        let now: DateTime<Utc> = "2026-03-17T12:00:00Z".parse().unwrap();
        let events = json!([{
            "id": "evt-noloop",
            "programID": "prog-1",
            "intervalPeriod": {
                "start": "2026-03-17T00:00:00Z",
                "duration": "PT2H"
            },
            "intervals": [
                {
                    "id": 0,
                    "intervalPeriod": {"start": "2026-03-17T00:00:00Z", "duration": "PT1H"},
                    "payloads": [{"type": "PRICE", "values": [0.10]}]
                },
                {
                    "id": 1,
                    "intervalPeriod": {"start": "2026-03-17T01:00:00Z", "duration": "PT1H"},
                    "payloads": [{"type": "PRICE", "values": [0.20]}]
                }
            ]
        }]);
        let snapshots = parse_rate_snapshots(
            &serde_json::from_value::<Vec<OadrEvent>>(events).unwrap(),
            now,
        );
        assert_eq!(
            snapshots.len(),
            2,
            "no looping expected when duration == cycle"
        );
    }

    #[test]
    fn test_parse_rate_snapshots_looping_covers_now() {
        // 2-hour cycle starting 2026-01-01, P9999Y duration → should loop
        // now is 2 days later — original intervals are long expired
        let now: DateTime<Utc> = "2026-01-03T00:30:00Z".parse().unwrap();
        let events = json!([{
            "id": "evt-loop",
            "programID": "prog-1",
            "intervalPeriod": {"start": "2026-01-01T00:00:00Z", "duration": "P9999Y"},
            "intervals": [
                {
                    "id": 0,
                    "intervalPeriod": {"start": "2026-01-01T00:00:00Z", "duration": "PT1H"},
                    "payloads": [{"type": "PRICE", "values": [0.10]}]
                },
                {
                    "id": 1,
                    "intervalPeriod": {"start": "2026-01-01T01:00:00Z", "duration": "PT1H"},
                    "payloads": [{"type": "PRICE", "values": [0.20]}]
                }
            ]
        }]);
        let snapshots = parse_rate_snapshots(
            &serde_json::from_value::<Vec<OadrEvent>>(events).unwrap(),
            now,
        );

        // More than 2 intervals: looping occurred
        assert!(
            snapshots.len() > 2,
            "expected looped intervals, got {}",
            snapshots.len()
        );

        // An interval must cover now (2026-01-03T00:30 → cycle 24, interval 0: 00:00–01:00)
        let current = snapshots
            .iter()
            .find(|s| s.interval_start <= now && now < s.interval_end);
        assert!(current.is_some(), "no interval covers now");
        assert_eq!(current.unwrap().import_tariff_eur_kwh, Some(0.10));
    }

    #[test]
    fn test_parse_rate_snapshots_looping_has_future_intervals() {
        let now: DateTime<Utc> = "2026-01-03T00:30:00Z".parse().unwrap();
        let events = json!([{
            "id": "evt-loop",
            "programID": "prog-1",
            "intervalPeriod": {"start": "2026-01-01T00:00:00Z", "duration": "P9999Y"},
            "intervals": [
                {
                    "id": 0,
                    "intervalPeriod": {"start": "2026-01-01T00:00:00Z", "duration": "PT1H"},
                    "payloads": [{"type": "PRICE", "values": [0.10]}]
                },
                {
                    "id": 1,
                    "intervalPeriod": {"start": "2026-01-01T01:00:00Z", "duration": "PT1H"},
                    "payloads": [{"type": "PRICE", "values": [0.20]}]
                }
            ]
        }]);
        let snapshots = parse_rate_snapshots(
            &serde_json::from_value::<Vec<OadrEvent>>(events).unwrap(),
            now,
        );
        assert!(
            snapshots.iter().any(|s| s.interval_start > now),
            "expected at least one future interval"
        );
    }

    #[test]
    fn test_parse_rate_snapshots_looping_24h_cycle() {
        // 24 hourly intervals (like the seed price event), P9999Y → daily repeat
        // now is 2 days + 14.5 h after base midnight
        let now: DateTime<Utc> = "2026-01-03T14:30:00Z".parse().unwrap();

        let intervals: Vec<serde_json::Value> = (0u32..24)
            .map(|h| {
                json!({
                    "id": h,
                    "intervalPeriod": {
                        "start": format!("2026-01-01T{:02}:00:00Z", h),
                        "duration": "PT1H"
                    },
                    "payloads": [{"type": "PRICE", "values": [h as f64]}]
                })
            })
            .collect();

        let events = json!([{
            "id": "evt-daily",
            "programID": "prog-1",
            "intervalPeriod": {"start": "2026-01-01T00:00:00Z", "duration": "P9999Y"},
            "intervals": intervals
        }]);
        let snapshots = parse_rate_snapshots(
            &serde_json::from_value::<Vec<OadrEvent>>(events).unwrap(),
            now,
        );

        assert!(
            snapshots.len() > 24,
            "expected more than 24 intervals (looping), got {}",
            snapshots.len()
        );

        // now = 2026-01-03T14:30 → cycle 2 (offset 2×86400s), hour 14 → price = 14.0
        let current = snapshots
            .iter()
            .find(|s| s.interval_start <= now && now < s.interval_end);
        assert!(current.is_some(), "no interval covers now at {}", now);
        assert_eq!(current.unwrap().import_tariff_eur_kwh, Some(14.0));
    }

    // ── BL-02: priority-ordered merge ────────────────────────────────────

    fn price_event(
        id: &str,
        priority: Option<i64>,
        created: Option<&str>,
        price: f64,
    ) -> OadrEvent {
        serde_json::from_value(json!({
            "id": id,
            "programID": "prog-1",
            "priority": priority,
            "createdDateTime": created,
            "intervals": [{
                "id": 0,
                "intervalPeriod": {"start": "2026-02-01T10:00:00Z", "duration": "PT1H"},
                "payloads": [{"type": "PRICE", "values": [price]}]
            }]
        }))
        .unwrap()
    }

    #[test]
    fn test_parse_rate_snapshots_higher_priority_wins_regardless_of_order() {
        let now = Utc::now();
        let high = price_event("evt-high", Some(1), Some("2026-01-01T00:00:00Z"), 0.50);
        let low = price_event("evt-low", Some(5), Some("2026-01-01T00:00:00Z"), 0.10);

        for events in [
            vec![high.clone(), low.clone()],
            vec![low.clone(), high.clone()],
        ] {
            let snapshots = parse_rate_snapshots(&events, now);
            assert_eq!(snapshots.len(), 1);
            assert_eq!(
                snapshots[0].import_tariff_eur_kwh,
                Some(0.50),
                "priority 1 must beat priority 5 regardless of array order"
            );
        }
    }

    #[test]
    fn test_parse_rate_snapshots_equal_priority_newer_created_wins() {
        let now = Utc::now();
        let newer = price_event("evt-new", Some(2), Some("2026-02-01T08:00:00Z"), 0.40);
        let older = price_event("evt-old", Some(2), Some("2026-01-15T08:00:00Z"), 0.20);

        // older last in the array — would win under naive last-write-wins
        let snapshots = parse_rate_snapshots(&[newer, older], now);
        assert_eq!(snapshots.len(), 1);
        assert_eq!(
            snapshots[0].import_tariff_eur_kwh,
            Some(0.40),
            "newer createdDateTime must win at equal priority"
        );
    }

    #[test]
    fn test_parse_rate_snapshots_absent_priority_sorts_last() {
        let now = Utc::now();
        let explicit = price_event("evt-p5", Some(5), Some("2026-01-01T00:00:00Z"), 0.30);
        let none = price_event("evt-none", None, Some("2026-02-01T00:00:00Z"), 0.99);

        // None-priority event last in the array — would win under naive last-write-wins
        let snapshots = parse_rate_snapshots(&[explicit, none], now);
        assert_eq!(snapshots.len(), 1);
        assert_eq!(
            snapshots[0].import_tariff_eur_kwh,
            Some(0.30),
            "an event with any explicit priority must beat one without"
        );
    }

    #[test]
    fn test_extract_report_obligations_dedup() {
        // Dedup by (event_id, payload_type) is intentionally unconditional — it does
        // not check `fulfilled`. Recurrence (R6) is handled by re-arming the *same*
        // obligation's `due_at` in place (`AppState::rearm_obligation`), not by letting
        // this function regenerate a fresh one each cycle; regenerating here would
        // orphan the original (and its VTN report name history) instead of advancing
        // it. See docs/plans/review_items_resolution_strategy.md R6.
        let events = json!([
            {
                "id": "evt-1",
                "programID": "prog-1",
                "reportDescriptors": [
                    {"payloadType": "USAGE", "readingType": "DIRECT_READ"}
                ],
                "intervals": []
            }
        ]);
        let now = Utc::now();
        // Simulate already having an obligation for this event+type
        let existing = vec![OadrReportObligation {
            id: Uuid::new_v4(),
            event_id: "evt-1".to_string(),
            program_id: Some("prog-1".to_string()),
            payload_type: "USAGE".to_string(),
            reading_type: "DIRECT_READ".to_string(),
            resource_name: None,
            due_at: now,
            interval_duration_s: 3600,
            fulfilled: false,
            created_at: now,
            historical: true,
        }];
        let obligations = extract_report_obligations(
            &serde_json::from_value::<Vec<OadrEvent>>(events).unwrap(),
            now,
            &existing,
        );
        // Should not add a duplicate
        assert!(obligations.is_empty());
    }

    #[test]
    fn test_extract_report_obligations_frequency_field() {
        let events = json!([
            {
                "id": "evt-1",
                "programID": "prog-1",
                "reportDescriptors": [
                    {"payloadType": "USAGE", "readingType": "DIRECT_READ", "frequency": 900}
                ],
                "intervals": []
            }
        ]);
        let now = Utc::now();
        let obligations = extract_report_obligations(
            &serde_json::from_value::<Vec<OadrEvent>>(events).unwrap(),
            now,
            &[],
        );
        assert_eq!(obligations.len(), 1);
        assert_eq!(obligations[0].interval_duration_s, 900);
        assert_eq!(obligations[0].due_at, now + Duration::seconds(900));
    }
}
