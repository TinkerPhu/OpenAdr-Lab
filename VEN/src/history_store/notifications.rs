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
        "INSERT INTO notifications (id, created_at, severity, message, asset_id, event_id)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            row.id.to_string(),
            to_unix(row.created_at),
            severity_to_str(&row.severity),
            row.message,
            row.asset_id,
            row.event_id,
        ],
    )
    .map_err(|e| DomainError::StorageError(format!("insert notification: {e}")))?;
    Ok(())
}

pub(super) fn query(
    conn: &Connection,
    since: Option<DateTime<Utc>>,
    limit: usize,
) -> Result<Vec<UserNotification>, DomainError> {
    let since_unix = since.map(to_unix).unwrap_or(i64::MIN);
    let mut stmt = conn
        .prepare(
            "SELECT id, created_at, severity, message, asset_id, event_id
             FROM notifications WHERE created_at > ?1
             ORDER BY created_at ASC LIMIT ?2",
        )
        .map_err(|e| DomainError::StorageError(format!("prepare query: {e}")))?;
    let rows = stmt
        .query_map(params![since_unix, limit as i64], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, Option<String>>(4)?,
                r.get::<_, Option<String>>(5)?,
            ))
        })
        .map_err(|e| DomainError::StorageError(format!("query notifications: {e}")))?;
    let mut out = Vec::new();
    for row in rows {
        let (id, created_at, severity, message, asset_id, event_id) =
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
        });
    }
    Ok(out)
}
