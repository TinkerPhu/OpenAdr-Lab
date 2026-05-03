/// Controller reporter: builds OpenADR VEN report payloads.
///
/// Two modes:
///   - Measurement reports (timer-driven): one TELEMETRY_USAGE report per active event.
///   - Status reports (event-driven): TELEMETRY_STATUS triggered by PlanCycle/PacketTransition.
use chrono::{DateTime, Duration, Utc};
use serde_json::{json, Value};
use tracing::debug;

use crate::assets::{AssetState, HistoryPoint};
use crate::common::{parse_iso8601_duration_secs, Aggregation, Interpolation, TimeSeries};
use crate::controller::trace::ControllerEvent;
use crate::entities::capacity::OadrReportObligation;
use crate::entities::plan::SiteFlexibilityEnvelope;
use crate::simulator::SimState;

// ---------------------------------------------------------------------------
// HistoryPoint → TimeSeries conversion helpers
// ---------------------------------------------------------------------------

/// Build a `TimeSeries` of `power_kw` from a slice of `HistoryPoint`.
fn points_to_power_ts(points: &[HistoryPoint], interpolation: Interpolation) -> TimeSeries {
    let samples: Vec<(DateTime<Utc>, f64)> = points.iter().map(|p| (p.ts, p.power_kw)).collect();
    TimeSeries {
        samples,
        interpolation,
    }
}

