//! Phase 1 (A-2) — VTN recorder: a background poll task that archives
//! reports/events/VEN health into the *existing* Postgres instance (the one
//! openleadr-rs's own VTN already uses) under a separate `lab_recorder`
//! schema — never touching openleadr-rs's own tables.
//!
//! Pagination: until Phase 2's VEN-side pagination lands, this recorder does
//! its own `skip`/`limit` loop against the VTN's list endpoints (which
//! already support it, capped at 50/page). Dedup on `(id, modificationDateTime)`
//! so re-polls don't duplicate rows — enforced via a composite primary key +
//! `ON CONFLICT DO NOTHING`.
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::Value;
use sqlx::PgPool;
use tokio::sync::RwLock;
use tracing::{error, info, warn};

use crate::vtn_client::VtnClient;

const PAGE_LIMIT: i64 = 50;
const INITIAL_BACKOFF_S: u64 = 5;
const MAX_BACKOFF_S: u64 = 300;

/// Observable recorder health (2026-08-10 incident fix) — before this, a
/// single failed startup DB connection permanently disabled the recorder for
/// the process's whole lifetime with only a one-line log as evidence; the
/// recorder silently sat dead for 9 days with zero other visible signal.
/// Surfaced via `/api/health` so it satisfies `ui-transparency` instead of
/// being invisible outside the container logs.
#[derive(Debug, Clone, Default, Serialize)]
pub struct RecorderStatus {
    pub connected: bool,
    pub last_poll_at: Option<DateTime<Utc>>,
    pub last_success_at: Option<DateTime<Utc>>,
    pub consecutive_failures: u32,
    pub last_error: Option<String>,
}

pub type SharedRecorderStatus = Arc<RwLock<RecorderStatus>>;

fn next_backoff(current: Duration) -> Duration {
    (current * 2).min(Duration::from_secs(MAX_BACKOFF_S))
}

fn mark_connect_failure(status: &mut RecorderStatus, err: &str) {
    status.connected = false;
    status.consecutive_failures += 1;
    status.last_error = Some(err.to_string());
}

fn mark_connect_success(status: &mut RecorderStatus) {
    status.connected = true;
    status.consecutive_failures = 0;
    status.last_error = None;
}

fn mark_poll_tick(status: &mut RecorderStatus, now: DateTime<Utc>, had_success: bool) {
    status.last_poll_at = Some(now);
    if had_success {
        status.last_success_at = Some(now);
    }
}

pub async fn init_schema(pool: &PgPool) -> Result<()> {
    sqlx::query("CREATE SCHEMA IF NOT EXISTS lab_recorder")
        .execute(pool)
        .await?;
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS lab_recorder.reports_received (
            report_id TEXT NOT NULL,
            modification_date_time TEXT NOT NULL,
            received_at TIMESTAMPTZ NOT NULL DEFAULT now(),
            ven_name TEXT,
            report_type TEXT,
            payload_json JSONB NOT NULL,
            PRIMARY KEY (report_id, modification_date_time)
        )",
    )
    .execute(pool)
    .await?;
    // WP3.7 (Phase 3): SG-3 timeliness column. ADD COLUMN IF NOT EXISTS so
    // both fresh databases and ones created by the Phase-1 schema get it.
    sqlx::query(
        "ALTER TABLE lab_recorder.reports_received
         ADD COLUMN IF NOT EXISTS report_lag_s DOUBLE PRECISION",
    )
    .execute(pool)
    .await?;
    // GB-36: tracks the max interval end this report_id's snapshot covered,
    // so the next poll can tell which intervals are newly appended rather
    // than recomputing lag over the whole (ever-growing) intervals array.
    sqlx::query(
        "ALTER TABLE lab_recorder.reports_received
         ADD COLUMN IF NOT EXISTS max_interval_end TIMESTAMPTZ",
    )
    .execute(pool)
    .await?;
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS lab_recorder.events_published (
            event_id TEXT NOT NULL,
            modification_date_time TEXT NOT NULL,
            seen_at TIMESTAMPTZ NOT NULL DEFAULT now(),
            event_type TEXT,
            program_id TEXT,
            payload_json JSONB NOT NULL,
            PRIMARY KEY (event_id, modification_date_time)
        )",
    )
    .execute(pool)
    .await?;
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS lab_recorder.ven_snapshots (
            ven_name TEXT PRIMARY KEY,
            ts TIMESTAMPTZ NOT NULL,
            last_seen TIMESTAMPTZ NOT NULL,
            report_lag_s DOUBLE PRECISION
        )",
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// Extract the `(id, modificationDateTime)` dedup key from a raw OpenADR
/// object. Both are standard OpenADR 3 object fields. Returns `None` if
/// either is missing/not a string — defensive, so one malformed object never
/// crashes the recorder loop.
fn dedup_key(value: &Value) -> Option<(String, String)> {
    let id = value.get("id")?.as_str()?.to_string();
    let modified = value.get("modificationDateTime")?.as_str()?.to_string();
    Some((id, modified))
}

