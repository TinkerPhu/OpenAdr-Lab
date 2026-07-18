## Why

VEN-operational failures (VTN unreachable, storage write errors, background task
panics/restarts) currently only reach `tracing::error!`/`warn!` log lines —
invisible to anyone not tailing server logs. The resident-facing Notifications
feed (`services/notify.rs`) already carries a VTN-reachability producer, but it's
throttled/deduplicated for human consumption and not a diagnostic trail. WP-T4 of
`docs/plans/ven-ui-transparency.md`; per the plan doc's resolved §5 Q1, this is
built as **Option A: fully separate** from Notifications — a different store,
route, and page, not a shared buffer with a `category` filter.

## What Changes

- Add `EventLogEntry { id, created_at, category, message }` and a bounded
  in-memory ring (`AppState`, new `state/event_log.rs` submodule) + broadcast
  channel for live updates — independent of `state.notifications`.
- Producer calls at the sites that already exist from WP-T1/WP-T3: the poll loop's
  failure path (`tasks/backoff.rs::record_fail_sleep`), `state_persist.rs`'s two
  failure branches, and `supervised_spawn`'s completion branches (panic or
  unexpected exit).
- New routes `GET /events/log` (current ring snapshot) and `GET /events/log/events`
  (SSE live stream, mirroring the existing `/notifications/events` bridge pattern).
- UI: new Event Log page, separate badge/count from Notifications.

## Capabilities

### New Capabilities
- `event-log`: VEN-operational events (connectivity failures, storage errors, task
  panics) are recorded to a dedicated, bounded log distinct from resident-facing
  notifications, viewable as a snapshot and streamed live.

### Modified Capabilities
(none — `notification-dedup`/`notification-history-viewer` capabilities are
untouched; this is a parallel mechanism, not a change to Notifications' behavior)

## Impact

- **VEN** (Rust): `state/event_log.rs` (new), `state/mod.rs` (two field lines,
  within existing headroom), `tasks/backoff.rs` (`record_fail_sleep` gains one
  sibling call), `tasks/state_persist.rs` (two failure branches gain one call
  each), `tasks/mod.rs` (`supervised_spawn`'s two completion branches gain one call
  each), `routes/event_log.rs` (new), `routes/mod.rs` (two new routes).
- **VEN UI**: new Event Log page; `api/types.ts`/`api/client.ts` additions.
- **Non-goals**: **no persistence** — in-memory ring only for this WP. A
  persistent store would need a new `HistoryPort` trait method + SQLite migration;
  these are diagnostic notices (unlike personal notifications), so losing them on
  restart is an acceptable trade against that cost for a first cut. If this proves
  insufficient in practice, persistence is a contained follow-up (same shape as
  `HistoryPort::append_notification`). No dedup — every occurrence is
  diagnostically meaningful (matches design.md D1 from the plan-level discussion);
  bounded only by ring capacity. No `poll_programs.rs`/`poll_reports.rs` producer
  wiring — neither uses the `tasks/backoff.rs` helpers WP-T1 introduced (they keep
  their own local, unwired `Backoff` instances), so there is no existing call site
  to hook without new plumbing beyond this WP's scope.
