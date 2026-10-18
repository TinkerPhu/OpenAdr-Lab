//! What the fleet plans to import and export (`fleet-plan-feed`).
//!
//! `fleet_store` holds what the sites *did*; this holds what they *intend*. A VEN publishes the
//! plan it has just adopted; the BFF keeps every one, and answers "what did the fleet plan as of
//! instant T" by taking, per VEN, the latest plan adopted at or before T and summing them.
//!
//! "Now" and "an earlier instant" are the same question with a different `at`, answered by the one
//! function below, so a before/after comparison cannot disagree with itself about how plans are
//! added up. The adding up is the fleet power chart's: each plan becomes a step series (a planned
//! kW holds for its slot), resampled onto an epoch-aligned grid by `lab_core::time_series`, and
//! summed with `fleet_store::sum_over_grid`, which counts how many VENs each total is built from
//! and treats a VEN with nothing planned for a slot as absent, never as zero.

use anyhow::Result;
use chrono::{DateTime, Duration, Utc};
use lab_core::time_series::{Aggregation, Interpolation, TimeSeries};
use serde::Serialize;
use serde_json::Value;
use sqlx::PgPool;
use std::collections::{BTreeMap, BTreeSet};

use crate::fleet_store::sum_over_grid;

/// How long a stored plan is kept. The comparison it serves looks back minutes to hours, and a
/// plan is large (a slot per few minutes over the horizon), so this is days, not the decisions'
/// months.
pub const PLAN_RETENTION_DAYS: i64 = 7;

/// The finest grid a fleet plan is summed on, whatever slot length a VEN plans at.
const MIN_STEP_SECONDS: i64 = 60;
/// The grid when no plan says otherwise.
const DEFAULT_STEP_SECONDS: i64 = 300;

/// One slot of a published plan: the fields the fleet sum reads.
#[derive(Clone, Debug, PartialEq)]
pub struct PlanSlot {
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
    pub net_import_kw: f64,
    pub net_export_kw: f64,
}

fn parse_time(v: &Value) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(v.as_str()?)
        .ok()
        .map(|t| t.with_timezone(&Utc))
}

/// The slots of a plan message, or `None` if any slot cannot be read.
///
/// All or nothing on purpose: a plan with a slot silently missing would be summed as a plan with
/// less planned energy, which is a wrong answer that looks like a right one.
pub fn parse_slots(payload: &Value) -> Option<Vec<PlanSlot>> {
    payload
        .get("slots")?
        .as_array()?
        .iter()
        .map(|s| {
            Some(PlanSlot {
                start: parse_time(s.get("start")?)?,
                end: parse_time(s.get("end")?)?,
                net_import_kw: s.get("net_import_kw")?.as_f64()?,
                net_export_kw: s.get("net_export_kw")?.as_f64()?,
            })
        })
        .collect()
}

/// One plan as it is stored.
#[derive(Clone, Debug)]
pub struct PlanRow {
    pub ven_name: String,
    pub plan_id: String,
    /// When the VEN adopted it, by the VEN's clock. This decides which plan was current at an instant.
    pub created_at: DateTime<Utc>,
    /// When the BFF learned of it. Stored beside `created_at`, as for decisions, so a skewed VEN
    /// clock is visible as the gap between them.
    pub received_at: DateTime<Utc>,
    pub payload: Value,
}

impl PlanRow {
    /// A row from a published plan, or `None` if it is not one the fleet sum can use.
    pub fn from_message(
        ven_name: &str,
        payload: &Value,
        received_at: DateTime<Utc>,
    ) -> Option<Self> {
        let plan_id = payload.get("id")?.as_str()?.to_string();
        let created_at = parse_time(payload.get("created_at")?)?;
        parse_slots(payload)?;
        Some(Self {
            ven_name: ven_name.to_string(),
            plan_id,
            created_at,
            received_at,
            payload: payload.clone(),
        })
    }
}

/// A stored plan read back, with its slots parsed and the message as the VEN published it (the
/// Fleet page's per-VEN plan shows that, `GET /api/fleet/vens/:ven/plan`).
#[derive(Clone, Debug)]
pub struct StoredPlan {
    pub ven_name: String,
    pub plan_id: String,
    pub created_at: DateTime<Utc>,
    pub slots: Vec<PlanSlot>,
    pub payload: Value,
}

