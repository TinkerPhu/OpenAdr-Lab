---
title: Notification Feed
type: component
created: 2026-07-12
updated: 2026-07-12
synced_commit: c5a1d03
sources: [VEN/src/services/notify.rs, VEN/src/entities/notification.rs, VEN/src/routes/notifications.rs, VEN/src/history_store/notifications.rs, VEN/ui/src/components/NotificationsBell.tsx, VEN/src/tasks/poll_signals.rs, VEN/src/tasks/poll_events.rs]
tags: [notifications, ux, phase4]
---

# Notification Feed

Phase 4's WP4.3 (`docs/BACKLOG.md` BL-20): the first user-facing surface for
"things the resident should know" — grid emergencies, VTN outages, plan
warnings. Before this, `UserNotificationSeverity` existed only as vocabulary
(`entities/design_vocabulary.rs`).

## Pipeline

One `UserNotification` (`entities/notification.rs`: severity from the existing
enum, message, optional asset/event refs) fans out from the application-layer
`Notifier` (`services/notify.rs`) to three consumers:

1. **In-memory ring** on `AppState` (cap 200, mirrors the `/trace/events`
   pattern) — serves `GET /notifications?since=`.
2. **SSE broadcast** — `GET /notifications/events` bridges a tokio broadcast
   to SSE exactly like `/plan/events` (lagged clients never poison the sender).
3. **History store** — `notifications` table (schema v2, [[history-store]]);
   the ring is re-seeded from it at startup so the feed survives restarts.

Producers depend on the `Notifier` service only — inner rings gain no outward
deps ([[ven-hexagonal-architecture]]).

## Producers (edge-triggered, never per-poll)

| Trigger | Severity | Where |
|---|---|---|
| Newly-appearing grid-alert window | Alert | `tasks/poll_signals.rs` (once per window) |
| VTN reachable → unreachable / back | Warn / Info | `tasks/poll_events.rs` via `notify::outage_transition` |
| New warning on an **adopted** plan | Warning→Warn, Critical→Alert, Info suppressed | `services/notify.rs::notify_new_plan_warnings` (called from the plan cycle) |

The plan-warning channel is the backbone: WP4.4's stale-rate warning and
WP4.1-c's MAX_COST budget warning ride it without their own producer wiring.
**Dedup contract:** producers must use *stable* warning text — the diff against
the previous plan's messages is what makes each condition notify exactly once
([[milp-planner]]).

Producers named in the enum's doc comments but *not yet* wired: tier fallback,
deadline-at-risk, packet abandoned — they belong to the Stage-5 tier machinery
that doesn't exist yet (BL-09-adjacent).

## UI

`NotificationsBell` (VEN/ui, app bar next to the health chip): badge count +
popover feed, newest first, severity chips; polls every 10 s ([[ven-ui]]).
