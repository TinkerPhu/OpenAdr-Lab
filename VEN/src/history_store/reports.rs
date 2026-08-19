//! Paginated `reports_sent` query, split from `mod.rs` for the
//! 500-production-line file-size cap. Called only via the `HistoryPort`
//! impl on `SqliteHistoryStore`.

use chrono::{DateTime, Utc};
use rusqlite::{params, Connection};

use super::{from_unix, to_unix};
use crate::entities::history::{HistoryPage, ReportSent};
use crate::entities::DomainError;

/// `[from, to)`, `limit` rows starting at `offset` (`ORDER BY sent_at ASC`),
/// plus the total row count matching the range regardless of paging — lets a
/// paginated UI show "X-Y of total" without a second round trip.
pub(super) fn query(
    conn: &Connection,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
    limit: u32,
    offset: u32,
) -> Result<HistoryPage<ReportSent>, DomainError> {
    let from_ts = to_unix(from);
    let to_ts = to_unix(to);

    let total: u64 = conn
        .query_row(
            "SELECT count(*) FROM reports_sent WHERE sent_at >= ?1 AND sent_at < ?2",
            params![from_ts, to_ts],
            |r| r.get::<_, i64>(0),
        )
        .map_err(|e| DomainError::StorageError(format!("count query_reports: {e}")))?
        .max(0) as u64;

    let mut stmt = conn
        .prepare(
            "SELECT sent_at, report_type, event_id, payload_json FROM reports_sent
             WHERE sent_at >= ?1 AND sent_at < ?2 ORDER BY sent_at ASC LIMIT ?3 OFFSET ?4",
        )
        .map_err(|e| DomainError::StorageError(format!("prepare query_reports: {e}")))?;
    let raw: Vec<(i64, String, String, String)> = stmt
        .query_map(params![from_ts, to_ts, limit, offset], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        })
        .map_err(|e| DomainError::StorageError(format!("query_reports: {e}")))?
        .collect::<Result<_, _>>()
        .map_err(|e| DomainError::StorageError(format!("read query_reports rows: {e}")))?;

    let rows = raw
        .into_iter()
        .map(
            |(sent_at, report_type, event_id, payload_json)| -> Result<ReportSent, DomainError> {
                Ok(ReportSent {
                    sent_at: from_unix(sent_at)?,
                    report_type,
                    event_id,
                    payload_json,
                })
            },
        )
        .collect::<Result<_, _>>()?;

    Ok(HistoryPage { rows, total })
}

#[cfg(test)]
mod tests {
    use chrono::{DateTime, TimeZone, Utc};

    use super::super::SqliteHistoryStore;
    use crate::controller::HistoryPort;
    use crate::entities::history::ReportSent;

    fn ts(secs: i64) -> DateTime<Utc> {
        Utc.timestamp_opt(secs, 0).unwrap()
    }

    fn seed(store: &SqliteHistoryStore, count: i64) {
        for i in 0..count {
            store
                .append_report_sent(&ReportSent {
                    sent_at: ts(i * 10),
                    report_type: "TELEMETRY_USAGE".into(),
                    event_id: format!("evt-{i}"),
                    payload_json: "{}".into(),
                })
                .unwrap();
        }
    }

    #[test]
    fn query_returns_total_count_across_the_whole_range_not_just_the_page() {
        let store = SqliteHistoryStore::in_memory().unwrap();
        seed(&store, 5);
        let page = store.query_reports(ts(0), ts(1000), 2, 0).unwrap();
        assert_eq!(page.rows.len(), 2, "page is capped at `limit`");
        assert_eq!(
            page.total, 5,
            "total counts every matching row, not just this page"
        );
    }

    #[test]
    fn query_offset_advances_past_already_returned_rows() {
        let store = SqliteHistoryStore::in_memory().unwrap();
        seed(&store, 5);
        let first = store.query_reports(ts(0), ts(1000), 2, 0).unwrap();
        let second = store.query_reports(ts(0), ts(1000), 2, 2).unwrap();
        assert_eq!(
            first.rows.iter().map(|r| &r.event_id).collect::<Vec<_>>(),
            vec!["evt-0", "evt-1"]
        );
        assert_eq!(
            second.rows.iter().map(|r| &r.event_id).collect::<Vec<_>>(),
            vec!["evt-2", "evt-3"]
        );
    }

    #[test]
    fn query_offset_past_the_end_returns_no_rows_but_the_real_total() {
        let store = SqliteHistoryStore::in_memory().unwrap();
        seed(&store, 3);
        let page = store.query_reports(ts(0), ts(1000), 10, 100).unwrap();
        assert_eq!(page.rows.len(), 0);
        assert_eq!(page.total, 3);
    }
}
