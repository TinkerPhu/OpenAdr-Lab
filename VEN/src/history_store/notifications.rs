//! WP4.3 (BL-20) — notifications-table persistence, split from `mod.rs`
//! for the 500-production-line file-size cap. Called only via the
//! `HistoryPort` impl on `SqliteHistoryStore`.

use chrono::{DateTime, Utc};
use rusqlite::{params, Connection};

use super::{from_unix, to_unix};
use crate::entities::design_vocabulary::UserNotificationSeverity;
use crate::entities::notification::UserNotification;
use crate::entities::DomainError;

/// Severity ⇄ TEXT column (wire names match the serde SCREAMING_SNAKE form).
fn severity_to_str(s: &UserNotificationSeverity) -> &'static str {
    match s {
        UserNotificationSeverity::Info => "INFO",
        UserNotificationSeverity::Warn => "WARN",
        UserNotificationSeverity::Alert => "ALERT",
    }
}

fn severity_from_str(s: &str) -> Result<UserNotificationSeverity, DomainError> {
    match s {
        "INFO" => Ok(UserNotificationSeverity::Info),
        "WARN" => Ok(UserNotificationSeverity::Warn),
        "ALERT" => Ok(UserNotificationSeverity::Alert),
        other => Err(DomainError::StorageError(format!(
            "invalid stored severity: {other}"
        ))),
    }
}

pub(super) fn append(conn: &Connection, row: &UserNotification) -> Result<(), DomainError> {
    conn.execute(
        "INSERT INTO notifications
            (id, created_at, severity, message, asset_id, event_id,
             dedup_key, count, last_seen_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            row.id.to_string(),
            to_unix(row.created_at),
            severity_to_str(&row.severity),
            row.message,
            row.asset_id,
            row.event_id,
            row.dedup_key,
            row.count,
            to_unix(row.last_seen_at),
        ],
    )
    .map_err(|e| DomainError::StorageError(format!("insert notification: {e}")))?;
    Ok(())
}

/// 030 (notification-dedup): bump an existing row on a dedup hit. The
/// window decision is the application layer's (`Notifier`) — this only
/// records the outcome.
pub(super) fn update_seen(
    conn: &Connection,
    id: uuid::Uuid,
    count: u32,
    last_seen_at: DateTime<Utc>,
) -> Result<(), DomainError> {
    let n = conn
        .execute(
            "UPDATE notifications SET count = ?2, last_seen_at = ?3 WHERE id = ?1",
            params![id.to_string(), count, to_unix(last_seen_at)],
        )
        .map_err(|e| DomainError::StorageError(format!("update notification seen: {e}")))?;
    if n == 0 {
        return Err(DomainError::NotFound { id });
    }
    Ok(())
}

pub(super) fn query(
    conn: &Connection,
    since: Option<DateTime<Utc>>,
    limit: usize,
    severity: Option<UserNotificationSeverity>,
) -> Result<Vec<UserNotification>, DomainError> {
    let since_unix = since.map(to_unix).unwrap_or(i64::MIN);
    // 030: severity filter applies BEFORE the limit so a filtered page still
    // holds `limit` matching rows. '%' matches all severities.
    let severity_filter = severity.as_ref().map(severity_to_str).unwrap_or("%");
    // The NEWEST `limit` rows, returned oldest-first: seeding the in-memory
    // ring must keep the most recent history, not the most ancient (found in
    // the Phase 3+4 review — a plain ASC LIMIT drops the newest rows once
    // more than `limit` notifications have accumulated).
    // Recency = last_seen_at (equals created_at until a dedup hit), so a
    // long-running deduplicated condition stays in the newest rows.
    let mut stmt = conn
        .prepare(
            "SELECT id, created_at, severity, message, asset_id, event_id,
                    dedup_key, count, last_seen_at FROM (
                 SELECT id, created_at, severity, message, asset_id, event_id,
                        dedup_key, count, last_seen_at
                 FROM notifications WHERE last_seen_at > ?1 AND severity LIKE ?3
                 ORDER BY last_seen_at DESC LIMIT ?2
             ) ORDER BY last_seen_at ASC",
        )
        .map_err(|e| DomainError::StorageError(format!("prepare query: {e}")))?;
    let rows = stmt
        .query_map(params![since_unix, limit as i64, severity_filter], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, Option<String>>(4)?,
                r.get::<_, Option<String>>(5)?,
                r.get::<_, Option<String>>(6)?,
                r.get::<_, u32>(7)?,
                r.get::<_, i64>(8)?,
            ))
        })
        .map_err(|e| DomainError::StorageError(format!("query notifications: {e}")))?;
    let mut out = Vec::new();
    for row in rows {
        let (id, created_at, severity, message, asset_id, event_id, dedup_key, count, last_seen_at) =
            row.map_err(|e| DomainError::StorageError(format!("read row: {e}")))?;
        out.push(UserNotification {
            id: id
                .parse()
                .map_err(|e| DomainError::StorageError(format!("invalid stored uuid: {e}")))?,
            created_at: from_unix(created_at)?,
            severity: severity_from_str(&severity)?,
            message,
            asset_id,
            event_id,
            dedup_key,
            count,
            last_seen_at: from_unix(last_seen_at)?,
        });
    }
    Ok(out)
}
