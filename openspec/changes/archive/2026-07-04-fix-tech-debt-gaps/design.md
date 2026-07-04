## Context

The VEN backend runs seven long-lived `tokio::spawn` tasks. A panic in any task silently kills it — the process continues but DR control stops. Profile loading performs no validation, so misconfigured YAML causes wrong physics or panics at runtime rather than a clear startup error. All errors flow through `anyhow::Result`, making domain-specific recovery impossible at service boundaries. Four timing constants are hardcoded with no config key.

All fixes are internal to `VEN/src/`. No API surface changes. No infrastructure changes.

## Goals / Non-Goals

**Goals:**
- Background tasks restart automatically after a panic (with logging and cooldown)
- Invalid profiles fail at startup with a human-readable error, not a runtime panic
- Service layer distinguishes key error kinds so callers can handle them differently
- Four magic-number durations are named and at least two become profile-configurable

**Non-Goals:**
- `AppState` catch-all refactor (AB-04) — separate change
- Code duplication (CD-01/CD-02) — separate change
- Full typed OpenADR structs in `vtn.rs` reports — separate change
- `TimeSeries<T>` alignment architecture — separate change

## Decisions

### D-01 — Supervisor: single `supervised_spawn` utility, not per-task boilerplate

**Decision:** Add one `pub fn supervised_spawn(name: &'static str, cooldown_s: u64, f: impl Fn() -> JoinHandle<()> + Send + 'static)` function to `tasks/mod.rs`. `main.rs` calls it once per task — the restart loop, error logging, and cooldown live entirely inside `supervised_spawn`. No external supervisor crate.

```rust
// tasks/mod.rs
pub fn supervised_spawn(
    name: &'static str,
    cooldown_s: u64,
    make_task: impl Fn() -> tokio::task::JoinHandle<()> + Send + 'static,
) {
    tokio::spawn(async move {
        loop {
            if let Err(e) = make_task().await {
                tracing::error!(task = name, "panicked: {e:?}, restarting in {cooldown_s}s");
                tokio::time::sleep(std::time::Duration::from_secs(cooldown_s)).await;
            }
        }
    });
}

// main.rs — each of the seven spawns becomes:
supervised_spawn("sim_tick", 5, || tasks::sim_tick::spawn(...));
supervised_spawn("planning",  5, || tasks::planning::spawn(...));
// ... etc.
```

**Rationale:** Seven tasks means seven potential copy-paste sites. Extracting the pattern to a single function eliminates the duplication, makes the restart behaviour independently testable, and makes the intent visible at the call site without requiring the reader to parse each loop individually. External crates add a dependency for something trivially expressible in one function.

**Alternative considered:** Inline `loop { spawn(...).await; ... }` per task — works but multiplies maintenance surface by 7. `JoinSet` with centralised restart — more complex with no benefit for a fixed known set of tasks.

**Cooldown:** 5 seconds, passed as a parameter so tests can use 0 and production can tune per-task if needed later.

### D-02 — Profile validation: `Profile::validate()` returns `Vec<String>` errors

**Decision:** Add `Profile::validate(&self) -> Result<(), Vec<String>>` to `profile.rs`. Collect all violations into a `Vec<String>` and return them all at once rather than failing on the first. Called in `main.rs` immediately after `Profile::load()`.

**Rationale:** Returning all violations at once is more useful than stopping at the first — a user fixing a misconfigured profile needs to see all problems in one startup attempt. `Vec<String>` keeps the implementation simple; no need for a typed error enum here since validation failures always terminate the process.

**Checks to implement:**
1. Absorber asset IDs reference assets that exist in `assets:`
2. `ev.soc_target` ∈ [0.0, 1.0]
3. `ev.max_discharge_kw` ≥ 0.0
4. `battery.min_soc` ∈ [0.0, 1.0) and < 1.0
5. `battery.round_trip_efficiency` ∈ (0.0, 1.0]
6. `planner.replan_interval_s` > 0
7. `planner.phase2_epsilon_eur` ≥ 0.0
8. `absorber.dead_band_kw` ≥ 0.0
9. At least one asset declared

### D-03 — Domain errors: `thiserror` enum in `entities/error.rs`, used at service boundaries only

**Decision:** Add `VEN/src/entities/error.rs` with a `DomainError` enum. Update service function signatures in `services/` to return `Result<T, DomainError>` at call sites where the caller needs to distinguish kinds. Route handlers map `DomainError` to HTTP status codes.

**Variants:**
```rust
#[derive(Debug, thiserror::Error)]
pub enum DomainError {
    #[error("session conflict: {0}")]
    SessionConflict(String),
    #[error("request not found: {id}")]
    NotFound { id: uuid::Uuid },
    #[error("plan infeasible: {0}")]
    PlanInfeasible(String),
    #[error("VTN unreachable: {0}")]
    VtnUnreachable(String),
    #[error("profile invalid: {0}")]
    ProfileInvalid(String),
}
```

**Rationale:** `thiserror` is already in `Cargo.toml`. Placing the enum in `entities/` keeps it in the inner ring where all service code can import it without violating the dependency rule. Services that do not need discrimination (e.g. internal helpers) keep `anyhow::Result` — the change is additive, not a wholesale replacement.

**Alternative considered:** Replace all `anyhow` with typed errors — too large, high churn, low immediate value. Targeted adoption at service boundaries is sufficient.

### D-04 — Magic numbers: two become profile config, two become named constants

**Decision:**
- `planner.solver_timeout_s` (default `60`) → added to `PlannerConfig` in `profile.rs`, passed to `solver_phase1.rs` and `solver_phase2.rs` via `PlannerParams`
- `planner.planning_initial_delay_s` (default `5`) → added to `PlannerConfig`, used in `tasks/planning.rs`
- `obligation check interval` (5 s) → named constant `OBLIGATION_CHECK_INTERVAL_S: u64 = 5` in `tasks/obligation.rs`
- `vtn token safety margin` (60 s) → named constant `TOKEN_EXPIRY_MARGIN_S: u64 = 60` in `vtn.rs`

**Rationale:** The solver timeout and planning delay are operationally tunable (e.g. slower Pi4 may need longer timeout); they belong in profile config. The obligation interval and token margin are implementation constants that don't warrant user configuration but benefit from being named for documentation purposes.

## Risks / Trade-offs

- **Restart loop masking bugs** → The 5 s cooldown and `error!` log make the restart visible. A task that panics on every start will log continuously, which is an observable signal to investigate.
- **Profile validation false positives** → Overly strict bounds could block valid exotic profiles. Mitigated by: checks are limited to clear invariants (non-negative, in [0,1] ranges, referential integrity). Physics-feasibility checks are left for a later change.
- **Partial `DomainError` adoption** → Some callers still get `anyhow::Error` from services not yet converted. This is intentional — additive adoption avoids a big-bang refactor. Document which services use `DomainError` and which do not.
- **`PlannerParams` struct grows** → Adding `solver_timeout_s` and `planning_initial_delay_s` to `PlannerParams` is a minor API extension. Existing callers use struct update syntax or the `Default` impl, so no breaking change.
