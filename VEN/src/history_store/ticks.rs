//! tick_samples persistence, split from `mod.rs` for the 500-production-line
//! file-size cap. Called only via the `HistoryPort` impl on `SqliteHistoryStore`.

use chrono::{DateTime, Utc};
use rusqlite::{params, Connection};

use super::{from_unix, to_unix};
use crate::entities::history::TickSample;
use crate::entities::DomainError;

pub(super) type TickSampleRow = (
    i64,
    String,
    f64,
    Option<f64>,
    Option<f64>,
    Option<f64>,
    Option<String>,
);

pub(super) fn append(conn: &mut Connection, rows: &[TickSample]) -> Result<(), DomainError> {
    let tx = conn
        .transaction()
        .map_err(|e| DomainError::StorageError(format!("begin tx: {e}")))?;
    {
        let mut stmt = tx
            .prepare(
                "INSERT INTO tick_samples
                    (ts, asset_id, power_kw, soc_pct, temperature_c, generation_limit_kw, curtailment_source)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            )
            .map_err(|e| DomainError::StorageError(format!("prepare insert: {e}")))?;
        for row in rows {
            stmt.execute(params![
                to_unix(row.ts),
                row.asset_id,
                row.power_kw,
                row.soc_pct,
                row.temperature_c,
                row.generation_limit_kw,
                row.curtailment_source,
            ])
            .map_err(|e| DomainError::StorageError(format!("insert tick sample: {e}")))?;
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
) -> Result<Vec<TickSample>, DomainError> {
    let (sql, asset_filter): (&str, Option<&str>) = match asset_id {
        Some(id) => (
            "SELECT ts, asset_id, power_kw, soc_pct, temperature_c, generation_limit_kw, curtailment_source
             FROM tick_samples WHERE ts >= ?1 AND ts < ?2 AND asset_id = ?3 ORDER BY ts ASC",
            Some(id),
        ),
        None => (
            "SELECT ts, asset_id, power_kw, soc_pct, temperature_c, generation_limit_kw, curtailment_source
             FROM tick_samples WHERE ts >= ?1 AND ts < ?2 ORDER BY ts ASC",
            None,
        ),
    };
    let mut stmt = conn
        .prepare(sql)
        .map_err(|e| DomainError::StorageError(format!("prepare query_ticks: {e}")))?;
    let map_row = |row: &rusqlite::Row| -> rusqlite::Result<TickSampleRow> {
        Ok((
            row.get(0)?,
            row.get(1)?,
            row.get(2)?,
            row.get(3)?,
            row.get(4)?,
            row.get(5)?,
            row.get(6)?,
        ))
    };
    let raw: Vec<_> = if let Some(id) = asset_filter {
        stmt.query_map(params![to_unix(from), to_unix(to), id], map_row)
    } else {
        stmt.query_map(params![to_unix(from), to_unix(to)], map_row)
    }
    .map_err(|e| DomainError::StorageError(format!("query_ticks: {e}")))?
    .collect::<Result<_, _>>()
    .map_err(|e| DomainError::StorageError(format!("read query_ticks rows: {e}")))?;

    raw.into_iter()
        .map(
            |(
                ts,
                asset_id,
                power_kw,
                soc_pct,
                temperature_c,
                generation_limit_kw,
                curtailment_source,
            )|
             -> Result<TickSample, DomainError> {
                Ok(TickSample {
                    ts: from_unix(ts)?,
                    asset_id,
                    power_kw,
                    soc_pct,
                    temperature_c,
                    generation_limit_kw,
                    curtailment_source,
                })
            },
        )
        .collect()
}
