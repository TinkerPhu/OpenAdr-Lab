# Feature Specification: Remove Profile from Routes Layer (AB-06)

**Feature Branch**: `023-remove-profile-routes`  
**Created**: 2026-05-13  
**Status**: Draft  
**Input**: User description: "specify according docs/plans/phase6-remove-profile-from-routes.md"

## Clarifications

### Session 2026-05-13

- Q: What implementation approach should be used for the automated boundary check (FR-005)? → A: A `#[test]` in a dedicated `VEN/tests/architecture.rs` file that greps `VEN/src/routes/**/*.rs` for `Profile` imports and fails with a diagnostic message.
- Q: Should the `profile: Arc<Profile>` field on `AppCtx` be removed or retained in this phase? → A: Retained. The `sim_tick` task is a confirmed non-route consumer. The field stays in `AppCtx` and is annotated with a doc-comment naming `sim_tick` as its owner. Full removal is deferred to Phase 4/5.
- Q: How should startup failure in schema pre-computation propagate? → A: `schema_from_profile` is infallible — it returns `HashMap` directly (not a `Result`). No startup error handling is needed; the function builds the schema from an already-parsed, validated `Profile` struct. If the profile YAML is invalid, startup panics earlier during profile loading.
- Q: How should SC-002 (identical `GET /sim/schema` response) be verified? → A: A Cargo integration test in `VEN/tests/` that constructs the schema from a known profile fixture and asserts the JSON output is identical to a captured pre-change snapshot.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Route Handlers Are Config-Agnostic (Priority: P1)

A developer extending or debugging the HTTP route layer should never need to understand raw configuration format (YAML structure, profile variants) to reason about what a route does. All values a route handler reads from the shared application context are pre-computed, typed domain values — not raw config objects.

**Why this priority**: This is the core architectural fix. Without it, the route layer remains coupled to config internals, making routes harder to test in isolation and violating the rule that HTTP adapters must not import domain-level config types.

**Independent Test**: Verify all route handler files and confirm they contain zero references to the raw configuration type. Any attempt to introduce a config import into a route handler fails at build time or is caught by the automated check.

**Acceptance Scenarios**:

1. **Given** all route handler files, **When** a search for raw configuration imports is run, **Then** zero matches are found.
2. **Given** the `GET /sim/schema` endpoint, **When** a client requests the schema, **Then** the response is identical in structure and content to the response produced before this change.
3. **Given** the application starts up, **When** the shared context is initialized, **Then** the schema value is pre-computed once and route handlers access it directly without any further configuration lookup.

---

### User Story 2 - Automated Boundary Check Prevents Regression (Priority: P2)

A developer merging future changes should be automatically alerted if a route file re-introduces a configuration import, without relying on manual code review to catch it.

**Why this priority**: Without an automated check the boundary can silently erode. The check makes the constraint permanent and catches violations immediately in CI.

**Independent Test**: Inject a deliberate config import into a route file and confirm the automated check step fails. Remove the import and confirm the check passes.

**Acceptance Scenarios**:

1. **Given** the CI or test pipeline, **When** a route file contains a raw configuration import, **Then** the automated boundary check fails with a clear diagnostic message.
2. **Given** clean route files with no configuration imports, **When** the boundary check runs, **Then** it passes without errors.

---

### User Story 3 - Shared Context Field Is Explicitly Annotated (Priority: P3)

After routes no longer access raw configuration, the `profile: Arc<Profile>` field remains on the shared context because `sim_tick` still requires it. The field MUST carry a doc-comment that names `sim_tick` as the owner and states that removal is deferred to Phase 4/5. This makes the retention intentional and visible to future maintainers.

**Why this priority**: An undocumented field retained "just in case" is a future footgun. The annotation makes the decision explicit and prevents a future maintainer from removing it without understanding the dependency.

