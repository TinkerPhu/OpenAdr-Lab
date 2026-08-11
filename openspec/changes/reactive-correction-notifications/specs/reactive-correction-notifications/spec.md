## ADDED Requirements

### Requirement: Reactive correction start notification
When the deviation arbiter's per-tick active lever transitions from inactive (no lever fired the
previous tick, or no plan existed yet) to active (a lever fired this tick), the system SHALL emit
exactly one `UserNotification` of severity `Info` or `Warn` through the existing `Notifier`
(ring + SSE + persistence), reaching every subscriber regardless of which UI page is mounted.

#### Scenario: Sustained deviation triggers a correction while off the Planner tab
- **WHEN** the arbiter's active lever goes from `None` to `Some(lever)` on a tick
- **THEN** a notification of severity `Info` or `Warn` is pushed to the notification ring, broadcast
  over the notification SSE stream, and persisted to the history store, independent of whether the
  Planner page is mounted

#### Scenario: Continuously active correction does not repeat
- **WHEN** the arbiter's active lever is `Some` on tick N and remains `Some` (the same lever, or a
  different lever handling the same continuing deviation) on tick N+1
- **THEN** no additional notification is emitted for tick N+1

### Requirement: Reactive correction cleared notification
When the deviation arbiter's per-tick active lever transitions from active to inactive, the system
SHALL emit at most one follow-up `UserNotification` marking the correction as cleared.

#### Scenario: Correction clears
- **WHEN** the arbiter's active lever goes from `Some(lever)` to `None` on a tick
- **THEN** exactly one notification marking the correction as cleared is emitted through the same
  `Notifier` path

#### Scenario: No plan yet produces no spurious clear
- **WHEN** the arbiter's active lever is `None` on tick N (no active correction) and remains `None`
  on tick N+1 (including the no-plan-yet startup window)
- **THEN** no "cleared" notification is emitted

### Requirement: Correction notification content is lever-agnostic
The start and cleared notification messages SHALL use fixed, stable text that does not embed the
specific active lever, asset id, or the tick's live deviation magnitude, so dedup keys stay
comparable across occurrences and a lever handoff mid-correction cannot make an already-emitted
message stale.

#### Scenario: Lever handoff during a continuing correction does not change or duplicate the notification
- **WHEN** the arbiter's active lever changes from `Some("battery")` to `Some("heater_pause")`
  without ever passing through `None` in between
- **THEN** no new notification is emitted and the existing "correction active" notification's
  message text is unchanged
