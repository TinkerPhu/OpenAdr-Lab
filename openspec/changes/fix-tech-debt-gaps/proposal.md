## Why

The VEN backend has three open reliability gaps (no panic recovery, no profile validation, untyped domain errors) and four hardcoded magic numbers identified in the architecture review (`ven_backend_review.md`). These create silent failure modes in production: a panicking task brings down DR control with no restart, an invalid profile produces wrong physics behaviour at runtime, and `anyhow`-only errors make domain-specific recovery (e.g. keep last valid plan on infeasibility) impossible. Fixing them now prevents operational incidents as the system moves toward continuous VTN reporting.

## What Changes

- **Background task panic recovery:** Introduce a single `supervised_spawn(name, cooldown_s, f)` utility in `tasks/mod.rs`. All seven task spawns in `main.rs` are replaced with one call each to this utility — restart loop, error logging, and cooldown live in one place, not repeated per task.
- **Profile startup validation:** Add `Profile::validate()` called from `main.rs` immediately after `Profile::load()`. Fail fast with a descriptive error on any invariant violation (unknown absorber asset IDs, out-of-range SoC targets, physics-infeasible discharge rates, etc.).
- **Typed domain errors:** Introduce `DomainError` enum (using `thiserror`, already in `Cargo.toml`) in `entities/`. Replace `anyhow::Error` at service-layer call sites where callers need to distinguish error kinds (plan infeasibility, VTN unreachable, profile invalid).
- **Named constants for magic numbers:** Replace the four remaining hardcoded durations (`planning.rs` initial delay 5 s, `obligation.rs` check interval 5 s, `solver_phase1.rs` HiGHS timeout 60 s, `vtn.rs` token margin 60 s) with named constants or profile config fields.

## Capabilities

### New Capabilities

- `task-supervisor`: A single `supervised_spawn` utility function used by all seven background task spawns. Handles restart loop, error logging, and cooldown in one place. No per-task boilerplate, no silent task death.
- `profile-validation`: Startup validation of the loaded `Profile` struct. Checks asset cross-references, numeric bounds, and physics feasibility. Terminates the process with a clear error message if any invariant is violated.
- `domain-errors`: `DomainError` enum in `entities/` covering the key failure modes at service boundaries: `PlanInfeasible`, `VtnUnreachable`, `ProfileInvalid`, `SessionConflict`. Services return `Result<T, DomainError>`; routes map to HTTP status codes.

### Modified Capabilities

- `planner-config`: Add `solver_timeout_s` and `planning_initial_delay_s` fields to `PlannerConfig` (profile `planner:` section) to replace hardcoded values in `tasks/planning.rs` and `milp_planner/solver_phase1.rs`.

## Impact

- **VEN backend:** All `VEN/src/tasks/`, `VEN/src/services/`, `VEN/src/entities/`, `VEN/src/profile.rs`, `VEN/src/main.rs`
- **No API changes:** All changes are internal; HTTP surface is unchanged
- **No Docker Compose changes:** No new containers or ports
- **openleadr-rs:** Not affected
- **Non-goals:** Code duplication (CD-01/CD-02), `AppState` refactor (AB-04), route → service remaining gaps, `TimeSeries<T>` alignment architecture — these remain for a separate change
