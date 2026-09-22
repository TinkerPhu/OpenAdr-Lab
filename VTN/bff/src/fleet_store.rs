//! Keeping what the fleet published, so "what happened" has an answer.
//!
//! The live state in `fleet.rs` is one message per VEN: enough to open a
//! dashboard with, useless for "show me the last hour". This is the durable
//! half — every telemetry message appended to `lab_recorder.fleet_telemetry`
//! (D-7: the VTN's own Postgres, a separate schema, never its tables).
//!
//! Three properties the design turns on:
//!
//! - **Lossy in, honest out.** Telemetry is QoS 0 and the writer's queue is
//!   bounded. A sample that cannot be queued is dropped and *counted*, never
//!   waited for: the ingest task must not stall because the database is slow,
//!   and a gap that is reported is survivable in a way a silent one is not.
//! - **Batched.** One multi-row insert per drain rather than one per message —
//!   20 VENs at the tick cadence is a steady trickle, and a per-message insert
//!   would have the fleet feed competing with the live VTN for IO on the Pi.
//! - **Two timestamps.** `ts` is the VEN's own clock, `received_at` is ours.
//!   Keeping both is what lets §6.3 measure the offset instead of assuming it
//!   away; the series is built on `ts`, because that is when the reading was
//!   taken.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use anyhow::Result;
use chrono::{DateTime, Duration, Utc};
use lab_core::time_series::{Aggregation, Interpolation, TimeSeries};
use serde_json::Value;
use sqlx::PgPool;
use tokio::sync::mpsc;
use tracing::{info, warn};

/// How many rows may wait to be written before samples are dropped.
///
/// At 20 VENs on a 5 s cadence this is roughly twenty minutes of backlog: long
/// enough to ride out a slow database, short enough that we notice.
const QUEUE_CAPACITY: usize = 4096;

/// Rows per insert. One statement, bounded so a long stall does not produce a
/// single enormous query.
const MAX_BATCH: usize = 500;

/// D-6.
const RAW_RETENTION_DAYS: i64 = 7;
const ROLLUP_RETENTION_DAYS: i64 = 90;

/// Past this age a query is served from the 1-minute rollup, because the raw
/// rows are gone. Deliberately shorter than the retention itself: a query that
/// straddles the boundary should not silently lose its older half.
const RAW_QUERY_HORIZON_DAYS: i64 = RAW_RETENTION_DAYS - 1;

/// One telemetry message, as it is stored.
#[derive(Clone, Debug)]
pub struct TelemetryRow {
    pub ven_name: String,
    pub ts: DateTime<Utc>,
    pub received_at: DateTime<Utc>,
    pub net_power_w: Option<f64>,
    pub payload: Value,
}

impl TelemetryRow {
    /// Build a row from what a VEN published.
    ///
    /// The VEN stamps its own `ts`; an unreadable or absent one falls back to
    /// our receive time rather than dropping the sample, because a reading with
    /// a slightly wrong timestamp is worth more than no reading — and the
    /// `received_at` column still says what we actually know.
    pub fn from_message(ven_name: &str, payload: &Value, received_at: DateTime<Utc>) -> Self {
        let ts = payload
            .get("ts")
            .and_then(Value::as_str)
            .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
            .map(|t| t.with_timezone(&Utc))
            .unwrap_or(received_at);
        Self {
            ven_name: ven_name.to_string(),
            ts,
            received_at,
            net_power_w: crate::fleet::net_power_w(payload),
            payload: payload.clone(),
        }
    }
}

/// One controller decision a VEN published, as it is stored.
///
/// `event_id` is lifted out of the body into its own column because it is what
/// every reaction question is asked by ("what did the fleet do about event
/// X"), and a JSON path in a WHERE clause is the wrong shape for that.
#[derive(Clone, Debug)]
pub struct TraceRow {
    pub ven_name: String,
    pub ts: DateTime<Utc>,
    pub received_at: DateTime<Utc>,
    pub kind: String,
    pub event_id: Option<String>,
    pub payload: Value,
}

