## ADDED Requirements

### Requirement: MILP solver timeout is configurable via profile
The `planner:` section of the YAML profile SHALL accept a `solver_timeout_s` field (positive integer, default `60`). This value SHALL be passed to the HiGHS solver as the time limit in `solver_phase1.rs` and `solver_phase2.rs`.

#### Scenario: Custom solver timeout is applied
- **WHEN** the profile sets `planner.solver_timeout_s: 30`
- **THEN** the HiGHS solver is invoked with a 30-second time limit in both Phase 1 and Phase 2

#### Scenario: Default timeout is 60 seconds when field is absent
- **WHEN** the profile does not specify `solver_timeout_s`
- **THEN** the HiGHS solver uses a 60-second time limit (unchanged from previous behaviour)

### Requirement: Planning loop initial delay is configurable via profile
The `planner:` section of the YAML profile SHALL accept a `planning_initial_delay_s` field (non-negative integer, default `5`). The planning loop SHALL sleep this many seconds after startup before its first plan computation.

#### Scenario: Custom initial delay is applied
- **WHEN** the profile sets `planner.planning_initial_delay_s: 10`
- **THEN** the planning loop waits 10 seconds after VEN startup before computing the first plan

#### Scenario: Default initial delay is 5 seconds when field is absent
- **WHEN** the profile does not specify `planning_initial_delay_s`
- **THEN** the planning loop waits 5 seconds before the first plan (unchanged from previous behaviour)

### Requirement: Remaining magic numbers are replaced with named constants
The obligation check interval (5 s) and the OAuth token expiry safety margin (60 s) SHALL be defined as named constants in their respective source files rather than as anonymous integer literals.

#### Scenario: Constants are identifiable in code
- **WHEN** a developer reads `tasks/obligation.rs`
- **THEN** the check interval appears as a named constant `OBLIGATION_CHECK_INTERVAL_S`, not as a bare `5`

#### Scenario: Token margin constant is named
- **WHEN** a developer reads `vtn.rs`
- **THEN** the expiry margin appears as a named constant `TOKEN_EXPIRY_MARGIN_S`, not as a bare `60`
