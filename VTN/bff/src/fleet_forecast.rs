//! The fleet's forecast (openspec `opportunistic-objective` D11): what every online VEN says it
//! will draw, summed on one grid, with the objectives behind each slot.
//!
//! The record is OpenADR: each VEN reports a `USAGE_FORECAST` on the fleet-monitoring event,
//! and the recorder keeps every report. This reads the newest one per VEN, converts each
//! interval to mean power with the unit the report declares (`lab_core::units::mean_power_kw`),
//! and joins the objective each VEN publishes on the fleet channel by `venName`. It also keeps
//! "what the fleet forecast said just before the objective changed": the `before` series, from
//! the newest report each VEN had sent when its objective last changed. It interprets nothing
//! about a VEN beyond what the VEN reported.

use std::collections::BTreeMap;

use chrono::{DateTime, Duration, Utc};
use serde::Serialize;
use serde_json::Value;

use crate::fleet::VenLiveState;

const PAYLOAD_TYPE: &str = "USAGE_FORECAST";

/// One VEN's forecast, as mean power per interval.
#[derive(Debug, Clone, PartialEq)]
pub struct VenForecast {
    pub ven: String,
    /// `(interval start, mean kW)`, ascending, then the last interval's end with its value, so
    /// the last interval is held to its end on a grid.
    pub samples: Vec<(DateTime<Utc>, f64)>,
    /// The report declared no unit, so the profile's (`KWH`) was assumed and is said so.
    pub assumed_unit: bool,
}

/// Read one recorded `USAGE_FORECAST` report. `None` when it carries no readable interval or
/// declares a unit this profile does not read it in (neither guessed nor converted).
pub fn read_forecast(ven: &str, report: &Value) -> Option<VenForecast> {
    let declared = report
        .get("payloadDescriptors")
        .and_then(|d| d.as_array())
        .and_then(|ds| {
            ds.iter()
                .find(|d| d.get("payloadType").and_then(Value::as_str) == Some(PAYLOAD_TYPE))
        })
        .and_then(|d| d.get("units").and_then(Value::as_str).map(str::to_string));
    let profile_unit =
        lab_core::wire_contract::report_quantity_of(PAYLOAD_TYPE).and_then(|q| q.units());
    let unit = declared.clone().or(profile_unit.map(str::to_string))?;
    let mut samples: Vec<(DateTime<Utc>, f64, i64)> = Vec::new();
    for resource in report
        .get("resources")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        for interval in resource
            .get("intervals")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let Some((start, duration_s)) = interval_span(interval) else {
                continue;
            };
            let Some(value) = interval_value(interval) else {
                continue;
            };
            let dt_h = lab_core::units::dt_h_from_s(duration_s as f64);
            let kw = match unit.as_str() {
                "KWH" => lab_core::units::mean_power_kw(value, dt_h),
                "KW" => Some(value),
                _ => None,
            };
            if let Some(kw) = kw {
                samples.push((start, kw, duration_s));
            }
        }
    }
    samples.sort_by_key(|(t, _, _)| *t);
    let &(last_start, last_kw, last_s) = samples.last()?;
    let mut out: Vec<(DateTime<Utc>, f64)> =
        samples.into_iter().map(|(t, kw, _)| (t, kw)).collect();
    out.push((last_start + Duration::seconds(last_s), last_kw));
    Some(VenForecast {
        ven: ven.to_string(),
        samples: out,
        assumed_unit: declared.is_none(),
    })
}

fn interval_span(interval: &Value) -> Option<(DateTime<Utc>, i64)> {
    let period = interval.get("intervalPeriod")?;
    let start = period
        .get("start")?
        .as_str()?
        .parse::<DateTime<Utc>>()
        .ok()?;
    let duration_s = crate::recorder::parse_pt_duration_s(period.get("duration")?.as_str()?);
    (duration_s > 0).then_some((start, duration_s))
}

fn interval_value(interval: &Value) -> Option<f64> {
    interval
        .get("payloads")?
        .as_array()?
        .iter()
        .find(|p| p.get("type").and_then(Value::as_str) == Some(PAYLOAD_TYPE))?
        .get("values")?
        .as_array()?
        .first()?
        .as_f64()
}