pub async fn insert_plan_batch(pool: &PgPool, rows: &[PlanRow]) -> Result<()> {
    if rows.is_empty() {
        return Ok(());
    }
    let names: Vec<&str> = rows.iter().map(|r| r.ven_name.as_str()).collect();
    let ids: Vec<&str> = rows.iter().map(|r| r.plan_id.as_str()).collect();
    let created: Vec<DateTime<Utc>> = rows.iter().map(|r| r.created_at).collect();
    let received: Vec<DateTime<Utc>> = rows.iter().map(|r| r.received_at).collect();
    let payloads: Vec<Value> = rows.iter().map(|r| r.payload.clone()).collect();

    sqlx::query(
        "INSERT INTO lab_recorder.fleet_plan
            (ven_name, plan_id, created_at, received_at, payload_json)
         SELECT * FROM UNNEST($1::text[], $2::text[], $3::timestamptz[], $4::timestamptz[],
                              $5::jsonb[])
         ON CONFLICT (ven_name, plan_id) DO NOTHING",
    )
    .bind(&names)
    .bind(&ids)
    .bind(&created)
    .bind(&received)
    .bind(&payloads)
    .execute(pool)
    .await?;
    Ok(())
}

/// For each VEN, the latest plan adopted at or before `at`.
///
/// A re-published retained message arrives again with the same id and is ignored by the insert,
/// so "latest" is by adoption time, not by arrival.
pub async fn latest_plans_at(pool: &PgPool, at: DateTime<Utc>) -> Result<Vec<StoredPlan>> {
    let rows: Vec<(String, String, DateTime<Utc>, Value)> = sqlx::query_as(
        "SELECT DISTINCT ON (ven_name) ven_name, plan_id, created_at, payload_json
         FROM lab_recorder.fleet_plan
         WHERE created_at <= $1
         ORDER BY ven_name, created_at DESC",
    )
    .bind(at)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .filter_map(|(ven_name, plan_id, created_at, payload)| {
            // A stored payload that no longer parses is skipped, like an unreadable message.
            Some(StoredPlan {
                ven_name,
                plan_id,
                created_at,
                slots: parse_slots(&payload)?,
                payload,
            })
        })
        .collect())
}

/// Drop plans past `PLAN_RETENTION_DAYS`.
pub async fn run_retention(pool: &PgPool) -> Result<u64> {
    Ok(
        sqlx::query("DELETE FROM lab_recorder.fleet_plan WHERE created_at < $1")
            .bind(Utc::now() - Duration::days(PLAN_RETENTION_DAYS))
            .execute(pool)
            .await?
            .rows_affected(),
    )
}

/// One grid point of the fleet plan.
#[derive(Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct FleetPlanPoint {
    pub ts: DateTime<Utc>,
    pub net_import_kw: f64,
    pub net_export_kw: f64,
    /// How many VENs are planned at this point. A total over eleven VENs and over twenty look the
    /// same once they are a number, so the count travels with it.
    pub contributing_vens: usize,
}

/// Which plan a VEN contributed, so a caller can tell a replan from a stale answer.
#[derive(Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ContributingPlan {
    pub ven_name: String,
    pub plan_id: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct FleetPlan {
    pub at: DateTime<Utc>,
    pub step_seconds: i64,
    pub points: Vec<FleetPlanPoint>,
    pub plans: Vec<ContributingPlan>,
    /// Known VENs with no plan at `at`: named, never counted as planning zero.
    pub vens_without_plan: Vec<String>,
}

/// The finest slot any of the plans uses, so no VEN's plan is smeared by the common grid.
fn grid_step_seconds(plans: &[StoredPlan]) -> i64 {
    plans
        .iter()
        .flat_map(|p| p.slots.iter())
        .map(|s| (s.end - s.start).num_seconds())
        .filter(|&d| d > 0)
        .min()
        .map_or(DEFAULT_STEP_SECONDS, |d| d.max(MIN_STEP_SECONDS))
}

