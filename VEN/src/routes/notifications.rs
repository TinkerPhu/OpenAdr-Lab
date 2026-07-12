//! WP4.3 (BL-20) — notification feed routes.
//!
//! `GET /notifications?since=<RFC3339>` — ring contents (seeded from the
//! history store at startup), oldest first.
//! `GET /notifications/events` — SSE stream of new notifications
//! (mirrors `/plan/events`).

use axum::{
    extract::{Query, State},
    response::sse::{Event, KeepAlive, Sse},
    Json,
};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use tokio_stream::wrappers::ReceiverStream;
use tokio_stream::StreamExt;

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