/// One slot of the fleet forecast.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ForecastSlot {
    pub start: DateTime<Utc>,
    /// The fleet's net power [kW], positive = import, over the VENs that forecast this slot.
    pub kw: f64,
    /// How many VENs it is built from.
    pub vens: usize,
    /// How many of them run each objective (`unknown` for a VEN that publishes none).
    pub objectives: BTreeMap<String, usize>,
}

/// The objective a VEN reports on its `objective` leaf, `unknown` without one.
pub fn objective_of(live: Option<&VenLiveState>) -> String {
    live.and_then(|l| l.objective.as_ref())
        .and_then(|o| o.get("objective"))
        .and_then(Value::as_str)
        .unwrap_or("unknown")
        .to_string()
}

/// Whether the fleet channel says this VEN is gone. A VEN it has never heard of is not.
fn offline(live: Option<&VenLiveState>) -> bool {
    live.and_then(|l| l.state.as_deref()) == Some("offline")
}

/// Sum the online VENs' forecasts on a `step` grid, from `from` on.
pub fn sum_forecasts(
    forecasts: &[VenForecast],
    fleet: &BTreeMap<String, VenLiveState>,
    step: Duration,
    from: DateTime<Utc>,
) -> Vec<ForecastSlot> {
    let mut online: Vec<&VenForecast> = forecasts
        .iter()
        .filter(|f| !offline(fleet.get(&f.ven)))
        .collect();
    online.sort_by(|a, b| a.ven.cmp(&b.ven));
    let rows = online
        .iter()
        .flat_map(|f| {
            f.samples
                .iter()
                .map(|&(t, kw)| (f.ven.clone(), t, Some(kw)))
        })
        .collect();
    let series = crate::fleet_store::group_and_resample(rows, step);
    let objective: BTreeMap<&str, String> = online
        .iter()
        .map(|f| (f.ven.as_str(), objective_of(fleet.get(&f.ven))))
        .collect();
    crate::fleet_store::sum_over_grid(&series)
        .into_iter()
        .filter(|(start, _, _)| *start + step > from)
        .map(|(start, kw, vens)| {
            let mut objectives = BTreeMap::new();
            for (ven, s) in &series {
                if s.samples.iter().any(|(t, _)| *t == start) {
                    *objectives
                        .entry(objective[ven.as_str()].clone())
                        .or_insert(0) += 1;
                }
            }
            ForecastSlot {
                start,
                kw,
                vens,
                objectives,
            }
        })
        .collect()
}

/// The newest `USAGE_FORECAST` report of every VEN that reports one, received before `before`
/// when given: `(venName, report)`.
pub async fn newest_reports(
    pool: &sqlx::PgPool,
    before: Option<(&str, DateTime<Utc>)>,
) -> anyhow::Result<Vec<(String, Value)>> {
    let contains =
        serde_json::json!({"resources": [{"intervals": [{"payloads": [{"type": PAYLOAD_TYPE}]}]}]});
    let rows: Vec<(String, Value)> = match before {
        None => {
            sqlx::query_as(
                "SELECT DISTINCT ON (ven_name) ven_name, payload_json
                 FROM lab_recorder.reports_received
                 WHERE ven_name IS NOT NULL AND payload_json @> $1
                 ORDER BY ven_name, received_at DESC",
            )
            .bind(&contains)
            .fetch_all(pool)
            .await?
        }
        Some((ven, at)) => {
            sqlx::query_as(
                "SELECT ven_name, payload_json
                 FROM lab_recorder.reports_received
                 WHERE ven_name = $1 AND payload_json @> $2 AND received_at < $3
                 ORDER BY received_at DESC
                 LIMIT 1",
            )
            .bind(ven)
            .bind(&contains)
            .bind(at)
            .fetch_all(pool)
            .await?
        }
    };
    Ok(rows)
}

/// The VENs whose objective changed, and when; the `before` series uses each one's newest
/// report from before that moment and every other VEN's current one.
pub fn objective_changes(fleet: &BTreeMap<String, VenLiveState>) -> Vec<(String, DateTime<Utc>)> {
    fleet
        .iter()
        .filter(|(_, l)| l.state.as_deref() != Some("offline"))
        .filter_map(|(ven, l)| Some((ven.clone(), l.objective_changed_at?)))
        .collect()
}

