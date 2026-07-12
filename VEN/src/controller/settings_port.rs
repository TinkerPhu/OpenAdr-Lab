// WP4.2 (BL-19) — SettingsPort: small key/value-per-asset persistence for
// user preferences (first consumer: comfort-curve overrides). Sibling of
// HistoryPort rather than an extension of it — settings are current-state,
// not time-series history, and keeping the trait separate keeps both
// single-responsibility. The same SQLite adapter implements both.
//
// Blocking by design, like HistoryPort: call through
// `tokio::task::spawn_blocking` from async contexts.
#![allow(dead_code)]
use chrono::{DateTime, Utc};

use crate::entities::DomainError;

pub trait SettingsPort: Send + Sync {
    /// Upsert one setting value (JSON string) for `(key, asset_id)`.
    fn put_setting(
        &self,
        key: &str,
        asset_id: &str,
        value_json: &str,
        updated_at: DateTime<Utc>,
    ) -> Result<(), DomainError>;

    /// Read one setting value, `None` when unset.
    fn get_setting(&self, key: &str, asset_id: &str) -> Result<Option<String>, DomainError>;

    /// Delete one setting; returns whether a row existed.
    fn delete_setting(&self, key: &str, asset_id: &str) -> Result<bool, DomainError>;

    /// All `(asset_id, value_json)` pairs for a key — startup re-seeding.
    fn settings_for_key(&self, key: &str) -> Result<Vec<(String, String)>, DomainError>;
}

/// Well-known settings keys.
pub const SETTING_COMFORT_CURVE: &str = "comfort_curve";
