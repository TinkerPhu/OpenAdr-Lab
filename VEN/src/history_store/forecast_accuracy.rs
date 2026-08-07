//! forecast_accuracy_samples persistence, split from `mod.rs` for the
//! 500-production-line file-size cap. Called only via the `HistoryPort`
//! impl on `SqliteHistoryStore`. See `openspec/changes/forecast-accuracy-tracking/`.

use std::str::FromStr;

use chrono::{DateTime, Utc};
use rusqlite::{params, Connection};

use super::{from_unix, to_unix};
use crate::entities::history::{ForecastAccuracySample, ForecastLeadKind, TickSample};
use crate::entities::DomainError;

pub(super) fn append(
    conn: &mut Connection,
    rows: &[ForecastAccuracySample],
) -> Result<(), DomainError> {
    let tx = conn
        .transaction()
        .map_err(|e| DomainError::StorageError(format!("begin tx: {e}")))?;
    {
        let mut stmt = tx
            .prepare(
                "INSERT INTO forecast_accuracy_samples
                    (asset_id, lead_kind, target_ts, predicted_kw, predicted_at, actual_kw, actual_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            )
            .map_err(|e| DomainError::StorageError(format!("prepare insert: {e}")))?;
        for row in rows {
            stmt.execute(params![
                row.asset_id,
                row.lead_kind.to_string(),
                to_unix(row.target_ts),
                row.predicted_kw,
                to_unix(row.predicted_at),
                row.actual_kw,
                row.actual_at.map(to_unix),
            ])
            .map_err(|e| DomainError::StorageError(format!("insert forecast sample: {e}")))?;
        }
    }
    tx.commit()
        .map_err(|e| DomainError::StorageError(format!("commit tx: {e}")))?;
    Ok(())
}

/// For each tick row, fill in `actual_kw`/`actual_at` on any still-open (`actual_kw IS NULL`)
/// sample for that `asset_id` whose `target_ts` falls in `[ticks.ts, ticks.ts + window_s)` —
/// design.md Decision 4. A row is only ever reconciled once: the `actual_kw IS NULL` guard means
/// a second call over the same window is a no-op for already-reconciled rows. Batched in one
/// transaction (mirroring `append` above) so a multi-asset flush commits atomically instead of
/// once per tick.
pub(super) fn reconcile(
    conn: &mut Connection,
    ticks: &[TickSample],
    window_s: i64,
) -> Result<(), DomainError> {
    let tx = conn
        .transaction()
        .map_err(|e| DomainError::StorageError(format!("begin tx: {e}")))?;
    {
        let mut stmt = tx
            .prepare(
                "UPDATE forecast_accuracy_samples
                 SET actual_kw = ?1, actual_at = ?2
                 WHERE asset_id = ?3 AND actual_kw IS NULL AND target_ts >= ?4 AND target_ts < ?5",
            )
            .map_err(|e| DomainError::StorageError(format!("prepare reconcile: {e}")))?;
        for tick in ticks {
            let window_start = to_unix(tick.ts);
            stmt.execute(params![
                tick.power_kw,
                window_start,
                tick.asset_id,
                window_start,
                window_start + window_s,
            ])
            .map_err(|e| DomainError::StorageError(format!("reconcile forecast sample: {e}")))?;
        }
    }
    tx.commit()
        .map_err(|e| DomainError::StorageError(format!("commit tx: {e}")))?;
    Ok(())
}

pub(super) fn query(
    conn: &Connection,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
    asset_id: Option<&str>,
    lead_kind: Option<ForecastLeadKind>,
) -> Result<Vec<ForecastAccuracySample>, DomainError> {
    type Row = (String, String, i64, f64, i64, Option<f64>, Option<i64>);

    let mut stmt = conn
        .prepare(
            "SELECT asset_id, lead_kind, target_ts, predicted_kw, predicted_at, actual_kw, actual_at
             FROM forecast_accuracy_samples
             WHERE target_ts >= ?1 AND target_ts < ?2
               AND (?3 IS NULL OR asset_id = ?3)
               AND (?4 IS NULL OR lead_kind = ?4)
             ORDER BY target_ts ASC",
        )
        .map_err(|e| DomainError::StorageError(format!("prepare query_forecast_accuracy: {e}")))?;
    let lead_kind_str = lead_kind.map(|k| k.to_string());
    let raw: Vec<Row> = stmt
        .query_map(
            params![to_unix(from), to_unix(to), asset_id, lead_kind_str],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    row.get(6)?,
                ))
            },
        )
        .map_err(|e| DomainError::StorageError(format!("query_forecast_accuracy: {e}")))?
        .collect::<Result<_, _>>()
        .map_err(|e| {
            DomainError::StorageError(format!("read query_forecast_accuracy rows: {e}"))
        })?;

    raw.into_iter()
        .map(
            |(asset_id, lead_kind, target_ts, predicted_kw, predicted_at, actual_kw, actual_at)| {
                Ok(ForecastAccuracySample {
                    asset_id,
                    lead_kind: ForecastLeadKind::from_str(&lead_kind).map_err(|e| {
                        DomainError::StorageError(format!("invalid stored lead_kind: {e}"))
                    })?,
                    target_ts: from_unix(target_ts)?,
                    predicted_kw,
                    predicted_at: from_unix(predicted_at)?,
                    actual_kw,
                    actual_at: actual_at.map(from_unix).transpose()?,
                })
            },
        )
        .collect()
}