/// Walk one duration segment's chars, accumulating digits and multiplying by
/// the unit each digit run's trailing letter maps to via `seconds_for`.
/// Letters `seconds_for` maps to `None` (e.g. `Y`/`M` in the date segment,
/// approximated as 0 — see `parse_pt_duration_s`) contribute nothing.
fn sum_digit_units(s: &str, seconds_for: impl Fn(char) -> Option<i64>) -> i64 {
    let mut total = 0i64;
    let mut num = String::new();
    for c in s.chars() {
        if c.is_ascii_digit() {
            num.push(c);
        } else {
            let v: i64 = num.parse().unwrap_or(0);
            num.clear();
            total += v * seconds_for(c).unwrap_or(0);
        }
    }
    total
}

/// Parse an ISO-8601 duration in either the VEN reporter's compact form
/// (`format_iso8601_duration`, e.g. `"PT5M"`) or the fully-qualified
/// `P[n]Y[n]M[n]DT[n]H[n]M[n]S` form the VTN normalizes durations to once a
/// report round-trips through it (e.g. `"P0Y0M0DT0H5M0S"`) — both shapes are
/// seen in practice, since `record_reports` fetches reports back from the
/// VTN rather than reading the VEN's raw POST body. Years/months are
/// approximated as 0 (this project's report intervals are minute/hour-scale
/// and never populate them, matching the same approximation in
/// `experiments/kpi.py`'s duration parser). Unknown shapes parse as 0
/// seconds.
fn parse_pt_duration_s(s: &str) -> i64 {
    let Some(rest) = s.strip_prefix('P') else {
        return 0;
    };
    let (date_part, time_part) = match rest.split_once('T') {
        Some((date, time)) => (date, time),
        None => (rest, ""),
    };
    let date_s = sum_digit_units(date_part, |c| match c {
        'D' => Some(86400),
        _ => None,
    });
    let time_s = sum_digit_units(time_part, |c| match c {
        'H' => Some(3600),
        'M' => Some(60),
        'S' => Some(1),
        _ => None,
    });
    date_s + time_s
}

/// Every interval end (`start + duration`) present in a report's `resources`,
/// across all resources/intervals. Unparseable intervals are skipped, not
/// fatal — a malformed one interval must not blank out lag for the rest.
fn interval_ends(report: &Value) -> Vec<DateTime<Utc>> {
    report
        .get("resources")
        .and_then(|v| v.as_array())
        .into_iter()
        .flatten()
        .flat_map(|r| {
            r.get("intervals")
                .and_then(|i| i.as_array())
                .into_iter()
                .flatten()
        })
        .filter_map(|iv| {
            let period = iv.get("intervalPeriod")?;
            let start = period
                .get("start")?
                .as_str()?
                .parse::<DateTime<Utc>>()
                .ok()?;
            let dur_s = period
                .get("duration")
                .and_then(|d| d.as_str())
                .map(parse_pt_duration_s)
                .unwrap_or(0);
            Some(start + chrono::Duration::seconds(dur_s))
        })
        .collect()
}

