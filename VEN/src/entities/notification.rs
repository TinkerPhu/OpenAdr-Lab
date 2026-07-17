//! WP4.3 (BL-20) — user-facing notification: something the resident should
//! see (grid emergency, budget breach, VTN outage, plan warnings), carried
//! by an in-memory ring for the live feed and appended to the history store
//! so the feed survives restarts.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::entities::design_vocabulary::UserNotificationSeverity;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct UserNotification {
    pub id: Uuid,
    pub created_at: DateTime<Utc>,
    pub severity: UserNotificationSeverity,
    pub message: String,
    /// Asset this notification concerns, when applicable (e.g. "ev").
    pub asset_id: Option<String>,
    /// OpenADR event that triggered it, when applicable.
    pub event_id: Option<String>,
    /// Dedup key (030): repeats with the same key inside the rolling window
    /// update `count`/`last_seen_at` instead of creating a new notification.
    #[serde(default)]
    pub dedup_key: Option<String>,
    /// How many times this notification occurred (dedup hits + 1).
    #[serde(default = "default_count")]
    pub count: u32,
    /// When this notification last occurred; equals `created_at` until a
    /// dedup hit bumps it. Message and `created_at` keep the first occurrence.
    pub last_seen_at: DateTime<Utc>,
}

fn default_count() -> u32 {
    1
}

impl UserNotification {
    pub fn new(
        now: DateTime<Utc>,
        severity: UserNotificationSeverity,
        message: impl Into<String>,
        asset_id: Option<String>,
        event_id: Option<String>,
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            created_at: now,
            severity,
            message: message.into(),
            asset_id,
            event_id,
            dedup_key: None,
            count: 1,
            last_seen_at: now,
        }
    }

    /// Builder-style dedup key (030) — keeps `new()` call sites unchanged.
    pub fn with_dedup_key(mut self, key: impl Into<String>) -> Self {
        self.dedup_key = Some(key.into());
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_user_notification_serde_roundtrip_screaming_severity() {
        let n = UserNotification::new(
            Utc::now(),
            UserNotificationSeverity::Alert,
            "grid emergency",
            None,
            Some("evt-1".into()),
        );
        let json = serde_json::to_string(&n).unwrap();
        assert!(json.contains("\"ALERT\""));
        let back: UserNotification = serde_json::from_str(&json).unwrap();
        assert_eq!(back, n);
    }

    #[test]
    fn new_defaults_count_one_and_last_seen_equals_created() {
        let n = UserNotification::new(
            Utc::now(),
            UserNotificationSeverity::Info,
            "plan updated",
            None,
            None,
        );
        assert_eq!(n.count, 1);
        assert_eq!(n.last_seen_at, n.created_at);
        assert_eq!(n.dedup_key, None);
    }

    #[test]
    fn with_dedup_key_sets_key_and_serializes() {
        let n = UserNotification::new(
            Utc::now(),
            UserNotificationSeverity::Alert,
            "storage error",
            None,
            None,
        )
        .with_dedup_key("storage-error");
        assert_eq!(n.dedup_key.as_deref(), Some("storage-error"));
        let json = serde_json::to_string(&n).unwrap();
        assert!(json.contains("\"dedup_key\":\"storage-error\""));
        assert!(json.contains("\"count\":1"));
        assert!(json.contains("\"last_seen_at\""));
    }
}
