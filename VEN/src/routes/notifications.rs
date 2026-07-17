//! WP4.3 (BL-20) — notification feed routes.
//!
//! `GET /notifications?since=<RFC3339>` — ring contents (seeded from the
//! history store at startup), oldest first.
//! `GET /notifications/events` — SSE stream of new notifications
//! (mirrors `/plan/events`).
//! 030: `GET /notifications/history?since=&limit=&severity=` — persisted
//! history beyond the ring, with optional severity filter.

use std::sync::Arc;

use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::sse::{Event, KeepAlive, Sse},
    response::IntoResponse,
    Json,
};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use tokio_stream::wrappers::ReceiverStream;
use tokio_stream::StreamExt;

use crate::controller::HistoryPort;
use crate::entities::design_vocabulary::UserNotificationSeverity;
use crate::entities::notification::UserNotification;
use crate::AppCtx;

#[derive(Deserialize)]
pub struct NotificationsQuery {
    /// Return only notifications created strictly after this timestamp.
    pub since: Option<DateTime<Utc>>,
}

/// GET /notifications — the in-memory notification ring, oldest first.
pub async fn get_notifications(
    State(ctx): State<AppCtx>,
    Query(q): Query<NotificationsQuery>,
) -> Json<Vec<crate::entities::notification::UserNotification>> {
    Json(ctx.state.notifications_since(q.since).await)
}

/// 030: query parameters for the persisted-history endpoint.
#[derive(Deserialize)]
pub struct NotificationsHistoryQuery {
    /// Only notifications last seen strictly after this timestamp.
    pub since: Option<DateTime<Utc>>,
    /// Newest N matching rows (returned oldest first). Default 200.
    pub limit: Option<usize>,
    /// Wire severity name: INFO | WARN | ALERT.
    pub severity: Option<String>,
}

const HISTORY_DEFAULT_LIMIT: usize = 200;

fn parse_severity(s: &str) -> Option<UserNotificationSeverity> {
    match s {
        "INFO" => Some(UserNotificationSeverity::Info),
        "WARN" => Some(UserNotificationSeverity::Warn),
        "ALERT" => Some(UserNotificationSeverity::Alert),
        _ => None,
    }
}

/// Handler core, separated from the axum wrapper so it is testable against
/// a mock `HistoryPort` (the adapter-contract layer). `None` history (store
/// disabled or failed to open) serves an empty list rather than an error.
async fn notifications_history(
    history: Option<Arc<dyn HistoryPort>>,
    q: NotificationsHistoryQuery,
) -> Result<Vec<UserNotification>, (StatusCode, Json<serde_json::Value>)> {
    let severity = match q.severity.as_deref() {
        None => None,
        Some(s) => Some(parse_severity(s).ok_or_else(|| {
            (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "error": format!("invalid severity: {s} (expected INFO, WARN or ALERT)")
                })),
            )
        })?),
    };
    let Some(h) = history else {
        return Ok(Vec::new());
    };
    let (since, limit) = (q.since, q.limit.unwrap_or(HISTORY_DEFAULT_LIMIT));
    tokio::task::spawn_blocking(move || h.query_notifications(since, limit, severity))
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": format!("query task panicked: {e}") })),
            )
        })?
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": e.to_string() })),
            )
        })
}

/// GET /notifications/history — persisted notification history (030).
pub async fn get_notifications_history(
    State(ctx): State<AppCtx>,
    Query(q): Query<NotificationsHistoryQuery>,
) -> axum::response::Response {
    match notifications_history(ctx.history.clone(), q).await {
        Ok(rows) => Json(rows).into_response(),
        Err(err) => err.into_response(),
    }
}