/// WP3.7 (Phase 3), fixed for GB-36 — SG-3 timeliness: seconds between the
/// report's `modificationDateTime` and the newest *newly appended* interval's
/// end. `modificationDateTime` is used as the as-of timestamp (not
/// `createdDateTime`) because openleadr-rs grows a long-lived report resource
/// by re-PUTting an ever-larger `intervals` array, bumping
/// `modificationDateTime` on every append while `createdDateTime` stays fixed
/// at the resource's original creation — using `createdDateTime` made lag
/// drift unboundedly negative as a report resource accumulated intervals over
/// its lifetime (observed down to -86320s in the 24h `s9_diurnal` run).
///
/// `prior_max_end` is the `max_interval_end` recorded for this report_id's
/// last-polled snapshot. When present, only intervals ending after it count
/// as "new" for this poll, so lag reflects just what was appended since last
/// time rather than the whole resource's history. Returns
/// `(lag_s, new_max_end)`: `new_max_end` is what to persist as this
/// snapshot's own marker for the next poll — the whole array's max on a
/// report_id's first-ever poll, or carried forward unchanged if nothing new
/// was appended. `lag_s` is `None` when there are no intervals at all, or
/// nothing new since the prior snapshot.
fn report_submission_lag_s(
    report: &Value,
    modified: DateTime<Utc>,
    prior_max_end: Option<DateTime<Utc>>,
) -> (Option<f64>, Option<DateTime<Utc>>) {
    let ends = interval_ends(report);
    if ends.is_empty() {
        return (None, prior_max_end);
    }
    let whole_max = ends.iter().copied().max();

    let new_since_prior = match prior_max_end {
        Some(prior) => ends.into_iter().filter(|e| *e > prior).max(),
        None => whole_max,
    };

    match new_since_prior {
        Some(end) => (
            Some((modified - end).num_milliseconds() as f64 / 1000.0),
            Some(end),
        ),
        None => (None, prior_max_end.or(whole_max)),
    }
}

/// Fetch every page of a list endpoint via `skip`/`limit`, stopping when a
/// page returns fewer than `PAGE_LIMIT` rows.
async fn fetch_all_pages(client: &VtnClient, path: &str) -> Result<Vec<Value>> {
    let mut all = Vec::new();
    let mut skip = 0i64;
    loop {
        let sep = if path.contains('?') { '&' } else { '?' };
        let page_path = format!("{path}{sep}skip={skip}&limit={PAGE_LIMIT}");
        let page: Vec<Value> = serde_json::from_value(client.get_json(&page_path, None).await?)
            .context("paginated response was not a JSON array")?;
        let n = page.len();
        all.extend(page);
        if (n as i64) < PAGE_LIMIT {
            break;
        }
        skip += PAGE_LIMIT;
    }
    Ok(all)
}

async fn record_reports(pool: &PgPool, client: &VtnClient) -> Result<u64> {
    let reports = fetch_all_pages(client, "/reports").await?;
    let mut n = 0;
    for r in &reports {
        let Some((id, modified)) = dedup_key(r) else {
            continue;
        };
        let ven_name = r.get("clientName").and_then(|v| v.as_str());
        let report_type = r.get("reportName").and_then(|v| v.as_str());

        // GB-36: fetch the prior snapshot's max_interval_end for this
        // report_id so lag is computed only over intervals appended since
        // then, not the whole (ever-growing) intervals array.
        let prior_max_end: Option<DateTime<Utc>> = sqlx::query_scalar::<_, Option<DateTime<Utc>>>(
            "SELECT max_interval_end FROM lab_recorder.reports_received
             WHERE report_id = $1
             ORDER BY modification_date_time::timestamptz DESC
             LIMIT 1",
        )
        .bind(&id)
        .fetch_optional(pool)
        .await?
        .flatten();

        let (report_lag_s, max_interval_end) = match modified.parse::<DateTime<Utc>>() {
            Ok(modified_ts) => report_submission_lag_s(r, modified_ts, prior_max_end),
            Err(_) => (None, prior_max_end),
        };

        let res = sqlx::query(
            "INSERT INTO lab_recorder.reports_received
                (report_id, modification_date_time, ven_name, report_type, payload_json, report_lag_s, max_interval_end)
             VALUES ($1, $2, $3, $4, $5, $6, $7)
             ON CONFLICT (report_id, modification_date_time) DO NOTHING",
        )
        .bind(&id)
        .bind(&modified)
        .bind(ven_name)
        .bind(report_type)
        .bind(r)
        .bind(report_lag_s)
        .bind(max_interval_end)
        .execute(pool)
        .await?;
        n += res.rows_affected();
    }
    Ok(n)
}

async fn record_events(pool: &PgPool, client: &VtnClient) -> Result<u64> {
    let events = fetch_all_pages(client, "/events").await?;
    let mut n = 0;
    for e in &events {
        let Some((id, modified)) = dedup_key(e) else {
            continue;
        };
        let event_type = e.get("eventName").and_then(|v| v.as_str());
        let program_id = e.get("programID").and_then(|v| v.as_str());
        let res = sqlx::query(
            "INSERT INTO lab_recorder.events_published
                (event_id, modification_date_time, event_type, program_id, payload_json)
             VALUES ($1, $2, $3, $4, $5)
             ON CONFLICT (event_id, modification_date_time) DO NOTHING",
        )
        .bind(&id)
        .bind(&modified)
        .bind(event_type)
        .bind(program_id)
        .bind(e)
        .execute(pool)
        .await?;
        n += res.rows_affected();
    }
    Ok(n)
}

