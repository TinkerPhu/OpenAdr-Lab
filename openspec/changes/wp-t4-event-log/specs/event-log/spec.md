## ADDED Requirements

### Requirement: VEN-operational failures are recorded to a dedicated event log
Connectivity failures, storage write errors, and background-task panics/exits
SHALL be recorded as `EventLogEntry { id, created_at, category, message }` in a
bounded log independent of the resident-facing Notifications feed — no shared
ring, dedup window, or route with `services/notify.rs`.

#### Scenario: A VTN poll failure is recorded
- **WHEN** the VTN poll loop's attempt fails
- **THEN** an entry with `category: "vtn_connection"` and a message derived from
  the error is appended to the event log
- **AND** this does not create or update any entry in the Notifications feed

#### Scenario: A storage write failure is recorded
- **WHEN** `state_persist`'s write or serialization fails
- **THEN** an entry with `category: "storage"` is appended to the event log

#### Scenario: A supervised task panic is recorded
- **WHEN** a background task's wrapped future panics and `supervised_spawn`
  restarts it
- **THEN** an entry with `category: "task_supervisor"` naming the task is
  appended to the event log

#### Scenario: The log is bounded, not deduplicated
- **WHEN** more entries are recorded than the ring's capacity
- **THEN** the oldest entries are evicted, and no two occurrences of the same
  failure are ever merged into one entry with a count

### Requirement: The event log is queryable as a snapshot and streamable live
`GET /events/log` SHALL return the current ring snapshot. `GET /events/log/events`
SHALL stream new entries via Server-Sent Events as they're recorded.

#### Scenario: Snapshot reflects entries recorded since process start
- **WHEN** `GET /events/log` is called
- **THEN** it returns every entry currently in the ring, oldest state lost only
  once the ring's capacity is exceeded

#### Scenario: SSE stream delivers new entries live
- **WHEN** a client is connected to `GET /events/log/events` and a new entry is
  recorded
- **THEN** the client receives that entry over the SSE stream without polling
