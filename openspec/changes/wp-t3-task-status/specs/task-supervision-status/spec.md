## ADDED Requirements

### Requirement: Each supervised task's status is queryable
`GET /tasks/status` SHALL return an array of `{name, last_run_ts, last_success,
restart_count}`, one entry per background task that has actually been spawned via
`supervised_spawn` in this running process — not a fixed list independent of
configuration.

#### Scenario: A freshly started task appears with restart_count zero
- **WHEN** a task is spawned for the first time and has not yet panicked
- **THEN** its entry in `GET /tasks/status` has `restart_count: 0` and
  `last_success: null`
- **AND** `last_run_ts` reflects the time it was (re)spawned

#### Scenario: A panicked-and-restarted task's restart_count increments
- **WHEN** a task's wrapped future panics and `supervised_spawn` respawns it
- **THEN** its entry's `restart_count` increases by exactly 1
- **AND** its `last_success` becomes `false`

#### Scenario: Conditionally-spawned tasks are absent when not configured
- **WHEN** a task that is only spawned under certain configuration (e.g.
  `state_persist` without a configured persist path) was never spawned in this
  process
- **THEN** it does not appear in `GET /tasks/status`'s response at all

### Requirement: Task-status recording does not change restart behavior
Recording `last_run_ts`/`last_success`/`restart_count` SHALL be purely additive to
`supervised_spawn`'s existing panic-restart behavior (per the `task-supervisor`
capability) — the cooldown duration, restart triggering, and logging are
unaffected.

#### Scenario: Cooldown timing is unchanged
- **WHEN** a task panics
- **THEN** the supervisor still waits the same cooldown duration before
  respawning, identical to behavior before task-status recording was added