async fn record_ven_snapshots(pool: &PgPool, client: &VtnClient) -> Result<u64> {
    let vens: Vec<Value> = serde_json::from_value(client.get_json("/vens", None).await?)
        .context("vens response was not a JSON array")?;
    let now = chrono::Utc::now();
    let mut n = 0;
    for v in &vens {
        let Some(ven_name) = v.get("venName").and_then(|v| v.as_str()) else {
            continue;
        };
        sqlx::query(
            "INSERT INTO lab_recorder.ven_snapshots (ven_name, ts, last_seen, report_lag_s)
             VALUES ($1, $2, $2, NULL)
             ON CONFLICT (ven_name) DO UPDATE SET ts = EXCLUDED.ts, last_seen = EXCLUDED.ts",
        )
        .bind(ven_name)
        .bind(now)
        .execute(pool)
        .await?;
        n += 1;
    }
    Ok(n)
}

/// Connect + init the recorder schema, retrying forever with exponential
/// backoff (capped at 5 min) instead of giving up after one attempt. A
/// transient DB/DNS blip at startup (the 2026-08-10 incident: a Docker
/// internal-DNS hiccup during a `vtn-bff` restart) must not permanently
/// disable the recorder for the rest of the process's lifetime.
async fn connect_and_init_with_retry(database_url: &str, status: &SharedRecorderStatus) -> PgPool {
    let mut backoff = Duration::from_secs(INITIAL_BACKOFF_S);
    loop {
        let attempt = async {
            let pool = PgPool::connect(database_url).await?;
            init_schema(&pool).await?;
            Ok::<PgPool, anyhow::Error>(pool)
        };
        match attempt.await {
            Ok(pool) => {
                mark_connect_success(&mut *status.write().await);
                return pool;
            }
            Err(e) => {
                error!("recorder: connect/init failed, retrying in {backoff:?}: {e:#}");
                mark_connect_failure(&mut *status.write().await, &format!("{e:#}"));
                tokio::time::sleep(backoff).await;
                backoff = next_backoff(backoff);
            }
        }
    }
}

