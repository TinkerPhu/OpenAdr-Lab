## ADDED Requirements

### Requirement: Background events are suspended for the run and restored afterwards
The harness SHALL save every VTN event it did not create to `background-events.json` in the run
directory before the first window, SHALL delete those events for the duration of the run, and
SHALL re-create them after the run ends, including when the run fails.

#### Scenario: Demo events do not reach the VENs during a run
- **WHEN** a run starts while demo events exist on the VTN
- **THEN** those events are saved to disk and deleted before any scenario window starts, and exist again (re-created from the saved bodies) after the run

#### Scenario: Recovery after a hard kill
- **WHEN** the harness was killed between deletion and restoration
- **THEN** `--restore-background-events <file>` re-creates every event in the saved snapshot

#### Scenario: Opting out
- **WHEN** the harness is started with `--keep-background-events`
- **THEN** no VTN event is saved, deleted or re-created

### Requirement: The scenario price must reach every VEN before the run proceeds
After posting the first `price_series` action, the harness SHALL verify that every VEN's resolved
tariff covering the current instant equals the scenario's first price, and SHALL abort the run
with cleanup and a non-zero exit when any VEN does not show it within the timeout.

#### Scenario: Price overridden on one VEN
- **WHEN** one VEN still resolves a different import price 180 s after the first price action
- **THEN** the harness deletes its created events, program and user requests, restores background events, names the VEN, and exits non-zero without writing a scenario result

### Requirement: The action log carries each action's parameters
The harness SHALL record, for each posted action, its scenario parameters (limits, durations,
levels, setpoints, price values) together with `at_minute`, `type` and `started_at` in
`run.json["actions"]`.

#### Scenario: Capacity limit logged with its value and duration
- **WHEN** a `capacity_limit` action with `import_kw: 1.5` and `duration_minutes: 20` is posted
- **THEN** its `run.json` action entry contains `import_kw` 1.5, `duration_minutes` 20 and its wall-clock `started_at`

### Requirement: Adopted plans are retained per VEN
While polling plans, the harness SHALL append each newly seen plan (by id) with its per-slot
asset allocations to `{ven}-plans.jsonl`, and SHALL NOT append a plan it already recorded.

#### Scenario: Plan changes once during a window
- **WHEN** a VEN's `GET /plan` returns plan A for several polls and then plan B
- **THEN** `{ven}-plans.jsonl` contains exactly two records, A then B, each with slot start, end and per-asset power

### Requirement: Long waits survive host suspend
The harness SHALL implement every wait for a wall-clock instant as short sleeps re-checked
against the wall clock, so a suspended and resumed host continues at the correct instant instead
of oversleeping.

#### Scenario: Clock jumps forward during a wait
- **WHEN** the wall clock passes the target instant while the process was suspended
- **THEN** the wait returns within one chunk (at most 60 s) of the process resuming
