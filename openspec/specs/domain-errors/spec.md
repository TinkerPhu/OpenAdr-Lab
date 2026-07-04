# domain-errors Specification

## Purpose
TBD - created by archiving change fix-tech-debt-gaps. Update Purpose after archive.
## Requirements
### Requirement: Service layer exposes typed domain errors at key boundaries
The `entities` module SHALL export a `DomainError` enum. Service functions that can fail in domain-meaningful ways SHALL return `Result<T, DomainError>` instead of `anyhow::Result<T>`. Route handlers SHALL map `DomainError` variants to appropriate HTTP status codes.

#### Scenario: Session conflict returns 409
- **WHEN** a route handler receives `Err(DomainError::SessionConflict(_))` from a service
- **THEN** the HTTP response status is 409 Conflict
- **AND** the response body contains a JSON `{"error": "<message>"}` field

#### Scenario: Not found returns 404
- **WHEN** a route handler receives `Err(DomainError::NotFound { id })` from a service
- **THEN** the HTTP response status is 404 Not Found
- **AND** the response body contains the missing resource id

#### Scenario: Plan infeasibility is distinguishable from VTN errors
- **WHEN** the planning service returns `Err(DomainError::PlanInfeasible(_))`
- **THEN** the caller can log the infeasibility reason separately from network errors
- **AND** the last valid plan is retained (no plan adoption on infeasibility)

### Requirement: DomainError variants cover the key failure modes
The `DomainError` enum SHALL include at minimum:
- `SessionConflict(String)` — an EV or heater session is already active
- `NotFound { id: Uuid }` — a resource (user request, session) does not exist
- `PlanInfeasible(String)` — the MILP solver returned no feasible solution
- `VtnUnreachable(String)` — the VTN HTTP endpoint could not be reached
- `ProfileInvalid(String)` — a profile constraint was violated at runtime

#### Scenario: All variants have human-readable Display output
- **WHEN** any `DomainError` variant is formatted with `{}` (Display)
- **THEN** the output is a non-empty, human-readable string describing the error

### Requirement: Existing anyhow usage is not required to change
Services and helpers that do not need domain discrimination SHALL continue using `anyhow::Result`. The introduction of `DomainError` SHALL be additive and SHALL NOT require a wholesale replacement of `anyhow` across the codebase.

#### Scenario: Internal helpers keep anyhow
- **WHEN** a private helper function in `simulator/` returns `anyhow::Result`
- **THEN** it is not required to be changed to `DomainError`
- **AND** the build compiles without error

