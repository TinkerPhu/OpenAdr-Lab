# Research: Remove Profile from Routes Layer (AB-06)

**Feature**: `023-remove-profile-routes`  
**Status**: Complete — no open questions

## Decisions

### D-001: Boundary Check Implementation

**Decision**: `#[test]` in `VEN/tests/architecture.rs` using `std::fs` to walk `VEN/src/routes/` and assert no file contains `use crate::profile`.

**Rationale**: Runs automatically with `cargo test` — no CI configuration changes required. Fails with a clear panic message naming the offending file. No new crate dependencies (uses `std::fs::read_dir` recursively).

**Alternatives considered**:
- Shell script in `scripts/`: requires separate CI step invocation; easy to skip.
- `build.rs` compile-time check: fails the *build* rather than the test suite — too aggressive and harder to diagnose.

---

### D-002: `AppCtx.profile` Retention

**Decision**: Retain `profile: Arc<Profile>` in `AppCtx` with a doc-comment. Do not remove in this phase.

**Rationale**: `tasks/sim_tick` (and related task files) are confirmed non-route consumers of `Profile`. Removing from `AppCtx` requires Phase 4/5 refactoring which is out of scope here.

**Alternatives considered**:
- Remove in this phase and pass directly to `sim_tick`: broadens scope; risks destabilising unrelated tasks.

---

### D-003: Startup Error Propagation

**Decision**: `schema_from_profile` is called directly (it returns `HashMap`, not `Result`) — no `.expect()` needed on the return value. The call is infallible given a valid `Profile`. Profile YAML parsing already panics earlier if malformed.

**Rationale**: The current `schema_from_profile` signature returns `HashMap<String, Vec<ControlDescriptor>>` directly — no `Result`. Adding error handling here would be defensive code for an impossible case (Principle IV).

**Alternatives considered**:
- Wrapping in `Result`: unnecessary complexity given the infallible signature.

---

### D-004: Schema Identity Test Approach

**Decision**: Cargo integration test in `VEN/tests/schema_snapshot.rs`. Loads the `ven-1.yaml` profile fixture, calls `schema_from_profile`, serialises to JSON, and compares against `VEN/tests/fixtures/schema_snapshot.json`.

**Rationale**: Pure unit test is sufficient; no HTTP server needed to verify schema content correctness. Integration test crate boundary forces `schema_from_profile` to be `pub` — which is a healthy visibility signal (it's a stable interface).

**Alternatives considered**:
- BDD step calling `GET /sim/schema`: overkill for a pure content identity test; requires full stack.
- Unit test inside `simulator/mod.rs`: valid, but integration test placement makes the contract more explicit.

---

### D-005: `schema_from_profile` Visibility

**Decision**: Change from `pub(crate)` to `pub`.

**Rationale**: Integration tests in `VEN/tests/` are compiled as a separate crate and can only access `pub` items. The function is a stable, pure function — making it `pub` is appropriate.

**Alternatives considered**:
- Keep `pub(crate)` and use an internal unit test: forces the snapshot test into `simulator/mod.rs`, mixing test concerns with implementation.