/// Spawns the recorder as a self-contained background task: connects (with
/// retry, see `connect_and_init_with_retry`) and then polls forever. Never
/// blocks or fails the caller — the whole recorder lifecycle, including its
/// initial connection, is decoupled from BFF startup so a recorder-side
/// problem can never take down the rest of the BFF.
pub fn spawn_recorder(
    database_url: String,
    business: VtnClient,
    ven_mgr: VtnClient,
    poll_secs: u64,
    status: SharedRecorderStatus,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let pool = connect_and_init_with_retry(&database_url, &status).await;
        info!("VTN recorder started (poll every {poll_secs}s)");

        let mut interval = tokio::time::interval(Duration::from_secs(poll_secs));
        loop {
            interval.tick().await;
            let mut had_success = false;

            match record_reports(&pool, &business).await {
                Ok(0) => had_success = true,
                Ok(n) => {
                    info!("recorder: {n} new report(s) archived");
                    had_success = true;
                }
                Err(e) => warn!("recorder: reports poll failed: {e:#}"),
            }
            match record_events(&pool, &business).await {
                Ok(0) => {}
                Ok(n) => info!("recorder: {n} new event(s) archived"),
                Err(e) => warn!("recorder: events poll failed: {e:#}"),
            }
            // /vens requires the VenManager role — the "any-business" client
            // (used for reports/events) is not authorized to list VENs.
            if let Err(e) = record_ven_snapshots(&pool, &ven_mgr).await {
                warn!("recorder: ven snapshot poll failed: {e:#}");
            }

            mark_poll_tick(&mut *status.write().await, Utc::now(), had_success);
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_dedup_key_extracts_id_and_modification_date_time() {
        let v = json!({"id": "r1", "modificationDateTime": "2026-01-01T00:00:00Z"});
        assert_eq!(
            dedup_key(&v),
            Some(("r1".to_string(), "2026-01-01T00:00:00Z".to_string()))
        );
    }

    #[test]
    fn test_dedup_key_missing_id_returns_none() {
        let v = json!({"modificationDateTime": "2026-01-01T00:00:00Z"});
        assert_eq!(dedup_key(&v), None);
    }

    #[test]
    fn test_dedup_key_missing_modification_date_time_returns_none() {
        let v = json!({"id": "r1"});
        assert_eq!(dedup_key(&v), None);
    }

    #[test]
    fn test_dedup_key_non_string_id_returns_none() {
        let v = json!({"id": 42, "modificationDateTime": "2026-01-01T00:00:00Z"});
        assert_eq!(dedup_key(&v), None);
    }

    // ── report_submission_lag_s (WP3.7, fixed for GB-36) ────────────

    #[test]
    fn test_parse_pt_duration_s_variants() {
        assert_eq!(parse_pt_duration_s("PT900S"), 900);
        assert_eq!(parse_pt_duration_s("PT15M"), 900);
        assert_eq!(parse_pt_duration_s("PT1H30M"), 5400);
        assert_eq!(parse_pt_duration_s("garbage"), 0);
        // Fully-qualified form the VTN normalizes durations to once a
        // report round-trips through it (real archived-report shape).
        assert_eq!(parse_pt_duration_s("P0Y0M0DT0H5M0S"), 300);
        assert_eq!(parse_pt_duration_s("P0Y0M0DT1H30M0S"), 5400);
        assert_eq!(parse_pt_duration_s("P0Y0M1DT0H0M0S"), 86400);
    }

    fn dt(s: &str) -> DateTime<Utc> {
        s.parse().unwrap()
    }

    #[test]
    fn test_report_lag_positive_for_past_measurement_window() {
        // Window [10:00, 10:15) modified at 10:15:30, no prior snapshot → 30s lag.
        let v = json!({
            "resources": [{"intervals": [{
                "intervalPeriod": {"start": "2026-01-01T10:00:00Z", "duration": "P0Y0M0DT0H15M0S"}
            }]}]
        });
        let (lag, new_max) = report_submission_lag_s(&v, dt("2026-01-01T10:15:30Z"), None);
        assert_eq!(lag, Some(30.0));
        assert_eq!(new_max, Some(dt("2026-01-01T10:15:00Z")));
    }

    #[test]
    fn test_report_lag_negative_for_forecast_window() {
        // Forecast slot ending 11:00 modified at 10:00, no prior snapshot → -3600s.
        let v = json!({
            "resources": [{"intervals": [{
                "intervalPeriod": {"start": "2026-01-01T10:55:00Z", "duration": "P0Y0M0DT0H5M0S"}
            }]}]
        });
        let (lag, _) = report_submission_lag_s(&v, dt("2026-01-01T10:00:00Z"), None);
        assert_eq!(lag, Some(-3600.0));
    }

    #[test]
    fn test_report_lag_uses_newest_interval() {
        let v = json!({
            "resources": [{"intervals": [
                {"intervalPeriod": {"start": "2026-01-01T10:00:00Z", "duration": "P0Y0M0DT0H15M0S"}},
                {"intervalPeriod": {"start": "2026-01-01T10:15:00Z", "duration": "P0Y0M0DT0H15M0S"}}
            ]}]
        });
        let (lag, _) = report_submission_lag_s(&v, dt("2026-01-01T10:30:00Z"), None);
        assert_eq!(lag, Some(0.0));
    }

    #[test]
    fn test_report_lag_none_without_intervals() {
        let no_intervals = json!({"resources": []});
        let (lag, new_max) =
            report_submission_lag_s(&no_intervals, dt("2026-01-01T10:00:00Z"), None);
        assert_eq!(lag, None);
        assert_eq!(new_max, None);
    }

    #[test]
    fn test_report_lag_first_poll_matches_whole_array_behavior() {
        // Regression guard: with no prior snapshot, behavior matches the
        // pre-fix whole-array computation (just against modificationDateTime
        // now instead of createdDateTime).
        let v = json!({
            "resources": [{"intervals": [
                {"intervalPeriod": {"start": "2026-01-01T00:00:00Z", "duration": "PT5M"}},
                {"intervalPeriod": {"start": "2026-01-01T00:05:00Z", "duration": "PT5M"}},
                {"intervalPeriod": {"start": "2026-01-01T00:10:00Z", "duration": "PT5M"}}
            ]}]
        });
        let (lag, new_max) = report_submission_lag_s(&v, dt("2026-01-01T00:15:10Z"), None);
        assert_eq!(lag, Some(10.0));
        assert_eq!(new_max, Some(dt("2026-01-01T00:15:00Z")));
    }

    #[test]
    fn test_report_lag_second_poll_only_counts_newly_appended_intervals() {
        // The core GB-36 regression: a report resource that grows across
        // polls must not have its lag computed over the whole accumulated
        // history — only over what's new since the prior snapshot.
        let first = json!({
            "resources": [{"intervals": [
                {"intervalPeriod": {"start": "2026-01-01T00:00:00Z", "duration": "PT5M"}},
                {"intervalPeriod": {"start": "2026-01-01T00:05:00Z", "duration": "PT5M"}}
            ]}]
        });
        let (_, prior_max) = report_submission_lag_s(&first, dt("2026-01-01T00:10:05Z"), None);
        assert_eq!(prior_max, Some(dt("2026-01-01T00:10:00Z")));

        // Resource grows: 20 more 5-min intervals appended (as in a
        // long-lived report resource), polled 5s after the newest one ends.
        let mut intervals = vec![
            json!({"intervalPeriod": {"start": "2026-01-01T00:00:00Z", "duration": "PT5M"}}),
            json!({"intervalPeriod": {"start": "2026-01-01T00:05:00Z", "duration": "PT5M"}}),
        ];
        for i in 0..20 {
            let start = dt("2026-01-01T00:10:00Z") + chrono::Duration::minutes(5 * i);
            intervals.push(json!({
                "intervalPeriod": {"start": start.to_rfc3339(), "duration": "PT5M"}
            }));
        }
        let second = json!({"resources": [{"intervals": intervals}]});
        let polled_at = dt("2026-01-01T00:10:00Z")
            + chrono::Duration::minutes(5 * 20)
            + chrono::Duration::seconds(5);
        let (lag, new_max) = report_submission_lag_s(&second, polled_at, prior_max);

        // Without the fix this would be ~6005s (measured against the whole
        // array's oldest history); with the fix it stays small (~5s), because
        // only the newest appended interval counts.
        assert_eq!(lag, Some(5.0));
        assert_eq!(new_max, Some(polled_at - chrono::Duration::seconds(5)));
    }

    #[test]
    fn test_report_lag_none_when_no_new_intervals_since_prior_poll() {
        let v = json!({
            "resources": [{"intervals": [
                {"intervalPeriod": {"start": "2026-01-01T00:00:00Z", "duration": "PT5M"}}
            ]}]
        });
        let prior = Some(dt("2026-01-01T00:05:00Z"));
        let (lag, new_max) = report_submission_lag_s(&v, dt("2026-01-01T00:06:00Z"), prior);
        assert_eq!(lag, None);
        assert_eq!(new_max, prior);
    }

    // ── recorder health/reconnect (2026-08-10 incident fix) ─────────────

    #[test]
    fn test_next_backoff_doubles_until_cap() {
        assert_eq!(
            next_backoff(Duration::from_secs(5)),
            Duration::from_secs(10)
        );
        assert_eq!(
            next_backoff(Duration::from_secs(200)),
            Duration::from_secs(300)
        );
        assert_eq!(
            next_backoff(Duration::from_secs(300)),
            Duration::from_secs(300)
        );
    }

    #[test]
    fn test_mark_connect_failure_sets_disconnected_and_increments_failures() {
        let mut s = RecorderStatus::default();
        mark_connect_failure(&mut s, "dns error");
        assert!(!s.connected);
        assert_eq!(s.consecutive_failures, 1);
        assert_eq!(s.last_error.as_deref(), Some("dns error"));

        mark_connect_failure(&mut s, "dns error again");
        assert_eq!(s.consecutive_failures, 2);
        assert_eq!(s.last_error.as_deref(), Some("dns error again"));
    }

    #[test]
    fn test_mark_connect_success_resets_failure_state() {
        let mut s = RecorderStatus {
            connected: false,
            consecutive_failures: 3,
            last_error: Some("x".into()),
            ..Default::default()
        };
        mark_connect_success(&mut s);
        assert!(s.connected);
        assert_eq!(s.consecutive_failures, 0);
        assert_eq!(s.last_error, None);
    }

    #[test]
    fn test_mark_poll_tick_updates_last_poll_always_and_success_conditionally() {
        let now = chrono::Utc::now();
        let mut s = RecorderStatus::default();

        mark_poll_tick(&mut s, now, false);
        assert_eq!(s.last_poll_at, Some(now));
        assert_eq!(s.last_success_at, None);

        mark_poll_tick(&mut s, now, true);
        assert_eq!(s.last_poll_at, Some(now));
        assert_eq!(s.last_success_at, Some(now));
    }
}