/// Extract SoC (0.0–1.0) from a `HistoryPoint`'s state, if the asset is EV or Battery.
fn soc_from_point(p: &HistoryPoint) -> Option<f64> {
    p.state.soc()
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

// ---------------------------------------------------------------------------
// Latest asset power helpers
// ---------------------------------------------------------------------------

/// Sum the most-recent `power_kw` across all assets that are currently importing
/// (positive power). Returns 0.0 if history is empty or all assets are exporting.
fn latest_net_import_kw(sim: &SimState) -> f64 {
    sim.assets
        .iter()
        .filter_map(|e| e.history.latest().map(|p| p.power_kw))
        .filter(|&kw| kw > 0.0)
        .sum()
}

/// Sum the most-recent `power_kw` across all assets that are currently exporting
/// (negative power), returned as a positive magnitude.
fn latest_net_export_kw(sim: &SimState) -> f64 {
    sim.assets
        .iter()
        .filter_map(|e| e.history.latest().map(|p| p.power_kw))
        .filter(|&kw| kw < 0.0)
        .map(|kw| -kw)
        .sum()
}

/// Build a TELEMETRY_USAGE measurement report for a single active OpenADR event.
///
/// The report includes:
///   - Net site import power (sum of positive asset power_kw from latest history row).
///   - OPERATING_STATE = "ACTIVE".
///   - STORAGE_CHARGE_LEVEL (EV SoC %) if EV history is available.
///
/// Returns None if the event has no id or programID.
pub fn build_measurement_report(event: &Value, sim: &SimState, ven_name: &str) -> Option<Value> {
    let event_id = event.get("id").and_then(|v| v.as_str())?;
    let program_id = event.get("programID").and_then(|v| v.as_str())?;

    let report_name = format!("auto-{}-{}", ven_name, event_id);
    let resource_name = format!("{}-meter", ven_name);

    let net_import_kw = latest_net_import_kw(sim);
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
            let export_kw = latest_net_export_kw(sim);
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

    // Add EV SoC if available
    if let Some(ev_entry) = sim.asset("ev") {
        if let Some(last) = ev_entry.history.latest() {
            if let Some(soc) = soc_from_point(last) {
                payloads.push(json!({
                    "type": "STORAGE_CHARGE_LEVEL",
                    "values": [format!("{:.1}", soc * 100.0)]
                }));
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

    debug!(
        report_name,
        event_id, report_type, report_value, "built measurement report"
    );
    Some(report)
}

/// Build measurement reports for all currently active events (timer-driven entry point).
pub fn build_measurement_reports_for_active_events(
    events: &[Value],
    sim: &SimState,
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
        let descriptors = event.get("reportDescriptors").and_then(|v| v.as_array());
        let has_descriptors = descriptors.map_or(false, |arr| !arr.is_empty());
        if has_descriptors {
            let event_id = event.get("id").and_then(|v| v.as_str()).unwrap_or("?");
            debug!(
                event_id,
                "timer-driven: skipping event with reportDescriptors"
            );
            continue;
        }
        if let Some(event_id) = event.get("id").and_then(|v| v.as_str()) {
            if seen.insert(event_id.to_string()) {
                debug!(event_id, "timer-driven: building single-interval report");
                if let Some(report) = build_measurement_report(event, sim, ven_name) {
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
    sim: &SimState,
    ven_name: &str,
    site_envelope: Option<&SiteFlexibilityEnvelope>,
) -> Option<Value> {
    let program_id = obligation.program_id.as_deref()?;
    let event_id = &obligation.event_id;

    let report_name = format!("ob-{}-{}-{}", ven_name, event_id, obligation.payload_type);
    let resource_name = format!("{}-meter", ven_name);
    let interval_width = Duration::seconds(obligation.interval_duration_s as i64);
    let duration_iso = format_iso8601_duration(obligation.interval_duration_s);

    // Build net site power TimeSeries (sum all assets' power_kw)
    let net_power_ts = build_net_site_power_ts(sim);

    let payload_type = &obligation.payload_type;

    let intervals: Vec<Value> = match payload_type.as_str() {
        "STORAGE_CHARGE_STATE" | "STORAGE_CHARGE_LEVEL" => {
            build_soc_intervals(sim, interval_width, &duration_iso)
        }
        "IMPORT_CAPACITY_RESERVATION" => {
            let up_w = site_envelope.map(|e| e.up_kw * 1000.0).unwrap_or(0.0);
            vec![json!({
                "id": 0,
                "payloads": [
                    {"type": "IMPORT_CAPACITY_RESERVATION", "values": [up_w]},
                    {"type": "OPERATING_STATE", "values": ["ACTIVE"]}
                ]
            })]
        }
        "EXPORT_CAPACITY_RESERVATION" => {
            let down_w = site_envelope.map(|e| e.down_kw * 1000.0).unwrap_or(0.0);
            vec![json!({
                "id": 0,
                "payloads": [
                    {"type": "EXPORT_CAPACITY_RESERVATION", "values": [down_w]},
                    {"type": "OPERATING_STATE", "values": ["ACTIVE"]}
                ]
            })]
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

/// Sum all assets' `power_kw` into a single net site power `TimeSeries`.
///
/// Uses LOCF cross-asset aggregation: for each unique timestamp across all
/// asset histories, sums each asset's `power_at(t)`.
fn build_net_site_power_ts(sim: &SimState) -> TimeSeries {
    let now = Utc::now();
    let full_window = Duration::hours(2); // wide enough to cover any reporting interval

    let mut per_asset: Vec<TimeSeries> = sim
        .assets
        .iter()
        .map(|e| {
            let points = e.history.slice(full_window, now);
            points_to_power_ts(&points, Interpolation::Step)
        })
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
fn build_soc_intervals(sim: &SimState, interval_width: Duration, duration_iso: &str) -> Vec<Value> {
    let now = Utc::now();
    let full_window = Duration::hours(2);

    // Look for EV or battery SoC timeseries
    let soc_ts = sim
        .asset("ev")
        .map(|e| {
            let points = e.history.slice(full_window, now);
            let samples: Vec<(DateTime<Utc>, f64)> = points
                .iter()
                .filter_map(|p| soc_from_point(p).map(|soc| (p.ts, soc)))
                .collect();
            TimeSeries {
                samples,
                interpolation: Interpolation::Step,
            }
        })
        .filter(|ts| !ts.samples.is_empty())
        .or_else(|| {
            sim.asset("battery")
                .map(|e| {
                    let points = e.history.slice(full_window, now);
                    let samples: Vec<(DateTime<Utc>, f64)> = points
                        .iter()
                        .filter_map(|p| soc_from_point(p).map(|soc| (p.ts, soc)))
                        .collect();
                    TimeSeries {
                        samples,
                        interpolation: Interpolation::Step,
                    }
                })
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
/// Returns None when no active OpenADR event provides a `programID` (VTN requires it)
/// or for unhandled event types.
pub fn build_status_report(
    event: &ControllerEvent,
    sim: &SimState,
    ven_name: &str,
    program_id: Option<&str>,
    _now: DateTime<Utc>,
) -> Option<Value> {
    let program_id = program_id?;

    let (description, asset_id_opt) = match event {
        ControllerEvent::PlanCycle {
            trigger_reason,
            total_slots,
            ..
        } => (
            format!("PlanCycle trigger={} slots={}", trigger_reason, total_slots),
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

    let net_import_kw = latest_net_import_kw(sim);

    let resource_name = asset_id_opt
        .as_deref()
        .map(|id| format!("{}-{}", ven_name, id))
        .unwrap_or_else(|| format!("{}-site", ven_name));

    let report = json!({
        "programID": program_id,
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
    use crate::assets::{AssetHistoryBuffer, BaseLoadState, EvState, Grid, HistoryPoint, PvState};
    use crate::simulator::{energy::EnergyCounter, GridMeter, SimState};
    use uuid::Uuid;

    fn ts(offset_s: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(1_700_000_000 + offset_s, 0).unwrap()
    }

    /// Build an `AssetEntry` with BaseLoad history (power-only).
    ///
    /// Offsets are treated as seconds from oldest→newest. The most recent row
    /// is pinned to just before Utc::now() so `build_net_site_power_ts` (which
    /// slices [now-2h, now]) can see the data.
    fn make_entry(id: &str, rows: &[(i64, f64)]) -> crate::simulator::AssetEntry {
        let max_offset = rows.iter().map(|r| r.0).max().unwrap_or(0);
        // Truncate to second precision so concurrent make_entry calls within the same
        // second produce identical timestamps, making cross-asset LOCF aggregation deterministic.
        let now_s = DateTime::from_timestamp(Utc::now().timestamp(), 0).unwrap();
        let base = now_s - Duration::seconds(max_offset);
        let mut buf = AssetHistoryBuffer::new(3600);
        for &(offset, power_kw) in rows {
            buf.push(HistoryPoint {
                ts: base + Duration::seconds(offset),
                power_kw,
                state: AssetState::BaseLoad(BaseLoadState {
                    actual_power_kw: power_kw,
                }),
            });
        }
        crate::simulator::AssetEntry {
            id: id.to_string(),
            state: AssetState::BaseLoad(BaseLoadState {
                actual_power_kw: 0.0,
            }),
            setpoint_kw: 0.0,
            last_power_kw: rows.last().map(|r| r.1).unwrap_or(0.0),
            energy: EnergyCounter::new(),
            history: buf,
        }
    }

    /// Build an `AssetEntry` with EV history (power + SoC).
    ///
    /// Same timestamp anchoring as `make_entry`.
    fn make_ev_entry(id: &str, rows: &[(i64, f64, f64)]) -> crate::simulator::AssetEntry {
        let max_offset = rows.iter().map(|r| r.0).max().unwrap_or(0);
        let now_s = DateTime::from_timestamp(Utc::now().timestamp(), 0).unwrap();
        let base = now_s - Duration::seconds(max_offset);
        let mut buf = AssetHistoryBuffer::new(3600);
        for &(offset, power_kw, soc) in rows {
            buf.push(HistoryPoint {
                ts: base + Duration::seconds(offset),
                power_kw,
                state: AssetState::Ev(EvState {
                    soc,
                    plugged: true,
                    actual_power_kw: power_kw,
                }),
            });
        }
        crate::simulator::AssetEntry {
            id: id.to_string(),
            state: AssetState::Ev(EvState {
                soc: 0.0,
                plugged: true,
                actual_power_kw: 0.0,
            }),
            setpoint_kw: 0.0,
            last_power_kw: rows.last().map(|r| r.1).unwrap_or(0.0),
            energy: EnergyCounter::new(),
            history: buf,
        }
    }

    /// Build a minimal `SimState` from a list of entries.
    fn make_sim(entries: Vec<crate::simulator::AssetEntry>) -> SimState {
        SimState {
            asset_configs: vec![],
            assets: entries,
            grid: GridMeter::default(),
            grid_asset: Grid::new(),
            pv_smoothing: crate::simulator::PvSmoothingState::default(),
            base_load_smoothing: Default::default(),
            last_tick: Utc::now(),
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

    // ── points_to_power_ts ────────────────────────────────────────

    #[test]
    fn points_to_power_ts_basic() {
        let pts = vec![
            HistoryPoint {
                ts: ts(0),
                power_kw: 1.0,
                state: AssetState::BaseLoad(BaseLoadState {
                    actual_power_kw: 1.0,
                }),
            },
            HistoryPoint {
                ts: ts(60),
                power_kw: 2.0,
                state: AssetState::BaseLoad(BaseLoadState {
                    actual_power_kw: 2.0,
                }),
            },
            HistoryPoint {
                ts: ts(120),
                power_kw: 3.0,
                state: AssetState::BaseLoad(BaseLoadState {
                    actual_power_kw: 3.0,
                }),
            },
        ];
        let series = points_to_power_ts(&pts, Interpolation::Step);
        assert_eq!(series.samples.len(), 3);
        assert!((series.samples[0].1 - 1.0).abs() < 1e-9);
        assert!((series.samples[2].1 - 3.0).abs() < 1e-9);
    }

    #[test]
    fn soc_from_point_ev() {
        let p = HistoryPoint {
            ts: ts(0),
            power_kw: 7.0,
            state: AssetState::Ev(EvState {
                soc: 0.4,
                plugged: true,
                actual_power_kw: 7.0,
            }),
        };
        assert!((soc_from_point(&p).unwrap() - 0.4).abs() < 1e-9);
    }

    #[test]
    fn soc_from_point_non_storage_is_none() {
        let p = HistoryPoint {
            ts: ts(0),
            power_kw: -2.0,
            state: AssetState::Pv(PvState {
                actual_power_kw: -2.0,
            }),
        };
        assert!(soc_from_point(&p).is_none());
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
        let sim = make_sim(vec![make_entry(
            "site",
            &[(0, 2.0), (900, 3.0), (1800, 4.0), (2700, 5.0)],
        )]);

        let ob = make_obligation("ev1", "prog1", "USAGE", 900);
        let report = build_measurement_report_for_obligation(&ob, &sim, "ven-1", None).unwrap();

        let intervals = report["resources"][0]["intervals"].as_array().unwrap();
        assert!(
            intervals.len() >= 2,
            "expected multiple intervals, got {}",
            intervals.len()
        );
        for (i, iv) in intervals.iter().enumerate() {
            assert_eq!(iv["id"], i as u64);
            assert!(iv["intervalPeriod"]["start"].is_string());
            assert_eq!(iv["intervalPeriod"]["duration"], "PT15M");
        }
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
        let sim = make_sim(vec![]);
        assert!(build_measurement_report_for_obligation(&ob, &sim, "ven-1", None).is_none());
    }

    #[test]
    fn obligation_report_empty_history_returns_none() {
        let ob = make_obligation("e1", "p1", "USAGE", 900);
        let sim = make_sim(vec![]);
        assert!(build_measurement_report_for_obligation(&ob, &sim, "ven-1", None).is_none());
    }

    // ── import/export split ────────────────────────────────────────

    #[test]
    fn obligation_report_import_clamps_negative_to_zero() {
        let sim = make_sim(vec![make_entry("pv", &[(0, -5.0), (900, -5.0)])]);
        let ob = make_obligation("e1", "p1", "IMPORT_CAPACITY_LIMIT", 900);
        let report = build_measurement_report_for_obligation(&ob, &sim, "ven-1", None).unwrap();
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
        let sim = make_sim(vec![make_entry("pv", &[(0, -3.0), (900, -3.0)])]);
        let ob = make_obligation("e1", "p1", "EXPORT_CAPACITY_LIMIT", 900);
        let report = build_measurement_report_for_obligation(&ob, &sim, "ven-1", None).unwrap();
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
                (val - 3000.0).abs() < 1.0,
                "export should be ~3000 W, got {val}"
            );
        }
    }

    // ── SoC point-in-time ──────────────────────────────────────────

    #[test]
    fn obligation_report_soc_point_in_time() {
        let sim = make_sim(vec![make_ev_entry(
            "ev",
            &[
                (0, 7.0, 0.2),
                (900, 7.0, 0.4),
                (1800, 7.0, 0.6),
                (2700, 7.0, 0.8),
            ],
        )]);
        let ob = make_obligation("e1", "p1", "STORAGE_CHARGE_LEVEL", 900);
        let report = build_measurement_report_for_obligation(&ob, &sim, "ven-1", None).unwrap();
        let intervals = report["resources"][0]["intervals"].as_array().unwrap();
        assert!(!intervals.is_empty());
        for iv in intervals {
            let soc_payload = iv["payloads"]
                .as_array()
                .unwrap()
                .iter()
                .find(|p| p["type"] == "STORAGE_CHARGE_LEVEL")
                .unwrap();
            let soc_pct: f64 = soc_payload["values"][0].as_str().unwrap().parse().unwrap();
            assert!(
                soc_pct >= 20.0 && soc_pct <= 80.0,
                "SoC {soc_pct} out of range"
            );
        }
    }

    // ── build_net_site_power_ts ────────────────────────────────────

    #[test]
    fn net_site_power_sums_assets() {
        let sim = make_sim(vec![
            make_entry("ev", &[(0, 2.0), (60, 3.0)]),
            make_entry("pv", &[(0, -1.0), (60, 1.0)]),
        ]);
        let net = build_net_site_power_ts(&sim);
        assert_eq!(net.samples.len(), 2);
        assert!((net.samples[0].1 - 1.0).abs() < 1e-9); // 2 + (-1) = 1
        assert!((net.samples[1].1 - 4.0).abs() < 1e-9); // 3 + 1 = 4
    }

    #[test]
    fn net_site_power_empty_history() {
        let sim = make_sim(vec![]);
        let net = build_net_site_power_ts(&sim);
        assert!(net.samples.is_empty());
    }

    // ── latest_net_import_kw / latest_net_export_kw ───────────────

    #[test]
    fn latest_net_import_kw_sums_positive_assets() {
        let sim = make_sim(vec![
            make_entry("a", &[(0, 3.0)]),
            make_entry("b", &[(0, -2.0)]), // export — ignored
        ]);
        assert!((latest_net_import_kw(&sim) - 3.0).abs() < 1e-9);
    }

    #[test]
    fn latest_net_import_kw_empty_history_returns_zero() {
        let sim = make_sim(vec![]);
        assert_eq!(latest_net_import_kw(&sim), 0.0);
    }

    #[test]
    fn latest_net_import_kw_all_exporting_returns_zero() {
        let sim = make_sim(vec![make_entry("pv", &[(0, -5.0)])]);
        assert_eq!(latest_net_import_kw(&sim), 0.0);
    }

    #[test]
    fn latest_net_export_kw_sums_negative_assets_as_positive() {
        let sim = make_sim(vec![
            make_entry("pv", &[(0, -2.5)]),
            make_entry("load", &[(0, 1.0)]), // import — ignored
        ]);
        assert!((latest_net_export_kw(&sim) - 2.5).abs() < 1e-9);
    }

    // ── IMPORT/EXPORT_CAPACITY_RESERVATION ────────────────────────

    #[test]
    fn test_reporter_import_capacity_reservation_from_envelope() {
        let sim = make_sim(vec![]);
        let env = SiteFlexibilityEnvelope {
            ts: Utc::now(),
            up_kw: 5.0,
            down_kw: 3.0,
            up_duration_s: None,
            down_duration_s: None,
        };
        let ob = make_obligation("e1", "p1", "IMPORT_CAPACITY_RESERVATION", 900);
        let report = build_measurement_report_for_obligation(&ob, &sim, "ven-test", Some(&env))
            .expect("should return Some");
        let val = report["resources"][0]["intervals"][0]["payloads"][0]["values"][0]
            .as_f64()
            .unwrap();
        assert!((val - 5000.0).abs() < 1.0, "expected 5000 W, got {val}");
    }

    #[test]
    fn test_reporter_export_capacity_reservation_from_envelope() {
        let sim = make_sim(vec![]);
        let env = SiteFlexibilityEnvelope {
            ts: Utc::now(),
            up_kw: 5.0,
            down_kw: 3.0,
            up_duration_s: None,
            down_duration_s: None,
        };
        let ob = make_obligation("e1", "p1", "EXPORT_CAPACITY_RESERVATION", 900);
        let report = build_measurement_report_for_obligation(&ob, &sim, "ven-test", Some(&env))
            .expect("should return Some");
        let val = report["resources"][0]["intervals"][0]["payloads"][0]["values"][0]
            .as_f64()
            .unwrap();
        assert!((val - 3000.0).abs() < 1.0, "expected 3000 W, got {val}");
    }

    #[test]
    fn test_reporter_capacity_reservation_no_envelope_returns_zero() {
        // When site_envelope is None (VEN just started), report 0 W — do NOT return None.
        let sim = make_sim(vec![]);
        let ob = make_obligation("e1", "p1", "IMPORT_CAPACITY_RESERVATION", 900);
        let report = build_measurement_report_for_obligation(&ob, &sim, "ven-test", None)
            .expect("should return Some even with no envelope");
        let val = report["resources"][0]["intervals"][0]["payloads"][0]["values"][0]
            .as_f64()
            .unwrap();
        assert_eq!(val, 0.0, "expected 0.0 W when no envelope");
    }
}
