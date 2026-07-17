# Proposal: ven-notification-dedup-viewer

## Why

The VEN notification pipeline (WP4.3/BL-20: `Notifier` → in-memory ring + SSE + SQLite)
surfaces plan warnings, VTN outage edges, and grid emergencies to the resident, but the
UI bell shows only the recent in-memory ring — the persisted history is invisible — and
any producer that fires repeatedly (e.g. a storage failure on every planner cycle) would
flood the feed with identical rows. Both gaps block wiring more error boundaries (per the
new `docs/guidelines/ERROR_HANDLING.md` per-audience rule) into the feed: without
deduplication, more producers means alarm fatigue; without a history view, persisted
notifications serve no user.

## What Changes

- `Notifier::notify` gains an optional `dedup_key`; repeated notifications with the same
  key inside a rolling 30-minute window update the existing notification's `count` and
  `last_seen_at` (ring entry updated in place, SSE re-emits the updated row) instead of
  creating a new row.
- SQLite `notifications` table migration: new `count` (default 1) and `last_seen_at`
  columns.
- New error-boundary producer: history-store `StorageError` failures notify at `ALERT`
  severity with `dedup_key = "storage-error"`. Existing edge-triggered producers
  (VTN outage, plan-warning diff) are unchanged — edge triggering remains the preferred
  dedup where a boolean condition exists.
- New endpoint `GET /notifications/history?since=&limit=&severity=` served from the
  existing SQLite store (severity filter added to the store query).
- New VEN UI page **Notifications**: full persisted list, severity filter, dedup
  rendering as `message ×N` with first-seen/last-seen timestamps; bell popover gains a
  "view all" link.

## Capabilities

### New Capabilities
- `notification-dedup`: dedup-key + rolling-window semantics in the application-layer
  `Notifier`, the count/last_seen_at persistence, and the StorageError boundary producer.
- `notification-history-viewer`: the history HTTP endpoint (filtering, paging) and the
  VEN UI Notifications page including ×N dedup rendering.

### Modified Capabilities
<!-- none: domain-errors requirements are unchanged; DomainError variants and their
     boundary mapping stay as specified. -->

## Non-goals

- No mirroring of `tracing` logs into the UI — docker logs stay the exhaustive record;
  the UI feed stays curated (per ERROR_HANDLING.md audience rule).
- No VTN/BFF or VTN UI error viewer.
- No change to edge-triggered producers (`outage_transition`, `new_plan_warnings`).
- No notification acknowledgement/read-state or retention policy (existing table growth
  is acceptable at lab scale).
- No new `DomainError` variants.

## Impact

- **Affected service**: VEN only (containers `ven-ven-1..3-1`) and VEN UI (`ven-ui-1`).
  VTN, BFF, VTN UI untouched. **No openleadr-rs change required.** No OpenADR 3.1 spec
  constraints apply — this is VEN-internal UX.
- **Code**: `VEN/src/services/notify.rs` (dedup logic), `VEN/src/state/mod.rs` (ring
  update-in-place), `VEN/src/history_store/notifications.rs` + migration (schema, upsert,
  severity filter), `VEN/src/routes/notifications.rs` (history route),
  `VEN/src/entities/notification.rs` (`count`, `last_seen_at` fields),
  `VEN/ui/src` (new page, nav route, bell link, hooks).
- **API**: additive — new endpoint; existing `GET /notifications` and SSE payload gain
  two fields (`count`, `last_seen_at`), backwards-compatible for the UI.
- **Determinism**: dedup window uses the already-injected `now` parameter; no wall-clock
  coupling (CLAUDE.md determinism rule).
- **Tests**: unit (window logic), use-case (Notifier with MockHistoryPort), adapter
  (migration + upsert + filter), UI (page + ×N rendering), one E2E BDD scenario
  ("repeated storage failures appear once with a count").
