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
use std::time::Duration;

use anyhow::{Context, Result};
use serde_json::Value;
use sqlx::PgPool;
use tracing::{info, warn};

use crate::vtn_client::VtnClient;

const PAGE_LIMIT: i64 = 50;

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

/// Parse the simple `PT#H#M#S` ISO-8601 duration subset the VEN reporter
/// emits (`format_iso8601_duration`). Unknown shapes parse as 0 seconds.
fn parse_pt_duration_s(s: &str) -> i64 {
    let Some(rest) = s.strip_prefix("PT") else {
        return 0;
    };
    let mut total = 0i64;
    let mut num = String::new();
    for c in rest.chars() {
        if c.is_ascii_digit() {
            num.push(c);
        } else {
            let v: i64 = num.parse().unwrap_or(0);
            num.clear();
            total += match c {
                'H' => v * 3600,
                'M' => v * 60,
                'S' => v,
                _ => 0,
            };
        }
    }
    total
}

/// WP3.7 — SG-3 timeliness: seconds between the end of the newest reported
/// interval window and the report's own `createdDateTime` (VTN ingestion
/// time). Positive = the report arrived after the window it measures (normal
/// for measurements — small is timely); negative = the report describes a
/// future window (normal for USAGE_FORECAST). `None` when the report has no
/// parseable `createdDateTime` or no interval with a parseable start.
fn report_submission_lag_s(report: &Value) -> Option<f64> {
    let created = report
        .get("createdDateTime")?
        .as_str()?
        .parse::<chrono::DateTime<chrono::Utc>>()
        .ok()?;

    let window_end = report
        .get("resources")?
        .as_array()?
        .iter()
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
                .parse::<chrono::DateTime<chrono::Utc>>()
                .ok()?;
            let dur_s = period
                .get("duration")
                .and_then(|d| d.as_str())
                .map(parse_pt_duration_s)
                .unwrap_or(0);
            Some(start + chrono::Duration::seconds(dur_s))
        })
        .max()?;

    Some((created - window_end).num_milliseconds() as f64 / 1000.0)
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
        let report_lag_s = report_submission_lag_s(r);
        let res = sqlx::query(
            "INSERT INTO lab_recorder.reports_received
                (report_id, modification_date_time, ven_name, report_type, payload_json, report_lag_s)
             VALUES ($1, $2, $3, $4, $5, $6)
             ON CONFLICT (report_id, modification_date_time) DO NOTHING",
        )
        .bind(&id)
        .bind(&modified)
        .bind(ven_name)
        .bind(report_type)
        .bind(r)
        .bind(report_lag_s)
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

pub fn spawn_recorder(
    pool: PgPool,
    business: VtnClient,
    ven_mgr: VtnClient,
    poll_secs: u64,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(poll_secs));
        loop {
            interval.tick().await;
            match record_reports(&pool, &business).await {
                Ok(0) => {}
                Ok(n) => info!("recorder: {n} new report(s) archived"),
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

    // ── report_submission_lag_s (WP3.7) ─────────────────────────────

    #[test]
    fn test_parse_pt_duration_s_variants() {
        assert_eq!(parse_pt_duration_s("PT900S"), 900);
        assert_eq!(parse_pt_duration_s("PT15M"), 900);
        assert_eq!(parse_pt_duration_s("PT1H30M"), 5400);
        assert_eq!(parse_pt_duration_s("garbage"), 0);
    }

    #[test]
    fn test_report_lag_positive_for_past_measurement_window() {
        // Window [10:00, 10:15) reported at 10:15:30 → 30s lag.
        let v = json!({
            "createdDateTime": "2026-01-01T10:15:30Z",
            "resources": [{"intervals": [{
                "intervalPeriod": {"start": "2026-01-01T10:00:00Z", "duration": "PT15M"}
            }]}]
        });
        assert_eq!(report_submission_lag_s(&v), Some(30.0));
    }

    #[test]
    fn test_report_lag_negative_for_forecast_window() {
        // Forecast slot ending 11:00 reported at 10:00 → -3600s.
        let v = json!({
            "createdDateTime": "2026-01-01T10:00:00Z",
            "resources": [{"intervals": [{
                "intervalPeriod": {"start": "2026-01-01T10:55:00Z", "duration": "PT5M"}
            }]}]
        });
        assert_eq!(report_submission_lag_s(&v), Some(-3600.0));
    }

    #[test]
    fn test_report_lag_uses_newest_interval() {
        let v = json!({
            "createdDateTime": "2026-01-01T10:30:00Z",
            "resources": [{"intervals": [
                {"intervalPeriod": {"start": "2026-01-01T10:00:00Z", "duration": "PT15M"}},
                {"intervalPeriod": {"start": "2026-01-01T10:15:00Z", "duration": "PT15M"}}
            ]}]
        });
        assert_eq!(report_submission_lag_s(&v), Some(0.0));
    }

    #[test]
    fn test_report_lag_none_without_created_or_intervals() {
        let no_created = json!({"resources": [{"intervals": [{
            "intervalPeriod": {"start": "2026-01-01T10:00:00Z"}
        }]}]});
        assert_eq!(report_submission_lag_s(&no_created), None);

        let no_intervals = json!({"createdDateTime": "2026-01-01T10:00:00Z", "resources": []});
        assert_eq!(report_submission_lag_s(&no_intervals), None);
    }
}
