# notification-history-viewer Specification

## Purpose
The persisted-notification history surface: `GET /notifications/history` over the
SQLite store (beyond the in-memory ring) and the VEN UI Notifications page with
severity filtering and dedup-aware ×N rendering, linked from the bell popover.
Introduced by change ven-notification-dedup-viewer (030).

## Requirements

### Requirement: VEN serves persisted notification history over HTTP
The VEN SHALL expose `GET /notifications/history` accepting optional query
parameters `since` (RFC3339 — only notifications with `last_seen_at` strictly
after it), `limit` (default 200), and `severity` (`INFO` | `WARN` | `ALERT`).
It SHALL return the newest `limit` matching rows from the SQLite store, ordered
oldest-first, as JSON `UserNotification` rows including `count`, `last_seen_at`,
and `dedup_key`. Wire field names and severity values SHALL match the existing
notification vocabulary (SCREAMING_SNAKE severities).

#### Scenario: History returns persisted rows beyond the in-memory ring
- **GIVEN** more notifications persisted than the in-memory ring capacity
- **WHEN** a client calls `GET /notifications/history?limit=500`
- **THEN** rows older than the ring's oldest entry are included

#### Scenario: Severity filter returns only matching rows
- **GIVEN** persisted notifications of severities INFO, WARN, and ALERT
- **WHEN** a client calls `GET /notifications/history?severity=ALERT`
- **THEN** every returned row has severity ALERT

#### Scenario: Invalid severity value is rejected
- **WHEN** a client calls `GET /notifications/history?severity=BOGUS`
- **THEN** the response status is 400 Bad Request with a JSON error message

#### Scenario: Limit keeps the newest rows
- **GIVEN** 300 persisted notifications
- **WHEN** a client calls `GET /notifications/history?limit=100`
- **THEN** the 100 newest rows are returned, ordered oldest-first

### Requirement: VEN UI provides a Notifications page over the history endpoint
The VEN UI SHALL provide a `/notifications` route with a nav entry showing the
persisted history: severity filter, timestamps, and dedup-aware rendering — a row
with `count` > 1 SHALL display the count (e.g. `message ×17`) together with its
first-seen (`created_at`) and last-seen (`last_seen_at`) times. The bell popover
SHALL link to this page.

#### Scenario: Deduplicated notification renders with its count
- **GIVEN** a notification with `count` 17
- **WHEN** the Notifications page renders it
- **THEN** the row shows the message with "×17" and both first-seen and last-seen timestamps

#### Scenario: Single notification renders without a count marker
- **GIVEN** a notification with `count` 1
- **WHEN** the Notifications page renders it
- **THEN** no "×N" marker is shown

#### Scenario: Severity filter narrows the list
- **GIVEN** the page shows INFO, WARN, and ALERT notifications
- **WHEN** the user selects the ALERT filter
- **THEN** only ALERT rows remain visible

#### Scenario: Bell links to the full history
- **WHEN** the user opens the notifications bell popover
- **THEN** a "view all" link navigates to the Notifications page

### Requirement: Live updates reconcile deduplicated rows by id
UI consumers of the live notification feed SHALL treat an incoming notification
whose `id` already exists in the displayed list as an update (replacing count and
last_seen_at) rather than appending a duplicate entry. The current UI consumes
the feed by polling, where this holds by wholesale refetch; the backend
re-broadcasts updated rows on SSE so future stream consumers can reconcile by id.

#### Scenario: A feed update with a known id updates the existing row
- **GIVEN** the bell feed shows a notification with `count` 3
- **WHEN** the feed delivers a row with the same `id` and `count` 4
- **THEN** the existing entry shows `count` 4 and the list length is unchanged
