//! WP4.2 (BL-19) — `SettingsPort` implementation on the SQLite store
//! (user_settings table, schema v3). Split from `mod.rs` for the
//! 500-production-line file-size cap.

use chrono::{DateTime, Utc};
use rusqlite::params;

use super::{to_unix, SqliteHistoryStore};
use crate::controller::SettingsPort;
use crate::entities::DomainError;

impl SettingsPort for SqliteHistoryStore {
    fn put_setting(
        &self,
        key: &str,
        asset_id: &str,
        value_json: &str,
        updated_at: DateTime<Utc>,
    ) -> Result<(), DomainError> {
        let conn = self.lock()?;
        conn.execute(
            "INSERT INTO user_settings (key, asset_id, value_json, updated_at)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(key, asset_id) DO UPDATE
             SET value_json = excluded.value_json, updated_at = excluded.updated_at",
            params![key, asset_id, value_json, to_unix(updated_at)],
        )
        .map_err(|e| DomainError::StorageError(format!("upsert setting: {e}")))?;
        Ok(())
    }

    fn get_setting(&self, key: &str, asset_id: &str) -> Result<Option<String>, DomainError> {
        let conn = self.lock()?;
        conn.query_row(
            "SELECT value_json FROM user_settings WHERE key = ?1 AND asset_id = ?2",
            params![key, asset_id],
            |r| r.get::<_, String>(0),
        )
        .map(Some)
        .or_else(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => Ok(None),
            other => Err(DomainError::StorageError(format!("get setting: {other}"))),
        })
    }

    fn delete_setting(&self, key: &str, asset_id: &str) -> Result<bool, DomainError> {
        let conn = self.lock()?;
        let n = conn
            .execute(
                "DELETE FROM user_settings WHERE key = ?1 AND asset_id = ?2",
                params![key, asset_id],
            )
            .map_err(|e| DomainError::StorageError(format!("delete setting: {e}")))?;
        Ok(n > 0)
    }

    fn settings_for_key(&self, key: &str) -> Result<Vec<(String, String)>, DomainError> {
        let conn = self.lock()?;
        let mut stmt = conn
            .prepare("SELECT asset_id, value_json FROM user_settings WHERE key = ?1")
            .map_err(|e| DomainError::StorageError(format!("prepare query: {e}")))?;
        let rows = stmt
            .query_map(params![key], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
            })
            .map_err(|e| DomainError::StorageError(format!("query settings: {e}")))?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row.map_err(|e| DomainError::StorageError(format!("read row: {e}")))?);
        }
        Ok(out)
    }
}