/// The forecasts the `before` series sums: each VEN's pre-change report where it changed, its
/// current one otherwise. `None` when no online VEN changed objective or none of the changed
/// ones had reported before it changed.
pub fn before_forecasts(
    current: &[VenForecast],
    pre_change: &[VenForecast],
) -> Option<Vec<VenForecast>> {
    if pre_change.is_empty() {
        return None;
    }
    let mut out: Vec<VenForecast> = current
        .iter()
        .filter(|f| !pre_change.iter().any(|p| p.ven == f.ven))
        .cloned()
        .collect();
    out.extend(pre_change.iter().cloned());
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn t(s: &str) -> DateTime<Utc> {
        s.parse().unwrap()
    }

    fn report(units: Option<&str>, kwh: &[f64]) -> Value {
        let intervals: Vec<Value> = kwh
            .iter()
            .enumerate()
            .map(|(i, v)| {
                json!({
                    "id": i,
                    "intervalPeriod": {
                        "start": (t("2026-10-17T12:00:00Z") + Duration::minutes(15 * i as i64)).to_rfc3339(),
                        "duration": "PT15M"
                    },
                    "payloads": [{"type": "USAGE_FORECAST", "values": [v]}]
                })
            })
            .collect();
        let mut r =
            json!({"resources": [{"resourceName": "AGGREGATED_REPORT", "intervals": intervals}]});
        if let Some(u) = units {
            r["payloadDescriptors"] = json!([{"payloadType": "USAGE_FORECAST", "units": u}]);
        }
        r
    }

    fn live(state: &str, objective: Option<&str>) -> VenLiveState {
        let mut fleet = BTreeMap::new();
        crate::fleet::apply_message(
            &mut fleet,
            "x",
            "status",
            json!({"state": state}).to_string().as_bytes(),
            t("2026-10-17T12:00:00Z"),
        );
        if let Some(o) = objective {
            let body = json!({"objective": o, "available_objectives": [o]}).to_string();
            crate::fleet::apply_message(
                &mut fleet,
                "x",
                "objective",
                body.as_bytes(),
                t("2026-10-17T12:00:00Z"),
            );
        }
        fleet.remove("x").unwrap()
    }

    #[test]
    fn a_kwh_forecast_becomes_mean_power_per_interval() {
        let f = read_forecast("ven-1", &report(Some("KWH"), &[0.5, -0.25])).unwrap();
        assert_eq!(
            f.samples,
            vec![
                (t("2026-10-17T12:00:00Z"), 2.0),
                (t("2026-10-17T12:15:00Z"), -1.0),
                (t("2026-10-17T12:30:00Z"), -1.0),
            ],
            "0.5 kWh over 15 min is 2 kW; the last interval is held to its end"
        );
        assert!(!f.assumed_unit);
    }

    /// The profile's unit applies to a report that declares none, and that is said; a unit the
    /// profile does not read is neither guessed nor converted.
    #[test]
    fn an_undeclared_unit_is_assumed_and_said_and_a_foreign_one_refused() {
        assert!(
            read_forecast("ven-1", &report(None, &[0.5]))
                .unwrap()
                .assumed_unit
        );
        assert_eq!(
            read_forecast("ven-1", &report(Some("KW"), &[2.0]))
                .unwrap()
                .samples[0]
                .1,
            2.0
        );
        assert!(read_forecast("ven-1", &report(Some("PERCENT"), &[2.0])).is_none());
    }

    fn fleet(entries: &[(&str, VenLiveState)]) -> BTreeMap<String, VenLiveState> {
        entries
            .iter()
            .map(|(v, l)| (v.to_string(), l.clone()))
            .collect()
    }

    #[test]
    fn the_fleet_sum_excludes_offline_vens_and_lists_the_objectives() {
        let forecasts = vec![
            read_forecast("ven-1", &report(Some("KWH"), &[0.5, 0.5])).unwrap(),
            read_forecast("ven-2", &report(Some("KWH"), &[0.25, 0.25])).unwrap(),
            read_forecast("ven-3", &report(Some("KWH"), &[1.0, 1.0])).unwrap(),
            read_forecast("ven-4", &report(Some("KWH"), &[0.25, 0.25])).unwrap(),
        ];
        let fleet = fleet(&[
            ("ven-1", live("online", Some("opportunistic"))),
            ("ven-2", live("online", Some("min_cost"))),
            ("ven-3", live("offline", Some("min_cost"))),
            // ven-4 publishes no objective leaf: counted as unknown, not dropped.
        ]);
        let slots = sum_forecasts(
            &forecasts,
            &fleet,
            Duration::minutes(15),
            t("2026-10-17T12:00:00Z"),
        );
        let first = &slots[0];
        assert_eq!(first.start, t("2026-10-17T12:00:00Z"));
        assert!(
            (first.kw - (2.0 + 1.0 + 1.0)).abs() < 1e-9,
            "ven-3 is offline, got {}",
            first.kw
        );
        assert_eq!(first.vens, 3);
        assert_eq!(
            first.objectives,
            BTreeMap::from([
                ("min_cost".into(), 1),
                ("opportunistic".into(), 1),
                ("unknown".into(), 1)
            ])
        );
    }

    /// The `before` series: each changed VEN's pre-change forecast, every other VEN's current one,
    /// on the same conversion and grid; nothing when no VEN changed or none had reported before.
    #[test]
    fn the_before_series_swaps_in_each_changed_ven_s_earlier_forecast() {
        let now_1 = read_forecast("ven-1", &report(Some("KWH"), &[0.5])).unwrap();
        let now_2 = read_forecast("ven-2", &report(Some("KWH"), &[0.25])).unwrap();
        let was_1 = read_forecast("ven-1", &report(Some("KWH"), &[1.0])).unwrap();
        assert_eq!(before_forecasts(&[now_1.clone(), now_2.clone()], &[]), None);
        let before = before_forecasts(&[now_1, now_2], &[was_1]).unwrap();
        let fleet = fleet(&[
            ("ven-1", live("online", None)),
            ("ven-2", live("online", None)),
        ]);
        let slots = sum_forecasts(
            &before,
            &fleet,
            Duration::minutes(15),
            t("2026-10-17T12:00:00Z"),
        );
        assert!(
            (slots[0].kw - (4.0 + 1.0)).abs() < 1e-9,
            "4 kW before ven-1 changed, got {}",
            slots[0].kw
        );
    }

    /// Only online VENs that changed objective are asked for an earlier report; a second change
    /// moves the moment.
    #[test]
    fn objective_changes_are_the_online_vens_last_change() {
        let mut fleet = BTreeMap::new();
        let body = |o: &str| json!({"objective": o}).to_string();
        crate::fleet::apply_message(
            &mut fleet,
            "ven-1",
            "objective",
            body("min_cost").as_bytes(),
            t("2026-10-17T11:00:00Z"),
        );
        assert!(
            objective_changes(&fleet).is_empty(),
            "the first objective seen is not a change"
        );
        crate::fleet::apply_message(
            &mut fleet,
            "ven-1",
            "objective",
            body("opportunistic").as_bytes(),
            t("2026-10-17T12:00:00Z"),
        );
        crate::fleet::apply_message(
            &mut fleet,
            "ven-1",
            "objective",
            body("opportunistic").as_bytes(),
            t("2026-10-17T12:05:00Z"),
        );
        assert_eq!(
            objective_changes(&fleet),
            vec![("ven-1".into(), t("2026-10-17T12:00:00Z"))]
        );
        crate::fleet::apply_message(
            &mut fleet,
            "ven-1",
            "objective",
            body("min_cost").as_bytes(),
            t("2026-10-17T13:00:00Z"),
        );
        assert_eq!(
            objective_changes(&fleet),
            vec![("ven-1".into(), t("2026-10-17T13:00:00Z"))]
        );
        crate::fleet::apply_message(
            &mut fleet,
            "ven-1",
            "status",
            br#"{"state":"offline"}"#,
            t("2026-10-17T13:01:00Z"),
        );
        assert!(
            objective_changes(&fleet).is_empty(),
            "offline VENs are excluded"
        );
    }
}
