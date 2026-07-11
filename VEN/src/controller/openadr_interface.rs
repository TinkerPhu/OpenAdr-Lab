use chrono::{DateTime, Duration, Utc};
use uuid::Uuid;

use crate::common::parse_iso8601_duration_secs;
use crate::controller::vtn_port::OadrEvent;
use crate::entities::capacity::{AlertWindow, OadrCapacityState, OadrReportObligation};
use crate::entities::tariff_snapshot::TariffSnapshot;

// ---------------------------------------------------------------------------
// Rate snapshot parsing
// ---------------------------------------------------------------------------

/// Parse all rate snapshots from a slice of OpenADR events.
/// Handles PRICE, EXPORT_PRICE, GHG payload types per event interval.
/// Multiple payload types for the same interval are merged into one TariffSnapshot.
///
/// Supports looping events: when `event.intervalPeriod.duration` exceeds the total
/// span of all intervals, the interval set is repeated (offset by one cycle each time)
/// to cover [now − 1 cycle … now + 3 days]. This implements the OpenADR 3 spec's
/// "persistent daily prices" pattern (`event.intervalPeriod.duration = "P9999Y"`).
pub fn parse_rate_snapshots(events: &[OadrEvent], now: DateTime<Utc>) -> Vec<TariffSnapshot> {
    let mut map: std::collections::BTreeMap<(i64, i64), TariffSnapshot> =
        std::collections::BTreeMap::new();

    // ── BL-02: priority-ordered merge ───────────────────────────────────────
    // OpenADR 3 spec (§ 6.6): event `priority` — lower number = higher priority; an
    // absent priority is treated as lowest. Sort ascending by "wins last" order so the
    // last-write-wins merge below naturally lets the higher-priority event survive:
    // lowest-priority events (including `None`) are processed first, highest-priority
    // last. Equal priority breaks the tie on `createdDateTime` — newer wins, so older
    // events are processed first.
    let mut ordered: Vec<&OadrEvent> = events.iter().collect();
    ordered.sort_by(|a, b| {
        let pa = a.priority.unwrap_or(i64::MAX);
        let pb = b.priority.unwrap_or(i64::MAX);
        pb.cmp(&pa).then_with(|| {
            let created = |e: &OadrEvent| {
                e.createdDateTime
                    .as_deref()
                    .and_then(|s| s.parse::<DateTime<Utc>>().ok())
                    .unwrap_or(DateTime::<Utc>::MIN_UTC)
            };
            created(a).cmp(&created(b))
        })
    });

    for event in ordered {
        if event.intervals.is_empty() {
            continue;
        }

        // ── Collect base intervals ────────────────────────────────────────────
        type IntervalEntry = (DateTime<Utc>, i64, Vec<(String, f64)>);
        let mut base: Vec<IntervalEntry> = Vec::new();

        for interval in &event.intervals {
            let ip = match interval.intervalPeriod.as_ref() {
                Some(ip) => ip,
                None => continue,
            };
            let start_str = match ip.start.as_deref() {
                Some(s) => s,
                None => continue,
            };
            let interval_start: DateTime<Utc> = match start_str.parse() {
                Ok(dt) => dt,
                Err(_) => continue,
            };
            let duration_secs =
                parse_iso8601_duration_secs(ip.duration.as_deref().unwrap_or("PT1H"));

            let mut payloads: Vec<(String, f64)> = Vec::new();
            for p in &interval.payloads {
                let t = p.r#type.as_str();
                let v = p.values.first().and_then(|v| v.as_f64());
                if matches!(t, "PRICE" | "EXPORT_PRICE" | "GHG") {
                    if let Some(val) = v {
                        payloads.push((t.to_string(), val));
                    }
                }
            }

            base.push((interval_start, duration_secs, payloads));
        }

        if base.is_empty() {
            continue;
        }

        // ── Determine looping offsets ─────────────────────────────────────────
        let first_start = base.iter().map(|(s, _, _)| *s).min().unwrap();
        let last_end = base
            .iter()
            .map(|(s, d, _)| *s + Duration::seconds(*d))
            .max()
            .unwrap();
        let cycle_secs = (last_end - first_start).num_seconds();

        let event_dur_secs = event
            .intervalPeriod
            .as_ref()
            .and_then(|ip| ip.duration.as_deref())
            .map(parse_iso8601_duration_secs)
            .unwrap_or(cycle_secs);

        let offsets: Vec<i64> = if cycle_secs > 0 && event_dur_secs > cycle_secs {
            let elapsed = (now - first_start).num_seconds().max(0);
            let n = elapsed / cycle_secs; // index of the cycle that contains now
            let from = n.saturating_sub(1); // one cycle back for "most recent past" fallback
            let ahead = (3 * 86400i64) / cycle_secs + 2; // cycles needed to cover 3 days ahead
            let to = (from + ahead).min(from + 10); // hard cap: at most 11 cycles total
            (from..=to).map(|k| k * cycle_secs).collect()
        } else {
            vec![0i64]
        };

        // ── Insert snapshots into map for each offset ─────────────────────────
        for &offset in &offsets {
            for (base_start, dur, payloads) in &base {
                let start = *base_start + Duration::seconds(offset);
                let end = start + Duration::seconds(*dur);
                let key = (start.timestamp(), end.timestamp());

                // CONFLICT NOTE: Multiple active events can define prices for the same interval
                // (e.g. one PRICE event + one GHG event, or two PRICE events from different programs).
                // This merge uses last-write-wins: whichever event is processed last in the loop
                // overwrites a previously-set value for the same field. `ordered` above is sorted
                // so the highest-priority event (BL-02) is processed last and therefore wins.
                let entry = map.entry(key).or_insert_with(|| TariffSnapshot {
                    interval_start: start,
                    interval_end: end,
                    import_tariff_eur_kwh: None,
                    export_tariff_eur_kwh: None,
                    co2_g_kwh: None,
                });

                for (t, v) in payloads {
                    match t.as_str() {
                        "PRICE" => entry.import_tariff_eur_kwh = Some(*v),
                        "EXPORT_PRICE" => entry.export_tariff_eur_kwh = Some(*v),
                        "GHG" => entry.co2_g_kwh = Some(*v),
                        _ => {}
                    }
                }
            }
        }
    }

    let mut result: Vec<TariffSnapshot> = map
        .into_values()
        .filter(|s| {
            s.import_tariff_eur_kwh.is_some()
                || s.export_tariff_eur_kwh.is_some()
                || s.co2_g_kwh.is_some()
        })
        .collect();

    result.sort_by_key(|s| s.interval_start);
    result
}

// ---------------------------------------------------------------------------
// Capacity state parsing
// ---------------------------------------------------------------------------

/// Parse capacity limits from the CURRENT set of active events.
/// Computed from scratch on each call — reflects the live VTN state.
/// Strictest limit wins (lowest value when multiple events specify same field).
pub fn parse_capacity_state(events: &[OadrEvent]) -> OadrCapacityState {
    let mut existing = OadrCapacityState::default();
    let mut import_limit: Option<(f64, String)> = None;
    let mut export_limit: Option<(f64, String)> = None;
    let mut import_sub: Option<f64> = None;
    let mut import_res: Option<f64> = None;
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
        existing.last_updated = Some(Utc::now());
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
        let cap = parse_capacity_state(&serde_json::from_value::<Vec<OadrEvent>>(events).unwrap());
        assert_eq!(cap.import_limit_kw, Some(5.0));
        assert_eq!(cap.import_limit_event_id, Some("evt-cap".to_string()));
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
        let cap = parse_capacity_state(&serde_json::from_value::<Vec<OadrEvent>>(events).unwrap());
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