**Independent Test**: Inspect `AppCtx` — the `profile` field is present and its doc-comment names `sim_tick` and references Phase 4/5 removal.

**Acceptance Scenarios**:

1. **Given** the `AppCtx` struct definition, **When** it is inspected, **Then** the `profile: Arc<Profile>` field is present with a doc-comment identifying `sim_tick` as its owner and deferring removal to Phase 4/5.
2. **Given** no route handler file, **When** a search for `profile` field access is run within `VEN/src/routes/`, **Then** zero matches are found.

---

### Edge Cases

- What if additional configuration accesses in routes surface after Phase 5 cleanup? The same pre-computation pattern applies to all instances uniformly.
- What if the pre-computed schema is large? It is built once at startup and shared by reference — no per-request cost regardless of size.
- What if configuration is malformed at startup? `schema_from_profile` is infallible — the Profile struct is already validated during YAML loading at startup, which panics on parse error before `schema_from_profile` is ever called. No error handling is needed at the schema pre-computation call site.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: The system MUST ensure no route handler file contains a direct import of the raw application configuration type.
- **FR-002**: The system MUST pre-compute the simulator schema once at application startup from the raw configuration and store it in the shared application context as a typed, ready-to-use value.
- **FR-003**: The `GET /sim/schema` endpoint MUST return a response with identical structure and content to its pre-change response, verified by a Cargo integration test in `VEN/tests/` using a known profile fixture and a captured JSON snapshot.
- **FR-004**: The `profile: Arc<Profile>` field on the shared application context (`AppCtx`) MUST be retained in this phase, as `sim_tick` is a confirmed non-route consumer. It MUST be annotated with a doc-comment identifying `sim_tick` as its owner and noting that removal is deferred to Phase 4/5.
- **FR-005**: An automated check MUST be added as a `#[test]` in `VEN/tests/architecture.rs` that greps all files under `VEN/src/routes/` for imports of the raw `Profile` configuration type and fails the test run with a clear diagnostic message when any are found.
- **FR-006**: All existing automated test scenarios MUST continue to pass without modification after this change.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: Zero references to the raw configuration type exist in any route handler file, confirmed by automated search producing zero matches.
- **SC-002**: The `GET /sim/schema` endpoint response is identical to the pre-change response, confirmed by a Cargo integration test in `VEN/tests/` that constructs the schema from a known profile fixture and asserts the JSON output matches a captured pre-change snapshot.
- **SC-003**: All existing BDD and integration test scenarios pass on the target deployment environment after the change, with zero new failures.
- **SC-004**: The `VEN/tests/architecture.rs` boundary check demonstrably fails when a test violation is injected into a route file and passes when the violation is removed.
- **SC-005**: `AppCtx.profile` is retained and carries a doc-comment identifying `sim_tick` as its owner and deferring removal to Phase 4/5.

## Assumptions

- Phase 5 application service layer changes are complete, or the only remaining configuration access in routes is the `GET /sim/schema` handler — this phase is valid as a standalone if that is the sole breach.
- The schema-building function already exists in the simulator module and requires no logic changes; only its call site moves from the route handler to application startup.
- The pre-computed schema is read-only after startup and safe to share across concurrent requests.
- Task files (e.g., the `sim_tick` task) are out of scope for this phase; they are addressed in Phase 4/5. `AppCtx.profile` is retained for their use and annotated accordingly.
- The infrastructure startup entry point reading raw configuration at startup remains correct and is not changed.

## Dependencies

- **Prerequisite**: Phase 4/5 changes removing configuration from entities, assets, and controller — either complete or the route breach is isolated enough to proceed independently (confirmed: it is).
- **Architecture reference**: AB-06 documented in `docs/architecture/ven_backend_review.md` and `docs/architecture/ven_backend_components.md`.
- **Source plan**: `docs/plans/phase6-remove-profile-from-routes.md` and `docs/plans/ven_backend_architecture_refactoring.md §4 Phase 6`.
