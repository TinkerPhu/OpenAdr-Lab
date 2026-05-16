## Context

`cargo check` on VEN/src/ (as of branch `029-fix-arch-invariants-tests`) emits 42 warnings. Chapter 8 of `docs/plans/ven_backend_architecture_refactoring_v2.md` deferred these to a dedicated cleanup task and described them as mostly `cargo fix`-able false positives. An explore-mode audit revealed this is only partially true: 11 are unused imports, but 31 are dead-code warnings covering a spectrum from "safe to delete" to "unfinished features" to "a behavioral bug in the EV session deletion route."

The VEN binary is a self-contained Rust service (`VEN/src/`). No BFF, VTN, UI, or database is affected by this change.

## Goals / Non-Goals

**Goals:**
- Reach zero `cargo check` warnings in the VEN binary
- Delete code that has no callers and no future purpose
- Annotate code that is deliberately deferred (unfinished features) so warnings are suppressed with explanation
- Fix the `DELETE /ev-session` behavioral gap so linked UserRequests are completed on session deletion

**Non-Goals:**
- Wire `apply_battery_correction_overlay` into the production dispatch loop (separate feature)
- Implement packet-based scheduling (`PacketSeed` / `ComfortRateSeed`)
- Address the VG-01–VG-07 architecture violations from `docs/plans/ven_backend_architecture_refactoring_v2.md`
- Any change to the public HTTP API shape, persistence format, or BFF layer

## Decisions

### Decision 1 — Six warning groups, six strategies

The 42 warnings split cleanly into groups requiring different treatment. Applying a single strategy (`cargo fix` or `#[allow(dead_code)]`) to all of them would either lose real logic or misrepresent the intent of deferred work.

| Group | Count | Strategy | Rationale |
|-------|-------|----------|-----------|
| Unused imports | 11 | Delete (cargo fix + manual re-export cleanup) | Mechanical. No callers, no risk. |
| Dead profile API | 8 | Delete | Methods replaced by `build_domain_params()` + `try_load()`. No callers anywhere in codebase. |
| Dead utilities | 4 | Delete | No callers, no tests, no future plan. |
| Test support mocks | ~12 | `#[cfg(test)]` on module declaration | Mocks are used in tests; compiling them unconditionally is the mistake, not the mocks. |
| Unfinished features | 5 items | `#[allow(dead_code)]` + comment | Real logic with tests or YAML config intent. Deleting would lose signal about incomplete work. |
| EV session bug | 1 | Fix the route | The service has passing tests for the correct behavior; the route simply wasn't wired to use it. |

### Decision 2 — `#[cfg(test)]` on `services/mod.rs::test_support` instead of `#[allow]` on each mock

Alternative: add `#[allow(dead_code)]` to each mock struct individually.

Chosen approach is better because it's idiomatic Rust (test-only infrastructure should be conditionally compiled), it silences all ~12 mock warnings in one line, and it makes the intent clear to future contributors. `cargo check` on the non-test build will no longer even compile the mocks.

Risk: any code that mistakenly imports from `test_support` outside a `#[cfg(test)]` context will fail to compile. This is desirable — it catches accidental non-test usage.

### Decision 3 — Delete dead profile methods, not suppress them

The 8 dead profile methods (`ev_config()`, `heater_config()`, etc.) could instead be annotated with `#[allow(dead_code)]`. They are not retained because:
- They have zero callers (confirmed by exhaustive grep)
- The refactoring that made them dead (`build_domain_params()`) is complete and stable
- Keeping unused API surface increases cognitive load
- They are not "deferred features" — they are genuinely superseded API

`Profile::load()` specifically: `main.rs` calls `Profile::try_load()` and handles the error itself. `load()` is a convenience wrapper that swallows errors. It should not be kept as a safety net since `main.rs` already has its own fallback to `Profile::default()`.

### Decision 4 — `#[allow(dead_code)]` with explanatory comment for `apply_battery_correction_overlay`

`apply_battery_correction_overlay` has 12 unit tests in `dispatcher.rs` and represents a complete implementation of Layer 1 battery correction (real-time deviation smoothing). It is deliberately not yet wired into `build_setpoints()` because the tick-loop integration requires setting the correct threshold/correction parameters from `PlannerParams` and handling the "hold previous correction" state.

Deleting the function and its tests would lose:
- The implemented logic
- The passing test suite that validates corner cases (SoC limits, direction flip, threshold hysteresis)

Strategy: add `#[allow(dead_code)]` on the function and `#[allow(dead_code)]` on the three `PlannerParams` fields that feed it (`deviation_threshold_kw`, `deviation_trigger_ticks`, `correction_min_kw`), each with a comment pointing to the integration work.

### Decision 5 — Fix `delete_ev_session` to call `EvSessionService::end()`

Current code in `routes/hems.rs`:
```rust
pub async fn delete_ev_session(State(ctx): State<AppCtx>) -> impl IntoResponse {
    ctx.state.set_ev_session(None).await;
    ...
}
```

Correct code:
```rust
pub async fn delete_ev_session(State(ctx): State<AppCtx>) -> impl IntoResponse {
    EvSessionService::end(&ctx.state).await.unwrap_or_default();
    ...
}
```

`EvSessionService::end()` calls `set_ev_session(None)` internally and additionally iterates active requests, finding those linked to the session and setting their status to `Completed`. The existing unit tests in `services/hems.rs` already verify this dual behavior.

Why this is a bug not a design choice: the service was written with tests before the route was updated. The route was later added/updated and the service call was omitted. There is no design intent to leave requests in Active state when a session ends.

## Risks / Trade-offs

| Risk | Mitigation |
|------|------------|
| `cargo fix` modifies re-exports incorrectly (e.g., removes `VtnPort` from `controller/mod.rs`) | Run `cargo fix` first, then audit the changed re-export lines manually before committing |
| `#[cfg(test)]` on `test_support` breaks a test that imports it without `#[cfg(test)]` | `cargo test` will catch this immediately; fix the import |
| Deleting `Profile::load()` breaks something not found by grep (e.g., integration test fixture) | `cargo check` and `cargo test` will catch any missing symbol |
| `EvSessionService::end()` returns `Result<()>`; route currently ignores errors | Use `.unwrap_or_default()` — the only error path in `end()` is None-session early return, which is already handled |
| `#[allow(dead_code)]` comments on deferred features become stale / misleading | Each comment names the specific integration work needed; reviewers can judge staleness from that context |