impl TraceRow {
    /// Build a row from a published decision, or `None` if it is not one.
    ///
    /// A body with no `type` is not a controller event; storing it as one with
    /// an empty kind would put a row in the table that no query can mean.
    pub fn from_message(
        ven_name: &str,
        payload: &Value,
        received_at: DateTime<Utc>,
    ) -> Option<Self> {
        let kind = payload.get("type").and_then(Value::as_str)?.to_string();
        let ts = payload
            .get("ts")
            .and_then(Value::as_str)
            .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
            .map(|t| t.with_timezone(&Utc))
            .unwrap_or(received_at);
        Some(Self {
            ven_name: ven_name.to_string(),
            ts,
            received_at,
            kind,
            // Absent on decisions that are not about one event (a periodic
            // plan cycle, an arbiter pass). Null, not "", so a query for a
            // specific event cannot accidentally match them.
            event_id: payload
                .get("event_id")
                .and_then(Value::as_str)
                .filter(|v| !v.is_empty())
                .map(str::to_string),
            payload: payload.clone(),
        })
    }
}

/// The write side of the store, held by the ingest task.
///
/// A clone-able queue handle rather than the pool itself, so ingest cannot
/// accidentally do database work on the path that reads the broker.
#[derive(Clone)]
pub struct TelemetryWriter {
    tx: mpsc::Sender<TelemetryRow>,
    trace_tx: mpsc::Sender<TraceRow>,
    dropped: Arc<AtomicU64>,
}

impl TelemetryWriter {
    /// Queue one row, or drop it.
    ///
    /// `try_send` rather than `send`: awaiting here would put database latency
    /// on the broker's event loop, and the next sample is seconds away anyway.
    pub fn offer(&self, row: TelemetryRow) {
        if self.tx.try_send(row).is_err() {
            let n = self.dropped.fetch_add(1, Ordering::Relaxed) + 1;
            // Once per thousand: enough to see a persistent problem in the
            // log, not enough to become the problem.
            if n % 1000 == 1 {
                warn!(dropped = n, "fleet telemetry store queue full, dropping");
            }
        }
    }

    /// Queue one decision, or drop it.
    ///
    /// Dropping here costs more than dropping a telemetry sample -- a decision
    /// happens once -- which is why it shares the same counter: the number
    /// being non-zero is the signal, and the log line says which queue.
    pub fn offer_trace(&self, row: TraceRow) {
        if self.trace_tx.try_send(row).is_err() {
            let n = self.dropped.fetch_add(1, Ordering::Relaxed) + 1;
            if n % 1000 == 1 {
                warn!(dropped = n, "fleet trace store queue full, dropping");
            }
        }
    }

    /// How many samples have been dropped, for `/api/health`.
    pub fn dropped(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }
}

