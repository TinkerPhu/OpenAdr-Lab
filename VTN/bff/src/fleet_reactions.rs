//! What the fleet did about one event (phase 0 §6.3).
//!
//! The question a fleet operator asks after publishing a capacity limit is not
//! "is the fleet drawing 40 kW" but "did the twenty sites I targeted actually
//! see it, and did anything change". That is a chain, and its links live in
//! two tables: the decisions each VEN published (`fleet_trace`) and the power
//! it published around them (`fleet_telemetry`).
//!
//! What this module deliberately does *not* do is decide whether a VEN
//! "reacted". It reports when the VEN said it saw the event, when it next
//! replanned, and what its power was just before and just after — and leaves
//! the judgement to the reader. A threshold invented here would be a second
//! opinion about a site's behaviour, which belongs to the site
//! (`asset-competence-assurance`), and it would be wrong in a different way
//! for every asset mix in the fleet.

use anyhow::Result;
use chrono::{DateTime, Duration, Utc};
use serde::Serialize;
use sqlx::PgPool;

/// How far either side of "seen" the before/after means are taken over.
///
/// A minute: long enough to average out one tick's noise, short enough that a
/// slower change belongs to something else.
const WINDOW: Duration = Duration::minutes(1);

/// How long after seeing an event a plan cycle still counts as following from
/// it. Beyond this, the VEN replanned for its own reasons.
const REPLAN_WINDOW: Duration = Duration::minutes(15);

/// One VEN's part of the chain.
#[derive(Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct VenReaction {
    pub ven_name: String,
    /// When the VEN said it saw this event, by its own clock.
    pub seen_at: DateTime<Utc>,
    /// When the BFF received that message. The pair is what §6.3 measures
    /// clock offset from, so neither is dropped in favour of the other.
    pub seen_received_at: DateTime<Utc>,
    /// The event *version* the VEN saw, if it said.
    pub modification_date_time: Option<String>,
    /// The first plan cycle after it saw the event, within `REPLAN_WINDOW`.
    pub replanned_at: Option<DateTime<Utc>>,
    /// Mean site power over the minute before and the minute after. `None`
    /// where the VEN published nothing in that window — which is a real
    /// answer, and not the same as no change.
    pub power_before_w: Option<f64>,
    pub power_after_w: Option<f64>,
}

impl VenReaction {
    /// The change across the event, when both sides are known.
    ///
    /// A named accessor rather than a stored field so it cannot drift from the
    /// two numbers it is derived from.
    pub fn delta_w(&self) -> Option<f64> {
        Some(self.power_after_w? - self.power_before_w?)
    }
}

/// Assemble the chain for one event, one row per VEN that saw it.
///
/// A VEN that never saw the event is absent rather than listed with nulls: the
/// list answers "who saw this", and padding it with the VENs that did not
/// would make the count meaningless.
pub async fn reactions(pool: &PgPool, event_id: &str) -> Result<Vec<VenReaction>> {
    let seen: Vec<(String, DateTime<Utc>, DateTime<Utc>, serde_json::Value)> = sqlx::query_as(
        "SELECT DISTINCT ON (ven_name) ven_name, ts, received_at, payload_json
         FROM lab_recorder.fleet_trace
         WHERE event_id = $1 AND kind = 'OpenAdrArrived'
         ORDER BY ven_name, ts ASC",
    )
    .bind(event_id)
    .fetch_all(pool)
    .await?;

    let mut out = Vec::with_capacity(seen.len());
    for (ven_name, ts, received_at, payload) in seen {
        let replanned_at: Option<DateTime<Utc>> = sqlx::query_scalar(
            "SELECT ts FROM lab_recorder.fleet_trace
             WHERE ven_name = $1 AND kind = 'PlanCycle' AND ts >= $2 AND ts <= $3
             ORDER BY ts ASC LIMIT 1",
        )
        .bind(&ven_name)
        .bind(ts)
        .bind(ts + REPLAN_WINDOW)
        .fetch_optional(pool)
        .await?;

        let power_before_w = mean_power(pool, &ven_name, ts - WINDOW, ts).await?;
        let power_after_w = mean_power(pool, &ven_name, ts, ts + WINDOW).await?;

        out.push(VenReaction {
            ven_name,
            seen_at: ts,
            seen_received_at: received_at,
            modification_date_time: payload
                .get("modification_date_time")
                .and_then(|v| v.as_str())
                .map(str::to_string),
            replanned_at,
            power_before_w,
            power_after_w,
        });
    }
    Ok(out)
}

/// Mean published site power over `[from, to)`, or `None` if it published
/// nothing — never a zero, which would be a claim we cannot make.
async fn mean_power(
    pool: &PgPool,
    ven_name: &str,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
) -> Result<Option<f64>> {
    let mean: Option<f64> = sqlx::query_scalar(
        "SELECT avg(net_power_w) FROM lab_recorder.fleet_telemetry
         WHERE ven_name = $1 AND ts >= $2 AND ts < $3",
    )
    .bind(ven_name)
    .bind(from)
    .bind(to)
    .fetch_one(pool)
    .await?;
    Ok(mean)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(s: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc)
    }

    fn reaction(before: Option<f64>, after: Option<f64>) -> VenReaction {
        VenReaction {
            ven_name: "ven-1".into(),
            seen_at: t("2026-09-22T10:00:00Z"),
            seen_received_at: t("2026-09-22T10:00:01Z"),
            modification_date_time: None,
            replanned_at: None,
            power_before_w: before,
            power_after_w: after,
        }
    }

    #[test]
    fn delta_is_the_change_across_the_event_keeping_its_sign() {
        assert_eq!(
            reaction(Some(4000.0), Some(1500.0)).delta_w(),
            Some(-2500.0)
        );
        assert_eq!(
            reaction(Some(-500.0), Some(-2000.0)).delta_w(),
            Some(-1500.0)
        );
    }

    /// A VEN that published nothing on one side of the event has no change to
    /// report. Treating the missing side as zero would turn "we do not know"
    /// into "it stopped drawing power", which is the opposite of a silence.
    #[test]
    fn delta_is_absent_when_either_side_is_unknown() {
        assert_eq!(reaction(None, Some(1500.0)).delta_w(), None);
        assert_eq!(reaction(Some(4000.0), None).delta_w(), None);
    }
}
