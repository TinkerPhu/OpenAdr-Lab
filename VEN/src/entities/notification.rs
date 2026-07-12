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
        }
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
}
