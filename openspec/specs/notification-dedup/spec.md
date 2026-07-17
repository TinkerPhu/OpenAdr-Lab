# notification-dedup Specification

## Purpose
Deduplication of user notifications in the VEN's application-layer `Notifier`:
keyed repeats inside a rolling window collapse into one notification whose
`count`/`last_seen_at` advance, so repeat-firing error producers (first:
history-store `StorageError`) can feed the resident notification feed without
alarm fatigue. Introduced by change ven-notification-dedup-viewer (030).

## Requirements

### Requirement: Keyed notifications deduplicate within a rolling window
`Notifier::notify` SHALL accept an optional `dedup_key`. When a notification with
the same `dedup_key` exists whose `last_seen_at` is within the last 30 minutes of
the injected `now`, the Notifier SHALL NOT create a new notification; it SHALL
increment that notification's `count`, set its `last_seen_at` to `now`, update the
in-memory ring entry in place, re-broadcast the updated notification, and persist
the updated `count`/`last_seen_at` via `HistoryPort`. The original `message` and
`created_at` SHALL be preserved on a dedup hit.

#### Scenario: Repeated keyed notification within the window collapses to one row
- **GIVEN** a notification with `dedup_key` "storage-error" was emitted 5 minutes ago
- **WHEN** `notify` is called again with `dedup_key` "storage-error"
- **THEN** no new notification is created
- **AND** the existing notification's `count` is 2 and `last_seen_at` equals the injected `now`
- **AND** the updated notification is re-broadcast on the SSE channel

#### Scenario: Keyed notification outside the window creates a new row
- **GIVEN** a notification with `dedup_key` "storage-error" whose `last_seen_at` is 31 minutes before `now`
- **WHEN** `notify` is called with `dedup_key` "storage-error"
- **THEN** a new notification is created with `count` 1

#### Scenario: Notifications without a dedup key are never deduplicated
- **GIVEN** two `notify` calls with identical message and no `dedup_key`, 1 minute apart
- **WHEN** both calls complete
- **THEN** two distinct notifications exist in the ring and the store

#### Scenario: Dedup window uses the injected clock
- **WHEN** dedup-window tests run with a fixed injected `now`
- **THEN** they pass deterministically without sleeping or reading the wall clock

### Requirement: Notification rows persist count and last_seen_at
The SQLite `notifications` table SHALL have `dedup_key` (nullable TEXT), `count`
(INTEGER, default 1), and `last_seen_at` (INTEGER unix seconds) columns (schema
v4, additive migration). Pre-migration rows SHALL be backfilled with `count` 1
and `last_seen_at` equal to `created_at`. Dedup state SHALL survive a VEN
restart via the existing ring-seeding from the store.

#### Scenario: Migration backfills existing rows
- **GIVEN** a pre-migration database containing notifications
- **WHEN** the VEN starts and the migration runs
- **THEN** every existing row has `count` 1 and `last_seen_at` equal to its `created_at`

#### Scenario: Dedup hit survives restart
- **GIVEN** a keyed notification persisted 5 minutes before a VEN restart
- **WHEN** the VEN restarts (ring re-seeded from the store) and `notify` fires the same `dedup_key`
- **THEN** the persisted notification's `count` increments instead of a new row appearing

### Requirement: History-store write failures produce one deduplicated ALERT
When a history-store write on a hot path fails with `DomainError::StorageError`,
the boundary SHALL notify with severity `ALERT` and `dedup_key` "storage-error".
The persist step inside `Notifier::notify` itself SHALL remain log-only (no
recursive notification).

#### Scenario: Repeated storage failures appear once with a count
- **GIVEN** the history store fails on every planner cycle for 20 minutes
- **WHEN** the resident opens the notification feed
- **THEN** exactly one ALERT "storage error" notification is visible with `count` > 1

#### Scenario: Notification persist failure does not notify
- **GIVEN** `HistoryPort::append_notification` fails inside `Notifier::notify`
- **WHEN** the notify call completes
- **THEN** a warning is logged and no additional notification is produced