/// GET /notifications/events — Server-Sent Events stream of new notifications.
pub async fn get_notification_events(
    State(ctx): State<AppCtx>,
) -> Sse<impl tokio_stream::Stream<Item = Result<Event, std::convert::Infallible>>> {
    let mut bcast_rx = ctx.notifier.subscribe();
    // Bridge broadcast → mpsc so lagged clients don't poison the broadcast sender.
    let (fwd_tx, fwd_rx) = tokio::sync::mpsc::channel::<Event>(32);
    tokio::spawn(async move {
        loop {
            match bcast_rx.recv().await {
                Ok(n) => {
                    if let Ok(data) = serde_json::to_string(&n) {
                        if fwd_tx.send(Event::default().data(data)).await.is_err() {
                            break; // client disconnected
                        }
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    });
    let stream = ReceiverStream::new(fwd_rx).map(Ok::<_, std::convert::Infallible>);
    Sse::new(stream).keep_alive(KeepAlive::default())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::test_support::mock_history_port::MockHistoryPort;
    use chrono::TimeZone;

    fn ts(secs: i64) -> DateTime<Utc> {
        Utc.timestamp_opt(secs, 0).unwrap()
    }

    fn seed(mock: &MockHistoryPort, n: usize, severity: UserNotificationSeverity) {
        for i in 0..n {
            mock.append_notification(&UserNotification::new(
                ts(i as i64 * 60),
                severity.clone(),
                format!("{severity:?} #{i}"),
                None,
                None,
            ))
            .unwrap();
        }
    }

    fn query(
        since: Option<DateTime<Utc>>,
        limit: Option<usize>,
        severity: Option<&str>,
    ) -> NotificationsHistoryQuery {
        NotificationsHistoryQuery {
            since,
            limit,
            severity: severity.map(String::from),
        }
    }

    #[tokio::test]
    async fn notifications_history_returns_rows_beyond_ring_cap() {
        let mock = Arc::new(MockHistoryPort::new());
        seed(
            &mock,
            crate::state::NOTIFICATION_RING_CAP + 6,
            UserNotificationSeverity::Info,
        );
        let rows = notifications_history(Some(mock), query(None, Some(500), None))
            .await
            .unwrap();
        assert_eq!(
            rows.len(),
            crate::state::NOTIFICATION_RING_CAP + 6,
            "history serves more than the ring holds"
        );
    }

    #[tokio::test]
    async fn notifications_history_severity_filter_matches_only() {
        let mock = Arc::new(MockHistoryPort::new());
        seed(&mock, 3, UserNotificationSeverity::Info);
        seed(&mock, 2, UserNotificationSeverity::Alert);
        let rows = notifications_history(Some(mock), query(None, None, Some("ALERT")))
            .await
            .unwrap();
        assert_eq!(rows.len(), 2);
        assert!(rows
            .iter()
            .all(|n| n.severity == UserNotificationSeverity::Alert));
    }

    #[tokio::test]
    async fn notifications_history_invalid_severity_is_400() {
        let mock = Arc::new(MockHistoryPort::new());
        let err = notifications_history(Some(mock), query(None, None, Some("BOGUS")))
            .await
            .unwrap_err();
        assert_eq!(err.0, StatusCode::BAD_REQUEST);
        assert!(err.1 .0["error"]
            .as_str()
            .unwrap()
            .contains("invalid severity"));
    }

    #[tokio::test]
    async fn notifications_history_limit_keeps_newest_oldest_first() {
        let mock = Arc::new(MockHistoryPort::new());
        seed(&mock, 300, UserNotificationSeverity::Info);
        let rows = notifications_history(Some(mock), query(None, Some(100), None))
            .await
            .unwrap();
        assert_eq!(rows.len(), 100);
        assert_eq!(rows[0].message, "Info #200", "newest 100, oldest first");
        assert_eq!(rows[99].message, "Info #299");
    }

    #[tokio::test]
    async fn notifications_history_without_store_is_empty() {
        let rows = notifications_history(None, query(None, None, None))
            .await
            .unwrap();
        assert!(rows.is_empty());
    }
}