pub async fn init_schema(pool: &PgPool) -> Result<()> {
    sqlx::query("CREATE SCHEMA IF NOT EXISTS lab_recorder")
        .execute(pool)
        .await?;
    // (ven_name, ts) as the key makes a re-published retained message
    // idempotent: a subscriber reconnecting receives the last value again, and
    // it is the same reading, not a new one.
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS lab_recorder.fleet_telemetry (
            ven_name TEXT NOT NULL,
            ts TIMESTAMPTZ NOT NULL,
            received_at TIMESTAMPTZ NOT NULL,
            net_power_w DOUBLE PRECISION,
            payload_json JSONB NOT NULL,
            PRIMARY KEY (ven_name, ts)
        )",
    )
    .execute(pool)
    .await?;
    sqlx::query(
        "CREATE INDEX IF NOT EXISTS fleet_telemetry_ts_idx
         ON lab_recorder.fleet_telemetry (ts)",
    )
    .execute(pool)
    .await?;
    // `samples` is kept because a bucket built from two readings and one built
    // from twelve are not equally trustworthy, and the mean alone cannot say
    // which it is.
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS lab_recorder.fleet_telemetry_1m (
            ven_name TEXT NOT NULL,
            bucket TIMESTAMPTZ NOT NULL,
            net_power_w_mean DOUBLE PRECISION,
            samples INTEGER NOT NULL,
            PRIMARY KEY (ven_name, bucket)
        )",
    )
    .execute(pool)
    .await?;
    // No natural key here: two decisions of the same kind at the same instant
    // are two decisions, not one. An id column keeps them both.
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS lab_recorder.fleet_trace (
            id BIGSERIAL PRIMARY KEY,
            ven_name TEXT NOT NULL,
            ts TIMESTAMPTZ NOT NULL,
            received_at TIMESTAMPTZ NOT NULL,
            kind TEXT NOT NULL,
            event_id TEXT,
            payload_json JSONB NOT NULL
        )",
    )
    .execute(pool)
    .await?;
    sqlx::query(
        "CREATE INDEX IF NOT EXISTS fleet_trace_event_idx
         ON lab_recorder.fleet_trace (event_id, ts)",
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// Start the writer and the retention job; hand back the queue immediately.
///
/// The queue exists before the database does, so ingest can start subscribing
/// without waiting for a connection — and a database that is slow to come up
/// costs the first few samples rather than the whole feed. Same decoupling the
/// recorder learned in the 2026-08-10 incident, for the same reason.
pub fn spawn(database_url: String) -> (TelemetryWriter, SharedPool) {
    let (tx, rx) = mpsc::channel(QUEUE_CAPACITY);
    let (trace_tx, trace_rx) = mpsc::channel(QUEUE_CAPACITY);
    let writer = TelemetryWriter {
        tx,
        trace_tx,
        dropped: Arc::new(AtomicU64::new(0)),
    };
    let shared: SharedPool = Arc::new(tokio::sync::RwLock::new(None));
    let task_pool = shared.clone();
    tokio::spawn(async move {
        let pool = crate::db::connect_and_init_with_retry(
            &database_url,
            "fleet telemetry store",
            |pool| async move {
                init_schema(&pool).await?;
                Ok(pool)
            },
            |_err| async {},
        )
        .await;
        info!("fleet telemetry store connected");
        *task_pool.write().await = Some(pool.clone());
        tokio::spawn(retention_loop(pool.clone()));
        tokio::spawn(trace_write_loop(pool.clone(), trace_rx));
        write_loop(pool, rx).await;
    });
    (writer, shared)
}

/// The query side's view of the pool.
///
/// `None` until the connection is up, and the query route says so rather than
/// answering with an empty series: "the store is not ready" and "nothing
/// happened in that window" are different answers, and only one of them is
/// worth retrying.
pub type SharedPool = Arc<tokio::sync::RwLock<Option<PgPool>>>;

async fn write_loop(pool: PgPool, mut rx: mpsc::Receiver<TelemetryRow>) {
    let mut batch: Vec<TelemetryRow> = Vec::with_capacity(MAX_BATCH);
    while let Some(first) = rx.recv().await {
        batch.clear();
        batch.push(first);
        // Take whatever else is already waiting. Not a timed window: the queue
        // is the window, and draining it is what makes one insert per burst
        // instead of one per message.
        while batch.len() < MAX_BATCH {
            match rx.try_recv() {
                Ok(row) => batch.push(row),
                Err(_) => break,
            }
        }
        if let Err(e) = insert_batch(&pool, &batch).await {
            warn!(rows = batch.len(), error = %e, "fleet telemetry insert failed");
        }
    }
}

async fn trace_write_loop(pool: PgPool, mut rx: mpsc::Receiver<TraceRow>) {
    let mut batch: Vec<TraceRow> = Vec::with_capacity(MAX_BATCH);
    while let Some(first) = rx.recv().await {
        batch.clear();
        batch.push(first);
        while batch.len() < MAX_BATCH {
            match rx.try_recv() {
                Ok(row) => batch.push(row),
                Err(_) => break,
            }
        }
        if let Err(e) = insert_trace_batch(&pool, &batch).await {
            warn!(rows = batch.len(), error = %e, "fleet trace insert failed");
        }
    }
}

async fn insert_trace_batch(pool: &PgPool, rows: &[TraceRow]) -> Result<()> {
    if rows.is_empty() {
        return Ok(());
    }
    let names: Vec<&str> = rows.iter().map(|r| r.ven_name.as_str()).collect();
    let ts: Vec<DateTime<Utc>> = rows.iter().map(|r| r.ts).collect();
    let received: Vec<DateTime<Utc>> = rows.iter().map(|r| r.received_at).collect();
    let kinds: Vec<&str> = rows.iter().map(|r| r.kind.as_str()).collect();
    let event_ids: Vec<Option<String>> = rows.iter().map(|r| r.event_id.clone()).collect();
    let payloads: Vec<Value> = rows.iter().map(|r| r.payload.clone()).collect();

    sqlx::query(
        "INSERT INTO lab_recorder.fleet_trace
            (ven_name, ts, received_at, kind, event_id, payload_json)
         SELECT * FROM UNNEST($1::text[], $2::timestamptz[], $3::timestamptz[],
                              $4::text[], $5::text[], $6::jsonb[])",
    )
    .bind(&names)
    .bind(&ts)
    .bind(&received)
    .bind(&kinds)
    .bind(&event_ids)
    .bind(&payloads)
    .execute(pool)
    .await?;
    Ok(())
}

async fn insert_batch(pool: &PgPool, rows: &[TelemetryRow]) -> Result<()> {
    if rows.is_empty() {
        return Ok(());
    }
    // UNNEST rather than a generated VALUES list: one prepared statement
    // whatever the batch size, so the database is not re-planning per burst.
    let names: Vec<&str> = rows.iter().map(|r| r.ven_name.as_str()).collect();
    let ts: Vec<DateTime<Utc>> = rows.iter().map(|r| r.ts).collect();
    let received: Vec<DateTime<Utc>> = rows.iter().map(|r| r.received_at).collect();
    let power: Vec<Option<f64>> = rows.iter().map(|r| r.net_power_w).collect();
    let payloads: Vec<Value> = rows.iter().map(|r| r.payload.clone()).collect();

    sqlx::query(
        "INSERT INTO lab_recorder.fleet_telemetry
            (ven_name, ts, received_at, net_power_w, payload_json)
         SELECT * FROM UNNEST($1::text[], $2::timestamptz[], $3::timestamptz[],
                              $4::double precision[], $5::jsonb[])
         ON CONFLICT (ven_name, ts) DO NOTHING",
    )
    .bind(&names)
    .bind(&ts)
    .bind(&received)
    .bind(&power)
    .bind(&payloads)
    .execute(pool)
    .await?;
    Ok(())
}

async fn retention_loop(pool: PgPool) {
    let mut tick = tokio::time::interval(std::time::Duration::from_secs(15 * 60));
    loop {
        tick.tick().await;
        if let Err(e) = run_retention(&pool).await {
            warn!(error = %e, "fleet telemetry retention pass failed");
        }
    }
}

/// Roll the raw rows up, then drop what is past its retention (D-6).
///
/// The rollup starts from the newest existing bucket rather than a fixed
/// lookback window, so an outage longer than that window cannot leave raw rows
/// unaggregated and then delete them. Re-aggregating the boundary bucket is
/// why the upsert overwrites rather than skips: that bucket was incomplete
/// when it was first written.
pub async fn run_retention(pool: &PgPool) -> Result<()> {
    let rolled = sqlx::query(
        "INSERT INTO lab_recorder.fleet_telemetry_1m (ven_name, bucket, net_power_w_mean, samples)
         SELECT ven_name, date_trunc('minute', ts), avg(net_power_w), count(*)::int
         FROM lab_recorder.fleet_telemetry
         WHERE ts >= COALESCE((SELECT max(bucket) FROM lab_recorder.fleet_telemetry_1m),
                              '-infinity'::timestamptz)
         GROUP BY 1, 2
         ON CONFLICT (ven_name, bucket) DO UPDATE
           SET net_power_w_mean = EXCLUDED.net_power_w_mean,
               samples = EXCLUDED.samples",
    )
    .execute(pool)
    .await?
    .rows_affected();

    let raw = sqlx::query("DELETE FROM lab_recorder.fleet_telemetry WHERE ts < $1")
        .bind(Utc::now() - Duration::days(RAW_RETENTION_DAYS))
        .execute(pool)
        .await?
        .rows_affected();

    let rollup = sqlx::query("DELETE FROM lab_recorder.fleet_telemetry_1m WHERE bucket < $1")
        .bind(Utc::now() - Duration::days(ROLLUP_RETENTION_DAYS))
        .execute(pool)
        .await?
        .rows_affected();

    // Decisions are rare next to samples, so they keep the long retention:
    // "what did the fleet do about that event last month" is a question worth
    // being able to answer.
    sqlx::query("DELETE FROM lab_recorder.fleet_trace WHERE ts < $1")
        .bind(Utc::now() - Duration::days(ROLLUP_RETENTION_DAYS))
        .execute(pool)
        .await?;

    if rolled + raw + rollup > 0 {
        info!(
            rolled,
            raw_deleted = raw,
            rollup_deleted = rollup,
            "fleet telemetry retention pass"
        );
    }
    Ok(())
}

/// Which table answers a query starting at `from`.
///
/// Named rather than inlined because the answer is also part of the response:
/// a reader comparing a 5-second series against a 1-minute one should be told
/// which they are looking at, not left to infer it from the sample spacing.
pub fn source_for(from: DateTime<Utc>, now: DateTime<Utc>) -> &'static str {
    if from >= now - Duration::days(RAW_QUERY_HORIZON_DAYS) {
        "raw"
    } else {
        "rollup"
    }
}

