//! WP-T4 (`docs/history/project_journal.md, search "WP-T"`) — Event Log routes.
//! Deliberately separate from `routes/notifications.rs`: different ring,
//! different broadcast channel, no shared route or dedup (see
//! `openspec/changes/wp-t4-event-log/design.md`).

use axum::extract::State;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::Json;
use tokio_stream::wrappers::ReceiverStream;
use tokio_stream::StreamExt;

use crate::state::EventLogEntry;
use crate::AppCtx;

/// GET /events/log — the in-memory event log ring, oldest first.
pub async fn get_event_log(State(ctx): State<AppCtx>) -> Json<Vec<EventLogEntry>> {
    Json(ctx.state.event_log_snapshot().await)
}

/// GET /events/log/events — Server-Sent Events stream of new event log entries.
/// Mirrors `routes/notifications.rs::get_notification_events`'s bridge pattern.
pub async fn get_event_log_events(
    State(ctx): State<AppCtx>,
) -> Sse<impl tokio_stream::Stream<Item = Result<Event, std::convert::Infallible>>> {
    let mut bcast_rx = ctx.state.subscribe_event_log();
    let (fwd_tx, fwd_rx) = tokio::sync::mpsc::channel::<Event>(32);
    tokio::spawn(async move {
        loop {
            match bcast_rx.recv().await {
                Ok(entry) => {
                    if let Ok(data) = serde_json::to_string(&entry) {
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
