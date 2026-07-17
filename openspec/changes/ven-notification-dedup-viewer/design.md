# Design: ven-notification-dedup-viewer

## Context

The VEN notification pipeline (WP4.3/BL-20) is application-layer code in
`VEN/src/services/notify.rs`. `Notifier::notify(state, now, severity, message,
asset_id, event_id)` fans one `UserNotification` out to three consumers:

1. in-memory ring on `AppState` (cap `NOTIFICATION_RING_CAP`) → `GET /notifications`
2. `tokio::broadcast` → SSE at `GET /notifications/events`
3. SQLite `notifications` table via `HistoryPort::append_notification`
   (`history_store/notifications.rs`), survives restarts

The UI (`NotificationsBell.tsx`) reads only the ring. Producers are edge-triggered
(`outage_transition`, `new_plan_warnings`) or one-shot (grid emergency, once per
window). There is no generic protection against a producer that fires every planner
cycle, and the persisted history has no consumer.

Constraints: hexagonal ring rules (services depend on `HistoryPort`, never on the
store), injectable clock (determinism rule), 500-production-line file cap,
test-first, `docs/guidelines/ERROR_HANDLING.md` audience rule (UI feed curated,
tracing/docker logs exhaustive).

## Goals / Non-Goals

**Goals:**
- Generic, opt-in deduplication in `Notifier` so new error-boundary producers can be
  added without alarm fatigue.
- Make the persisted notification history visible in the VEN UI with severity
  filtering and dedup-aware rendering.
- First deduplicated producer: history-store `StorageError` boundary.

**Non-Goals:**
- Mirroring `tracing` output into the UI (docker logs remain the exhaustive record).
- VTN/BFF/VTN-UI error surfacing.
- Read-state/acknowledgement, retention/pruning, or changes to the existing
  edge-triggered producers.

## Data flow

```
producer (task/service)
   └─ Notifier::notify(now, sev, msg, dedup_key, …)
        ├─ dedup: key seen within window? ──yes──► update count/last_seen_at
        │                                          (ring in place, SSE re-emit,
        │                                           SQLite UPDATE via HistoryPort)
        │                                 └─no───► new UserNotification
        │                                          (ring push, SSE, SQLite INSERT)
        ├─ AppState ring ──► GET /notifications ──────────► NotificationsBell (UI)
        ├─ broadcast ─────► GET /notifications/events (SSE) ► live update (UI)
        └─ HistoryPort ───► SQLite ► GET /notifications/history ► Notifications page (UI)
```

## Decisions

### D1 — Rolling window keyed dedup, not condition-tracking
`notify` gains `dedup_key: Option<String>`. If a notification with the same key has
`last_seen_at` within `DEDUP_WINDOW` (30 min) it is updated (`count += 1`,
`last_seen_at = now`) instead of inserted.
*Alternative considered*: "dedup while condition active" — rejected because only the
producer can know when a condition clears, which is exactly the edge-trigger pattern
that already exists; producers with a boolean condition keep using edges.
`dedup_key = None` preserves today's behaviour exactly.

### D2 — Dedup state lives in the ring, not a separate map
The lookup "same key within window" scans the in-memory ring (newest-first, bounded
at `NOTIFICATION_RING_CAP`), so `UserNotification` gains `dedup_key: Option<String>`,
`count: u32`, `last_seen_at: DateTime<Utc>`. No second data structure to keep
consistent; after restart the ring is seeded from SQLite as today, so dedup state
survives restarts for free. *Alternative*: `HashMap<String, (Uuid, DateTime)>` on
`Notifier` — rejected: duplicate state, lost on restart, needs its own eviction.

### D3 — Schema: additive columns + UPDATE, no upsert-by-key
Migration adds `dedup_key TEXT NULL`, `count INTEGER NOT NULL DEFAULT 1`,
`last_seen_at INTEGER NOT NULL DEFAULT created_at-value` (backfilled from
`created_at`). The dedup decision is made in `Notifier` (application layer);
the store only gets a new `HistoryPort::update_notification_seen(id, count,
last_seen_at)` method plus the widened row. *Alternative*: SQL `ON CONFLICT`
upsert keyed on `dedup_key` — rejected: pushes the window policy into the adapter,
and `dedup_key` is not unique across windows.

### D4 — SSE re-emits the updated row; UI reconciles by `id`
On a dedup hit the existing notification (same `id`, higher `count`) is re-broadcast.
The UI updates a list entry when an incoming SSE row's `id` already exists. This keeps
the SSE contract "stream of `UserNotification` JSON" unchanged — additive fields only.

### D5 — History endpoint reuses the store query
`GET /notifications/history?since=<RFC3339>&limit=<n>&severity=<INFO|WARN|ALERT>`
maps onto `history_store/notifications.rs::query()` extended with an optional
severity filter; default `limit` 200, newest-`limit` rows returned oldest-first (the
existing convention). Wire severity names stay SCREAMING_SNAKE (one vocabulary rule).

Example response row:
```json
{
  "id": "0e0f…", "created_at": "2026-07-17T09:12:00Z",
  "last_seen_at": "2026-07-17T09:41:00Z", "count": 17,
  "severity": "ALERT", "message": "storage error: insert notification: disk full",
  "asset_id": null, "event_id": null, "dedup_key": "storage-error"
}
```

### D6 — StorageError producer placement
The first keyed producer fires where history-store writes fail on hot paths (e.g.
the planner-cycle samplers), notifying `ALERT` with `dedup_key = "storage-error"`.
The `warn!` inside `Notifier::notify`'s own persist failure stays a log-only path —
notifying about a failure to persist notifications would recurse.

### D7 — UI: new page, bell unchanged except a link
New route `/notifications` and nav entry, page `Notifications.tsx` following the
`History.tsx` pattern: fetch via a `useNotificationHistory(severity?)` hook, severity
filter chips, rows rendered as `message ×17` (count > 1) with created/last-seen
timestamps. The bell popover gains a "view all" link. No MUI DataGrid — plain `List`
like the bell, consistent with REACT_GUIDELINES.

## Risks / Trade-offs

- [Ring cap smaller than window traffic → dedup miss → duplicate rows] → acceptable:
  worst case is today's behaviour; cap (64) × 30 min makes this unlikely at lab scale.
- [Message changes between repeats (different context in the string) → count hides
  the newest message] → dedup hit updates `count`/`last_seen_at` only; the stored
  message is the first occurrence. Documented in the spec; producers that need the
  newest context should put stable text in the message and variance in the log line.
- [Schema migration on existing Pi4 volumes] → additive `ALTER TABLE … ADD COLUMN`
  with defaults, applied by the existing migration mechanism at startup; rollback =
  old binary ignores the extra columns (SQLite tolerates them).
- [SSE consumers assuming append-only stream] → only consumer is the VEN UI, updated
  in the same change.
- [tasks/ file-size cap] → dedup logic lives in `services/notify.rs` (currently ~330
  lines incl. tests, well under cap); no tasks/ file grows.

## Migration Plan

1. Merge → deploy VEN images (`docker compose build ven && up -d ven-…`) — migration
   runs at startup, additive only.
2. VEN UI rebuild (`ven-ui-1`).
3. Rollback: redeploy previous images; extra columns are inert.

## Open Questions

- None blocking. `DEDUP_WINDOW = 30 min` is a constant for now; make it a profile
  value only if a real need appears (avoid speculative config).