/// Per-VEN power series over a window, resampled onto a shared grid.
///
/// The grid comes from `lab-core`, which is also what the VEN plans on: two
/// services answering "what is this series at 12:05" with different arithmetic
/// is the divergence the shared crate exists to prevent (D-06).
pub async fn power_series(
    pool: &PgPool,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
    step: Duration,
    now: DateTime<Utc>,
) -> Result<(&'static str, Vec<(String, TimeSeries)>)> {
    let source = source_for(from, now);
    let rows: Vec<(String, DateTime<Utc>, Option<f64>)> = if source == "raw" {
        sqlx::query_as(
            "SELECT ven_name, ts, net_power_w
             FROM lab_recorder.fleet_telemetry
             WHERE ts >= $1 AND ts < $2
             ORDER BY ven_name, ts",
        )
    } else {
        sqlx::query_as(
            "SELECT ven_name, bucket, net_power_w_mean
             FROM lab_recorder.fleet_telemetry_1m
             WHERE bucket >= $1 AND bucket < $2
             ORDER BY ven_name, bucket",
        )
    }
    .bind(from)
    .bind(to)
    .fetch_all(pool)
    .await?;

    Ok((source, group_and_resample(rows, step)))
}

/// Turn ordered `(ven, ts, value)` rows into one resampled series per VEN.
///
/// Separate from the query so the shaping is testable without a database —
/// the part that can be wrong in a way SQL cannot catch.
fn group_and_resample(
    rows: Vec<(String, DateTime<Utc>, Option<f64>)>,
    step: Duration,
) -> Vec<(String, TimeSeries)> {
    let mut out: Vec<(String, TimeSeries)> = Vec::new();
    for (ven, ts, value) in rows {
        // A row whose power is null is a message we stored but could not read
        // a meter value out of. It says the VEN was alive, not what it drew,
        // so it must not enter the series as a number.
        let Some(v) = value else { continue };
        match out.last_mut() {
            Some((name, series)) if *name == ven => series.samples.push((ts, v)),
            _ => out.push((
                ven,
                TimeSeries {
                    samples: vec![(ts, v)],
                    // Step, not Linear: a meter reading holds until the next
                    // one arrives. Interpolating between two samples would
                    // invent a ramp the site never had.
                    interpolation: Interpolation::Step,
                },
            )),
        }
    }
    for (_, series) in out.iter_mut() {
        *series = series.resample_uniform(step, Aggregation::Mean);
    }
    out
}

