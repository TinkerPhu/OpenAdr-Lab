## ADDED Requirements

### Requirement: Background tasks restart automatically after a panic
A single `supervised_spawn` utility function SHALL provide restart behaviour for all seven VEN background tasks (sim_tick, planning, poll_events, poll_programs, poll_reports, obligation, state_persist). The restart loop, error logging, and cooldown SHALL be implemented once inside this utility — not repeated at each call site. The VEN process SHALL NOT exit when a single task panics.

#### Scenario: Task restarts after panic
- **WHEN** a background task panics during execution
- **THEN** the error is logged at ERROR level with the task name and panic message
- **AND** the supervisor waits 5 seconds
- **AND** the task is re-spawned and resumes normal operation

#### Scenario: Process remains alive during task restart
- **WHEN** a background task panics
- **THEN** the VEN HTTP server continues serving requests during the cooldown
- **AND** the GET /health endpoint returns 200 OK throughout

#### Scenario: Repeated panics are logged continuously
- **WHEN** a background task panics on every restart attempt
- **THEN** each restart attempt is logged at ERROR level
- **AND** the supervisor does not give up or exit the process

### Requirement: Supervisor cooldown prevents tight restart loops
The supervisor SHALL wait at least 5 seconds between a task's panic and its restart. This cooldown SHALL be the same for all tasks.

#### Scenario: Cooldown is observed
- **WHEN** a background task panics
- **THEN** the task is not re-spawned for at least 5 seconds after the panic