/// One plan's import and export as step series on the common grid. Each ends at the plan's own
/// horizon: beyond it the VEN has nothing planned and contributes nothing.
fn plan_series(plan: &StoredPlan, step: Duration) -> Option<(TimeSeries, TimeSeries)> {
    let mut slots: Vec<&PlanSlot> = plan.slots.iter().filter(|s| s.end > s.start).collect();
    slots.sort_by_key(|s| s.start);
    slots.dedup_by_key(|s| s.start);
    let last = slots.last()?;
    let horizon_end = last.end;

    let series = |value: fn(&PlanSlot) -> f64| {
        let mut samples: Vec<(DateTime<Utc>, f64)> =
            slots.iter().map(|s| (s.start, value(s))).collect();
        // A closing sample at the horizon, so the final slot is held across its whole length.
        samples.push((horizon_end, value(last)));
        let mut resampled = TimeSeries {
            samples,
            interpolation: Interpolation::Step,
        }
        .resample_uniform(step, Aggregation::Mean);
        resampled.samples.retain(|(ts, _)| *ts < horizon_end);
        resampled
    };
    Some((series(|s| s.net_import_kw), series(|s| s.net_export_kw)))
}

/// The fleet's planned import and export as of `at`, from the plans current then.
///
/// `plans` is one plan per VEN (the latest at or before `at`); `known_vens` is every VEN the fleet
/// feed has heard of, so a VEN without a plan can be named.
pub fn fleet_plan(plans: &[StoredPlan], known_vens: &[String], at: DateTime<Utc>) -> FleetPlan {
    let step_seconds = grid_step_seconds(plans);
    let step = Duration::seconds(step_seconds);

    let mut imports: Vec<(String, TimeSeries)> = Vec::new();
    let mut exports: Vec<(String, TimeSeries)> = Vec::new();
    let mut contributing: Vec<ContributingPlan> = Vec::new();
    for plan in plans {
        let Some((import, export)) = plan_series(plan, step) else {
            continue;
        };
        imports.push((plan.ven_name.clone(), import));
        exports.push((plan.ven_name.clone(), export));
        contributing.push(ContributingPlan {
            ven_name: plan.ven_name.clone(),
            plan_id: plan.plan_id.clone(),
            created_at: plan.created_at,
        });
    }

    // Both directions are sampled at the same timestamps per VEN, so their totals align.
    let export_by_ts: BTreeMap<DateTime<Utc>, f64> = sum_over_grid(&exports)
        .into_iter()
        .map(|(ts, kw, _)| (ts, kw))
        .collect();
    let points = sum_over_grid(&imports)
        .into_iter()
        .map(|(ts, net_import_kw, contributing_vens)| FleetPlanPoint {
            ts,
            net_import_kw,
            net_export_kw: export_by_ts.get(&ts).copied().unwrap_or(0.0),
            contributing_vens,
        })
        .collect();

    let have: BTreeSet<&str> = contributing.iter().map(|c| c.ven_name.as_str()).collect();
    let mut vens_without_plan: Vec<String> = known_vens
        .iter()
        .filter(|v| !have.contains(v.as_str()))
        .cloned()
        .collect();
    vens_without_plan.sort();
    contributing.sort_by(|a, b| a.ven_name.cmp(&b.ven_name));

    FleetPlan {
        at,
        step_seconds,
        points,
        plans: contributing,
        vens_without_plan,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn t(s: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc)
    }

    fn slot(from: &str, to: &str, import_kw: f64, export_kw: f64) -> PlanSlot {
        PlanSlot {
            start: t(from),
            end: t(to),
            net_import_kw: import_kw,
            net_export_kw: export_kw,
        }
    }

    fn plan(ven: &str, id: &str, created: &str, slots: Vec<PlanSlot>) -> StoredPlan {
        StoredPlan {
            ven_name: ven.into(),
            plan_id: id.into(),
            created_at: t(created),
            slots,
            payload: Value::Null,
        }
    }

    fn message(id: &str, created: &str) -> Value {
        json!({
            "id": id, "created_at": created, "venName": "ven-1",
            "slots": [{
                "start": "2026-10-18T10:00:00Z", "end": "2026-10-18T10:05:00Z",
                "net_import_kw": 2.0, "net_export_kw": 0.0,
                "import_tariff_eur_kwh": 0.2, "export_tariff_eur_kwh": 0.05
            }]
        })
    }

    #[test]
    fn a_published_plan_becomes_a_row_keyed_by_its_id_and_adoption_time() {
        let row = PlanRow::from_message(
            "ven-1",
            &message("p1", "2026-10-18T10:00:00Z"),
            t("2026-10-18T10:00:02Z"),
        )
        .expect("readable plan");
        assert_eq!(row.plan_id, "p1");
        assert_eq!(row.created_at, t("2026-10-18T10:00:00Z"));
        assert_eq!(row.received_at, t("2026-10-18T10:00:02Z"));
    }

    #[test]
    fn a_message_the_fleet_sum_cannot_use_is_refused_whole() {
        let now = t("2026-10-18T10:00:00Z");
        for bad in [
            json!({"created_at": "2026-10-18T10:00:00Z", "slots": []}),
            json!({"id": "p", "slots": []}),
            json!({"id": "p", "created_at": "soon", "slots": []}),
            json!({"id": "p", "created_at": "2026-10-18T10:00:00Z"}),
            // one unreadable slot spoils the plan rather than being dropped from the sum
            json!({"id": "p", "created_at": "2026-10-18T10:00:00Z",
                   "slots": [{"start": "2026-10-18T10:00:00Z", "end": "2026-10-18T10:05:00Z",
                              "net_import_kw": "2", "net_export_kw": 0.0}]}),
        ] {
            assert!(PlanRow::from_message("ven-1", &bad, now).is_none(), "{bad}");
        }
    }

    #[test]
    fn two_vens_sum_slot_by_slot_with_the_count_beside_the_total() {
        let slots = |import_kw: f64| {
            vec![
                slot(
                    "2026-10-18T10:00:00Z",
                    "2026-10-18T10:05:00Z",
                    import_kw,
                    0.0,
                ),
                slot(
                    "2026-10-18T10:05:00Z",
                    "2026-10-18T10:10:00Z",
                    0.0,
                    import_kw / 2.0,
                ),
            ]
        };
        let plans = vec![
            plan("ven-1", "a", "2026-10-18T09:50:00Z", slots(2.0)),
            plan("ven-2", "b", "2026-10-18T09:55:00Z", slots(3.0)),
        ];
        let known = vec!["ven-1".to_string(), "ven-2".to_string()];
        let out = fleet_plan(&plans, &known, t("2026-10-18T10:00:00Z"));

        assert_eq!(out.step_seconds, 300);
        assert_eq!(
            out.points,
            vec![
                FleetPlanPoint {
                    ts: t("2026-10-18T10:00:00Z"),
                    net_import_kw: 5.0,
                    net_export_kw: 0.0,
                    contributing_vens: 2
                },
                FleetPlanPoint {
                    ts: t("2026-10-18T10:05:00Z"),
                    net_import_kw: 0.0,
                    net_export_kw: 2.5,
                    contributing_vens: 2
                },
            ]
        );
        assert!(out.vens_without_plan.is_empty());
    }

    #[test]
    fn a_ven_without_a_plan_is_named_and_not_counted_as_zero() {
        let plans = vec![plan(
            "ven-1",
            "a",
            "2026-10-18T09:50:00Z",
            vec![slot(
                "2026-10-18T10:00:00Z",
                "2026-10-18T10:05:00Z",
                2.0,
                0.0,
            )],
        )];
        let known = vec!["ven-1".to_string(), "ven-2".to_string()];
        let out = fleet_plan(&plans, &known, t("2026-10-18T10:00:00Z"));
        assert_eq!(out.vens_without_plan, vec!["ven-2".to_string()]);
        assert_eq!(out.points[0].contributing_vens, 1);
    }

    #[test]
    fn plans_with_different_slot_lengths_share_the_finest_grid() {
        let plans = vec![
            // 5-minute slots
            plan(
                "ven-1",
                "a",
                "2026-10-18T09:50:00Z",
                vec![
                    slot("2026-10-18T10:00:00Z", "2026-10-18T10:05:00Z", 1.0, 0.0),
                    slot("2026-10-18T10:05:00Z", "2026-10-18T10:10:00Z", 1.0, 0.0),
                    slot("2026-10-18T10:10:00Z", "2026-10-18T10:15:00Z", 1.0, 0.0),
                ],
            ),
            // one 15-minute slot
            plan(
                "ven-2",
                "b",
                "2026-10-18T09:50:00Z",
                vec![slot(
                    "2026-10-18T10:00:00Z",
                    "2026-10-18T10:15:00Z",
                    4.0,
                    0.0,
                )],
            ),
        ];
        let known = vec!["ven-1".to_string(), "ven-2".to_string()];
        let out = fleet_plan(&plans, &known, t("2026-10-18T10:00:00Z"));
        assert_eq!(out.step_seconds, 300);
        // The coarse slot is held across all three fine ones, not counted once.
        assert_eq!(
            out.points
                .iter()
                .map(|p| p.net_import_kw)
                .collect::<Vec<_>>(),
            vec![5.0, 5.0, 5.0]
        );
        assert!(out.points.iter().all(|p| p.contributing_vens == 2));
    }

    #[test]
    fn a_ven_contributes_nothing_past_its_own_horizon() {
        let plans = vec![
            plan(
                "ven-1",
                "a",
                "2026-10-18T09:50:00Z",
                vec![
                    slot("2026-10-18T10:00:00Z", "2026-10-18T10:05:00Z", 1.0, 0.0),
                    slot("2026-10-18T10:05:00Z", "2026-10-18T10:10:00Z", 1.0, 0.0),
                ],
            ),
            plan(
                "ven-2",
                "b",
                "2026-10-18T09:50:00Z",
                vec![slot(
                    "2026-10-18T10:00:00Z",
                    "2026-10-18T10:05:00Z",
                    2.0,
                    0.0,
                )],
            ),
        ];
        let known = vec!["ven-1".to_string(), "ven-2".to_string()];
        let out = fleet_plan(&plans, &known, t("2026-10-18T10:00:00Z"));
        assert_eq!(out.points.len(), 2);
        assert_eq!(out.points[0].contributing_vens, 2);
        assert_eq!(out.points[1].contributing_vens, 1);
        assert_eq!(out.points[1].net_import_kw, 1.0);
    }

    #[test]
    fn the_contributing_plans_are_listed_by_ven_with_their_adoption_time() {
        let plans = vec![
            plan(
                "ven-2",
                "b",
                "2026-10-18T09:55:00Z",
                vec![slot(
                    "2026-10-18T10:00:00Z",
                    "2026-10-18T10:05:00Z",
                    1.0,
                    0.0,
                )],
            ),
            plan(
                "ven-1",
                "a",
                "2026-10-18T09:50:00Z",
                vec![slot(
                    "2026-10-18T10:00:00Z",
                    "2026-10-18T10:05:00Z",
                    1.0,
                    0.0,
                )],
            ),
        ];
        let out = fleet_plan(&plans, &[], t("2026-10-18T10:00:00Z"));
        assert_eq!(
            out.plans
                .iter()
                .map(|p| (p.ven_name.as_str(), p.plan_id.as_str()))
                .collect::<Vec<_>>(),
            vec![("ven-1", "a"), ("ven-2", "b")]
        );
        assert_eq!(out.plans[0].created_at, t("2026-10-18T09:50:00Z"));
    }

    #[test]
    fn no_plans_is_an_empty_answer_that_names_everyone() {
        let known = vec!["ven-1".to_string()];
        let out = fleet_plan(&[], &known, t("2026-10-18T10:00:00Z"));
        assert!(out.points.is_empty() && out.plans.is_empty());
        assert_eq!(out.vens_without_plan, known);
        assert_eq!(out.step_seconds, 300);
    }

    /// "As of an instant" is the caller picking different plans; the same plans give the same answer.
    #[test]
    fn the_same_plans_give_the_same_answer_whatever_the_label_of_the_instant() {
        let plans = vec![plan(
            "ven-1",
            "a",
            "2026-10-18T09:50:00Z",
            vec![slot(
                "2026-10-18T10:00:00Z",
                "2026-10-18T10:05:00Z",
                2.0,
                0.0,
            )],
        )];
        let a = fleet_plan(&plans, &[], t("2026-10-18T10:00:00Z"));
        let b = fleet_plan(&plans, &[], t("2026-10-18T10:30:00Z"));
        assert_eq!(a.points, b.points);
    }
}