/// The fleet total at each grid timestamp, and how many VENs it is built from.
///
/// The count travels with the sum for the same reason the live route carries
/// `contributingVens`: a total over eleven VENs and a total over twenty look
/// identical once they are a number, and a fleet whose members drop in and out
/// of a window would otherwise show that as a change in demand.
///
/// VENs are summed only where they have a sample. A VEN absent from a bucket
/// is absent, not zero.
pub fn sum_over_grid(series: &[(String, TimeSeries)]) -> Vec<(DateTime<Utc>, f64, usize)> {
    let mut totals: std::collections::BTreeMap<DateTime<Utc>, (f64, usize)> = Default::default();
    for (_, s) in series {
        for (ts, v) in &s.samples {
            let entry = totals.entry(*ts).or_insert((0.0, 0));
            entry.0 += v;
            entry.1 += 1;
        }
    }
    totals
        .into_iter()
        .map(|(ts, (sum, n))| (ts, sum, n))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn t(s: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc)
    }

    #[test]
    fn from_message_takes_the_vens_own_timestamp() {
        let row = TelemetryRow::from_message(
            "ven-3",
            &json!({"ts": "2026-09-22T10:00:00Z", "grid": {"net_power_w": 1200.0}}),
            t("2026-09-22T10:00:02Z"),
        );
        assert_eq!(row.ts, t("2026-09-22T10:00:00Z"));
        assert_eq!(row.received_at, t("2026-09-22T10:00:02Z"));
        assert_eq!(row.net_power_w, Some(1200.0));
    }

    /// A reading with a slightly wrong timestamp beats no reading, and
    /// `received_at` still records what we actually know.
    #[test]
    fn from_message_falls_back_to_our_clock_when_the_vens_is_unreadable() {
        for body in [json!({"grid": {"net_power_w": 1.0}}), json!({"ts": "soon"})] {
            let row = TelemetryRow::from_message("ven-3", &body, t("2026-09-22T10:00:02Z"));
            assert_eq!(row.ts, t("2026-09-22T10:00:02Z"));
        }
    }

    /// Storing the message is not the same as understanding it: a body with no
    /// meter reading is kept, with a null value rather than a zero.
    #[test]
    fn from_message_keeps_a_body_it_cannot_read_a_meter_out_of() {
        let row =
            TelemetryRow::from_message("ven-3", &json!({"assets": {}}), t("2026-09-22T10:00:00Z"));
        assert!(row.net_power_w.is_none());
        assert_eq!(row.payload, json!({"assets": {}}));
    }

    #[test]
    fn source_is_the_rollup_only_once_the_raw_rows_are_gone() {
        let now = t("2026-09-22T10:00:00Z");
        assert_eq!(source_for(now - Duration::hours(2), now), "raw");
        assert_eq!(source_for(now - Duration::days(5), now), "raw");
        assert_eq!(source_for(now - Duration::days(30), now), "rollup");
    }

    #[test]
    fn group_and_resample_splits_by_ven_and_lands_on_a_shared_grid() {
        let rows = vec![
            ("ven-1".into(), t("2026-09-22T10:00:00Z"), Some(1000.0)),
            ("ven-1".into(), t("2026-09-22T10:00:30Z"), Some(2000.0)),
            ("ven-1".into(), t("2026-09-22T10:01:00Z"), Some(2000.0)),
            ("ven-2".into(), t("2026-09-22T10:00:00Z"), Some(-500.0)),
            ("ven-2".into(), t("2026-09-22T10:01:00Z"), Some(-500.0)),
        ];
        let out = group_and_resample(rows, Duration::minutes(1));
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].0, "ven-1");
        assert_eq!(out[1].0, "ven-2");
        // Both VENs land on the same timestamps, which is what makes a fleet
        // sum addable rather than approximate.
        let g1: Vec<_> = out[0].1.samples.iter().map(|(ts, _)| *ts).collect();
        let g2: Vec<_> = out[1].1.samples.iter().map(|(ts, _)| *ts).collect();
        assert_eq!(g1, g2);
        assert!(!g1.is_empty());
    }

    /// Export stays negative through the store and the resampling, the same
    /// invariant F-3 fixed on the report side.
    #[test]
    fn group_and_resample_keeps_exported_power_negative() {
        let rows = vec![
            ("ven-1".into(), t("2026-09-22T10:00:00Z"), Some(-3000.0)),
            ("ven-1".into(), t("2026-09-22T10:01:00Z"), Some(-3000.0)),
            ("ven-1".into(), t("2026-09-22T10:02:00Z"), Some(-3000.0)),
        ];
        let out = group_and_resample(rows, Duration::minutes(1));
        assert!(out[0].1.samples.iter().all(|(_, v)| *v == -3000.0));
    }

    #[test]
    fn sum_over_grid_adds_import_and_export_with_their_signs() {
        let series = vec![
            (
                "ven-1".to_string(),
                TimeSeries {
                    samples: vec![(t("2026-09-22T10:00:00Z"), 4000.0)],
                    interpolation: Interpolation::Step,
                },
            ),
            (
                "ven-2".to_string(),
                TimeSeries {
                    samples: vec![(t("2026-09-22T10:00:00Z"), -1500.0)],
                    interpolation: Interpolation::Step,
                },
            ),
        ];
        assert_eq!(
            sum_over_grid(&series),
            vec![(t("2026-09-22T10:00:00Z"), 2500.0, 2)]
        );
    }

    /// A VEN with no sample in a bucket is absent from it, and the count says
    /// so -- otherwise a fleet losing members mid-window reads as falling
    /// demand.
    #[test]
    fn sum_over_grid_counts_only_the_vens_present_in_each_bucket() {
        let series = vec![
            (
                "ven-1".to_string(),
                TimeSeries {
                    samples: vec![
                        (t("2026-09-22T10:00:00Z"), 1000.0),
                        (t("2026-09-22T10:01:00Z"), 1000.0),
                    ],
                    interpolation: Interpolation::Step,
                },
            ),
            (
                "ven-2".to_string(),
                TimeSeries {
                    samples: vec![(t("2026-09-22T10:00:00Z"), 1000.0)],
                    interpolation: Interpolation::Step,
                },
            ),
        ];
        assert_eq!(
            sum_over_grid(&series),
            vec![
                (t("2026-09-22T10:00:00Z"), 2000.0, 2),
                (t("2026-09-22T10:01:00Z"), 1000.0, 1),
            ]
        );
    }

    /// A stored message we could not read a meter out of says the VEN was
    /// alive, not what it drew. Zero would be a different claim.
    #[test]
    fn group_and_resample_drops_rows_with_no_reading_rather_than_reading_them_as_zero() {
        let rows = vec![
            ("ven-1".into(), t("2026-09-22T10:00:00Z"), Some(1000.0)),
            ("ven-1".into(), t("2026-09-22T10:00:30Z"), None),
            ("ven-1".into(), t("2026-09-22T10:01:00Z"), Some(1000.0)),
        ];
        let out = group_and_resample(rows, Duration::minutes(1));
        assert!(out[0].1.samples.iter().all(|(_, v)| *v == 1000.0));
    }
}
