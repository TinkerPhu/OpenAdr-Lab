/// Controller reporter: builds OpenADR VEN report payloads.
///
/// Two modes:
///   - Measurement reports (timer-driven): one TELEMETRY_USAGE report per active event.
///   - Status reports (event-driven): TELEMETRY_STATUS triggered by PlanCycle/PacketTransition.
use chrono::{DateTime, Duration, Utc};
use tracing::debug;

use crate::assets::HistoryPoint;
use crate::common::{parse_iso8601_duration_secs, Aggregation, Interpolation, TimeSeries};
use crate::controller::trace::ControllerEvent;
use crate::controller::vtn_port::{
    OadrEvent, OadrIntervalPeriod, OadrReportBody, OadrReportInterval, OadrReportPayload,
    OadrReportResource,
};
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
pub fn build_measurement_report(
    event: &OadrEvent,
    sim: &SimState,
    ven_name: &str,
) -> Option<OadrReportBody> {
    let event_id = &event.id;
    let program_id = &event.programID;

    let report_name = format!("auto-{}-{}", ven_name, event_id);
    let resource_name = format!("{}-meter", ven_name);

    let net_import_kw = latest_net_import_kw(sim);
    let net_import_w = net_import_kw * 1000.0;

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
            let export_kw = latest_net_export_kw(sim);
            ("USAGE", export_kw * 1000.0)
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
    if let Some(ev_entry) = sim.asset("ev") {
        if let Some(last) = ev_entry.history.latest() {
            if let Some(soc) = soc_from_point(last) {
                payloads.push(OadrReportPayload {
                    r#type: "STORAGE_CHARGE_LEVEL".to_string(),
                    values: vec![serde_json::Value::from(format!("{:.1}", soc * 100.0))],
                });
            }
        }
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
    sim: &SimState,
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
            .map_or(false, |arr| !arr.is_empty());
        if has_descriptors {
            debug!(
                event_id = %event.id,
                "timer-driven: skipping event with reportDescriptors"
            );
            continue;
        }
        if seen.insert(event.id.clone()) {
            debug!(event_id = %event.id, "timer-driven: building single-interval report");
            if let Some(report) = build_measurement_report(event, sim, ven_name) {
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
    sim: &SimState,
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
    let net_power_ts = build_net_site_power_ts(sim);

    let payload_type = &obligation.payload_type;

    let intervals: Vec<OadrReportInterval> = match payload_type.as_str() {
        "STORAGE_CHARGE_STATE" | "STORAGE_CHARGE_LEVEL" => {
            build_soc_intervals(sim, interval_width, &duration_iso)
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
        event_id,
        interval_count,
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
fn build_soc_intervals(
    sim: &SimState,
    interval_width: Duration,
    duration_iso: &str,
) -> Vec<OadrReportInterval> {
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

            OadrReportInterval {
                id: i,
                intervalPeriod: Some(OadrIntervalPeriod {
                    start: Some(ts.to_rfc3339()),
                    duration: Some(duration_iso.to_string()),
                }),
                payloads: vec![
                    OadrReportPayload {
                        r#type: "STORAGE_CHARGE_LEVEL".to_string(),
                        values: vec![serde_json::Value::from(format!(
                            "{:.1}",
                            soc_value * 100.0
                        ))],
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
/// Only emits for `PlanCycle` and `PacketTransition` variants.
/// Returns None when no active OpenADR event provides a `programID` (VTN requires it)
/// or for unhandled event types.
pub fn build_status_report(
    event: &ControllerEvent,
    sim: &SimState,
    ven_name: &str,
    program_id: Option<&str>,
    _now: DateTime<Utc>,
) -> Option<OadrReportBody> {
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
    use crate::assets::{AssetHistoryBuffer, AssetState, BaseLoadState, EvState, Grid, HistoryPoint, PvState};
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
        let intervals = &report.resources[0].intervals;
        for iv in intervals {
            let usage = iv
                .payloads
                .iter()
                .find(|p| p.r#type == "USAGE")
                .unwrap();
            let val = usage.values[0].as_f64().unwrap();
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
        let intervals = &report.resources[0].intervals;
        for iv in intervals {
            let usage = iv
                .payloads
                .iter()
                .find(|p| p.r#type == "USAGE")
                .unwrap();
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
        let iv = &report.resources[0].intervals[0];
        let val = iv.payloads[0].values[0].as_f64().unwrap();
        assert!((val - 5000.0).abs() < 1.0, "expected 5000 W, got {val}");
        assert_eq!(iv.payloads[0].r#type, "IMPORT_CAPACITY_RESERVATION");
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
        let iv = &report.resources[0].intervals[0];
        let val = iv.payloads[0].values[0].as_f64().unwrap();
        assert!((val - 3000.0).abs() < 1.0, "expected 3000 W, got {val}");
        assert_eq!(iv.payloads[0].r#type, "EXPORT_CAPACITY_RESERVATION");
    }

    #[test]
    fn test_reporter_capacity_reservation_no_envelope_returns_zero() {
        // When site_envelope is None (VEN just started), report 0 W — do NOT return None.
        let sim = make_sim(vec![]);
        let ob = make_obligation("e1", "p1", "IMPORT_CAPACITY_RESERVATION", 900);
        let report = build_measurement_report_for_obligation(&ob, &sim, "ven-test", None)
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
        let sim = make_sim(vec![make_entry("site", &[(0, 3.0)])]);
        let report = build_measurement_report(&event, &sim, "ven-1").unwrap();
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
        let sim = make_sim(vec![make_ev_entry("ev", &[(0, 7.0, 0.5)])]);
        let report = build_measurement_report(&event, &sim, "ven-1").unwrap();
        let iv = &report.resources[0].intervals[0];
        let soc_payload = iv.payloads.iter().find(|p| p.r#type == "STORAGE_CHARGE_LEVEL");
        assert!(soc_payload.is_some(), "expected SoC payload for EV");
        let soc_str = soc_payload.unwrap().values[0].as_str().unwrap();
        let soc_pct: f64 = soc_str.parse().unwrap();
        assert!((soc_pct - 50.0).abs() < 0.2, "expected ~50%, got {soc_pct}");
    }

    // ── build_measurement_reports_for_active_events ────────────────

    #[test]
    fn active_events_returns_empty_for_no_events() {
        let sim = make_sim(vec![make_entry("site", &[(0, 1.0)])]);
        let reports = build_measurement_reports_for_active_events(&[], &sim, "ven-1", Utc::now());
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
        let sim = make_sim(vec![make_entry("site", &[(0, 1.0)])]);
        let reports =
            build_measurement_reports_for_active_events(&[event], &sim, "ven-1", Utc::now());
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
        let sim = make_sim(vec![make_entry("site", &[(0, 2.0)])]);
        let report =
            build_status_report(&event, &sim, "ven-1", Some("prog-001"), Utc::now()).unwrap();
        assert_eq!(report.programID, "prog-001");
        assert!(report.eventID.is_none(), "status report must omit eventID");
        assert_eq!(report.clientName, "ven-1");
        assert_eq!(report.reportName.as_deref(), Some("status-ven-1"));
        assert_eq!(report.resources[0].resourceName, "ven-1-site");
        let iv = &report.resources[0].intervals[0];
        assert_eq!(iv.id, 0);
        let status_payload = iv.payloads.iter().find(|p| p.r#type == "TELEMETRY_STATUS").unwrap();
        let desc = status_payload.values[0].as_str().unwrap();
        assert!(desc.contains("PlanCycle"), "expected PlanCycle in description, got: {desc}");
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
        let sim = make_sim(vec![]);
        assert!(build_status_report(&event, &sim, "ven-1", None, Utc::now()).is_none());
    }
}
