# Feature Specification: VEN Backend Refactoring

**Feature Branch**: `016-refactor-ven-backend`  
**Created**: 2026-04-29  
**Status**: Draft  
**Source**: `docs/plans/refactoring_backlog.md` (code review 2026-04-28), verified against `VEN/src/`

---

## Clarifications

### Session 2026-04-29

- Q: Should `"boiler"` be removed entirely or retained as a supported identifier? → A: Boiler is a confirmed distinct real asset type (200 L DHW tank, behaviorally separate from the 2000 L space-heating heater). Define `ids::BOILER` as a named constant; rewrite the `hems.rs` dual-match to use constants; defer full dispatcher/planner propagation to a separate feature; add a code comment at the dual-match site documenting the gap.
- Q: Should `InnerState` split into 2 or 3 sub-structs, and where do `controller_trace` and `inject_state` go? → A: 3 sub-structs. `PollingState` { programs, events, reports }. `SimState` { sensor, sim, inject_state, controller_trace }. `HemsState` { active_plan, planned_tariffs, capacity_state, report_obligations, asset_ledger, active_requests, site_envelope, ev_session, heater_target, shiftable_loads, shiftable_runtimes, baseline_override, ev_settings }.
- Q: How should the FR-014 "no two sub-struct locks simultaneously" invariant be enforced? → A: Documentation only. An `// INVARIANT:` comment in `state.rs` documents the rule. All accessors MUST use snapshot-and-release (never hold a guard across an `.await` or a second lock acquisition). Verified by code review.
- Q: What happens if a local YAML profile still has a `devices:` key after FR-004 removes the field? → A: Fail loudly. A post-parse validation step MUST check that `assets` is non-empty after deserialization; if empty, the VEN MUST refuse to start and log an explicit error (`"Profile has no assets — check for legacy 'devices:' key"`).

---

## Background

The VEN backend (`VEN/src/`) has accumulated structural debt across several incremental feature
phases (Phase A → Plan C → Plan F/G). This refactoring addresses seven identified issues: two
are trivial dead-code deletions (R-01, R-07), three are stalled partial migrations that left
dual code paths in place (R-02, R-03, R-04), one is a readability/testability split of a large
orchestration function (R-05), and one is a locking-architecture improvement (R-06). A further
issue (R-08, AssetConfig dispatch expansion via trait objects) is explicitly deferred.

**Scope:** `VEN/src/` Rust backend only. No changes to the VEN UI, BFF, VTN, or tests unless
required for compilation or API compatibility.

---

## Verified Findings (code-reviewed against source)

The following table summarises each item from the backlog and its status after source
verification. Items marked ✅ match the backlog exactly; items marked ⚠️ have corrections.

| ID   | Backlog Claim | Verified? | Correction |
|------|---------------|-----------|------------|
| R-01 | `controller/profile.rs` is absent from `controller/mod.rs` and never compiled | ✅ | — |
| R-02 | All four YAML profiles use `assets:` format; `devices:` fallback unused | ✅ | `no_pv_test.yaml` also uses `assets:` (5 profiles total, all confirmed) |
| R-03 | String literals scattered across 5+ files; `"boiler"` appears only in `hems.rs:275` | ✅ | — |
| R-04 | `AssetCapabilities` used only by `GET /capability` route | ⚠️ | **Route already uses `AssetCapability` (new).** `AssetCapabilities` and `capabilities()` are externally dead — no caller outside `assets/` uses them. The route change described in the backlog is already done. |
| R-05 | `spawn_sim_tick` has 13 inline phases | ✅ | — |
| R-06 | `InnerState` has 20+ fields across 3 concerns | ✅ | Exactly 20 fields confirmed |
| R-07 | `cancel_request` legacy `None =>` branch targets pre-Plan-C requests | ✅ | — |

---

## User Scenarios & Testing *(mandatory)*

Users of this refactoring are **developers** maintaining and extending the VEN backend. Each
story describes a developer task that the current code makes unnecessarily difficult.

### User Story 1 — Remove Phantom Dead File (Priority: P1)

A developer navigating `VEN/src/controller/` encounters `profile.rs` (22 KB). They spend time
reading it before discovering it is never compiled and has silently diverged from the real
`VEN/src/profile.rs`. After this refactoring the phantom file no longer exists.

**Why this priority**: Zero risk, zero effort. The file accumulates drift with every commit and
misleads anyone reading the controller directory. First change to make.

**Independent Test**: Fully validated by confirming the file is deleted and the project compiles
without errors or warnings.

**Acceptance Scenarios**:

1. **Given** the current codebase, **When** `VEN/src/controller/profile.rs` is deleted, **Then** `cargo build` succeeds with no errors and no new warnings.
2. **Given** a developer browsing `VEN/src/controller/`, **When** they list the directory, **Then** no `profile.rs` file is present.
3. **Given** any file in the codebase, **When** it is searched for `use crate::controller::profile`, **Then** no such import is found (confirming the file was always dead).

---

### User Story 2 — Remove `cancel_request` Legacy Fallback (Priority: P1)

A developer reviewing `state.rs` finds a `None =>` branch that handles `UserRequest` records
without a `session_type` field — records that can only exist within a single uptime cycle
(the field is not persisted). After this refactoring the dead branch is gone and an unexpected
`session_type: None` logs a warning instead of silently no-oping.

**Why this priority**: Trivial deletion. The branch is already dead after any clean restart.
Leaving it in place hides bugs: a request with `session_type: None` today would silently fail
to clear the device session.

**Independent Test**: Verified by confirming the `None =>` arm is removed, a `warn!()` is
emitted for any future `session_type: None` case, and all `cancel_request` unit tests pass.

**Acceptance Scenarios**:

1. **Given** a `UserRequest` with `session_type: Some(SessionType::Ev)`, **When** `cancel_request` is called, **Then** `ev_session` is cleared and the function returns `true`.
2. **Given** a `UserRequest` with `session_type: Some(SessionType::Heater)`, **When** `cancel_request` is called, **Then** `heater_target` is cleared.
3. **Given** a `UserRequest` with `session_type: Some(SessionType::ShiftableLoad)`, **When** `cancel_request` is called, **Then** the matching shiftable load and runtime are removed.
4. **Given** a `UserRequest` with `session_type: None` (hypothetical future bug), **When** `cancel_request` is called, **Then** a warning is logged and `true` is still returned (request is marked cancelled).

---

### User Story 3 — Remove `AssetCapabilities` Dead Code (Priority: P2)

A developer adding a new asset type discovers they must implement `capabilities()` alongside
`capability()` and wonders which one is actually used. After this refactoring only the current
`AssetCapability` type and `capability()` method exist.

**Why this priority**: The legacy `AssetCapabilities` struct and all five `capabilities()` method
implementations are externally dead (the `/capability` route already uses `AssetCapability`).
Removing them eliminates misleading "planner compat" comments, the orphaned `EnergyState` and
`TimeWindow` structs, and the false impression that a new asset type must implement two separate
capability descriptors.

**Independent Test**: Verified by deleting `AssetCapabilities`, `EnergyState`, `TimeWindow`,
`AssetConfig::capabilities()`, and all five asset-level `capabilities()` methods; confirming
`cargo build` succeeds; and confirming `GET /capability/:asset_id` returns the correct response
shape.

**Acceptance Scenarios**:

1. **Given** the refactored codebase, **When** `cargo build` is run, **Then** no references to `AssetCapabilities`, `EnergyState`, or `TimeWindow` remain and the build succeeds.
2. **Given** the VEN is running, **When** `GET /capability/battery` is called, **Then** the response contains `max_import_kw`, `max_export_kw`, and `is_fixed` (unchanged from before).
3. **Given** a developer implementing a new asset type, **When** they look at the `Asset` trait, **Then** only `capability()` returning `AssetCapability` is required — no `capabilities()` method.

---

### User Story 4 — Remove Legacy `DeviceConfig` / `devices` Field (Priority: P2)

A developer adding a new asset to a YAML profile finds two parallel config formats and is
unsure which one to use. After this refactoring only the `assets:` list format exists and
every accessor method is a simple `self.assets.iter()` call.

**Why this priority**: All five YAML profiles already use the `assets:` format exclusively.
The `DeviceConfig` struct, its five optional fields, and the five `default_*` free functions
exist solely as dead fallback code. Every accessor method currently does two lookups; after
removal it does one.

**Prerequisite**: Confirm all profile YAML files in `VEN/profiles/` use `assets:` and not
`devices:` before deleting the fallback (already verified: all five profiles — ven-1, ven-2,
ven-3, test, no_pv_test — use `assets:`).

**Independent Test**: Verified by deleting `DeviceConfig`, the `devices` field from `Profile`,
and the 5 fallback `or(self.devices.*)` clauses; confirming `cargo build` succeeds; and
confirming all existing `profile.rs` tests pass.

**Acceptance Scenarios**:

1. **Given** the refactored codebase, **When** `cargo build` is run, **Then** no references to `DeviceConfig` or `self.devices` remain.
2. **Given** a YAML profile using `assets:` list format, **When** `ev_config()` is called, **Then** it returns the correct config without consulting any `devices` field.
3. **Given** any of the five profile YAML files, **When** parsed, **Then** deserialization succeeds and all asset accessors return the expected values.
4. **Given** the `Profile` struct, **When** a developer inspects it, **Then** only the `assets: Vec<AssetProfile>` field represents asset configuration — no `devices` field exists.

---

### User Story 5 — Centralize Asset ID String Constants (Priority: P2)

A developer searches for all places where a heater asset is referenced by ID and misses the
`"boiler"` alias in `hems.rs` because there is no shared definition. After this refactoring
all asset IDs are defined as constants in one place, and the `"boiler"` alias is explicitly
resolved (either promoted to a first-class alias or removed).

**Why this priority**: The `"boiler"` alias is a latent bug: a VEN configured with
`id: boiler` will match in the HEMS route but miss the dispatcher and planner. The scattering
of string literals also makes renaming or adding an asset type error-prone.

**Independent Test**: Verified by confirming a single `ids` module holds all asset ID
constants, no bare string literals matching `"battery"`, `"ev"`, `"heater"`, `"pv"`, or
`"boiler"` remain in production code (test assertions may keep string literals), and the
`"boiler"` alias question is explicitly resolved with a comment or a defined constant.

**Acceptance Scenarios**:

1. **Given** the refactored codebase, **When** production source files (excluding tests and test helpers) are searched for the string `"battery"`, `"ev"`, `"heater"`, `"pv"`, or `"boiler"` as standalone asset ID values, **Then** none are found — only references to the constants.
2. **Given** a shared constants module, **When** a developer needs the EV asset ID, **Then** they find a single authoritative definition.
3. **Given** the `"boiler"` alias, **When** the refactoring is complete, **Then** one of: (a) `"boiler"` is defined as an explicit alias constant with a comment explaining it maps to the heater, or (b) it is removed with a comment noting it was never propagated beyond the HEMS route.
4. **Given** a VEN YAML profile with `id: boiler`, **When** a user-request targets that ID, **Then** the HEMS session path accepts the request via the `ids::BOILER` constant. Note: full dispatcher and planner propagation for boiler is out of scope per FR-008; a `// TODO(boiler-physics):` comment at the dual-match site documents the gap.

---

### User Story 6 — Extract `spawn_sim_tick` Phases into Named Functions (Priority: P3)

A developer wants to write a unit test for the battery correction hold logic (Plan G, phase 4)
but cannot isolate it without running the full simulator tick, holding a mutex, and
constructing an `AppCtx`. After this refactoring each logical phase is a named function that
can be called independently in a test.

**Why this priority**: No behaviour change. Pure readability and testability improvement.
Lower priority than the dead-code removals because it requires careful extraction and
re-wiring of the orchestrator, but it is the largest single contributor to future
maintainability.

**Independent Test**: Verified by confirming `spawn_sim_tick` delegates to named functions,
at least one new unit test exercises an extracted phase in isolation without constructing
`AppCtx`, and end-to-end behaviour is unchanged (existing BDD tests pass).

**Acceptance Scenarios**:

1. **Given** the refactored `loops.rs`, **When** a developer reads `spawn_sim_tick`, **Then** it reads as an orchestrator calling clearly named functions — one per logical phase — not as a single inline block.
2. **Given** the extracted phase functions, **When** a developer writes a unit test for the setpoint computation phase, **Then** they can call the function directly with a snapshot and plan, without needing the full `AppCtx` or a running sim loop.
3. **Given** the VEN running against the VTN, **When** the sim tick fires, **Then** observable behaviour (asset states, history ring buffer, SSE events, planner triggers) is identical to the pre-refactoring behaviour.
4. **Given** the existing BDD and unit test suites, **When** run against the refactored code, **Then** all tests pass.

---

### User Story 7 — Split `InnerState` into Domain Sub-Structs (Priority: P3)

A developer tracing a lock-contention issue under load cannot tell whether the write lock in
the sim tick is blocking HTTP reads of `programs` (a completely unrelated concern). After this
refactoring the three concerns — polling state, HEMS controller state, and sim state — each
have independent locks, making contention visible and bounded.

**Why this priority**: The functional impact today is limited (contention exists but is not
yet a measured bottleneck). However, as the sim tick loop holds the write lock across all 13
phases, any future addition that prolongs the tick will starve HTTP handlers. The split
establishes the correct architecture before that becomes a production incident.

**Independent Test**: Verified by confirming `InnerState` is split into at least two sub-structs
each with its own lock, `programs/events/reports` reads no longer contend with sim-tick writes,
all HTTP routes compile and function correctly, and existing tests pass.

**Acceptance Scenarios**:

1. **Given** the refactored `state.rs`, **When** a developer reads the state definition, **Then** fields are grouped into clearly named domain sub-structs (e.g. polling state, HEMS state, sim state) each protected by its own lock.
2. **Given** concurrent HTTP reads of `/programs` and a running sim tick writing sim state, **When** load is applied, **Then** the read does not wait for the sim tick's write lock to be released (locks are now separate).
3. **Given** all existing accessors on `AppState`, **When** called, **Then** they behave identically to before (same values returned, same mutation semantics).
4. **Given** the existing BDD and unit test suites, **When** run against the refactored code, **Then** all tests pass.

---

### Edge Cases

- What if a developer has a local YAML profile using the legacy `devices:` format? → FR-009 requires a post-parse validation: if `assets` is empty after deserialization, the VEN refuses to start with an explicit error message (`"Profile has no assets — check for legacy 'devices:' key"`). Silent misconfiguration is not acceptable.
- What if the `"boiler"` alias is retained as a constant? → Boiler is a confirmed distinct asset type (see Assumptions). It is defined as `ids::BOILER`, the hems.rs dual-match uses the constant, and a comment documents the missing dispatcher/planner propagation as a follow-up item.
- What if extracting `spawn_sim_tick` phases creates borrowing issues due to the mutex guard? → The snapshot-and-release pattern (drop guard, work on snapshot) is already used elsewhere in the codebase and should be applied here.
- What if splitting `InnerState` introduces a deadlock? → No function should hold two sub-struct locks simultaneously. This must be an explicit invariant documented in `state.rs`.

---

## Requirements *(mandatory)*

### Functional Requirements

**Dead code removal (R-01, R-07)**

- **FR-001**: The file `VEN/src/controller/profile.rs` MUST be deleted from the repository.
- **FR-002**: The `None =>` branch in `cancel_request` MUST be removed. In its place, any `UserRequest` with `session_type: None` at cancel time MUST produce a logged warning.
- **FR-003**: The project MUST compile without errors after R-01 and R-07 are applied independently.

**Legacy migration completion (R-02, R-03, R-04)**

- **FR-004**: The `DeviceConfig` struct, the `devices` field on `Profile`, and all `or(self.devices.*)` fallback clauses in the five accessor methods MUST be removed from `profile.rs`.
- **FR-005**: Each accessor method (`ev_config`, `heater_config`, `pv_config`, `battery_config`, `base_load_kw`) MUST be simplified to a single-pass `self.assets.iter()` lookup with no fallback.
- **FR-006**: The `AssetCapabilities` struct, the `EnergyState` struct, the `TimeWindow` struct, `AssetConfig::capabilities()`, and the five asset-level `capabilities()` methods MUST be deleted.
- **FR-007**: All asset ID values (`"battery"`, `"ev"`, `"heater"`, `"pv"`, `"boiler"`, `"base_load"`) MUST be defined as named constants in a single shared module. All production call sites MUST reference these constants — no exemptions for `default_asset_id_*()` helper functions.
- **FR-008**: `"boiler"` MUST be defined as a named constant (`ids::BOILER`). Boiler is a distinct real asset type (200 L domestic hot water tank, behaviorally separate from the 2000 L space-heating heater) — not an alias or typo. The existing dual-match in `hems.rs` (`"heater" || "boiler"`) MUST be rewritten to use the named constants. Full propagation of boiler to the dispatcher and planner (which requires its own physics model) is explicitly OUT OF SCOPE for this refactoring and MUST be tracked as a follow-up. A code comment MUST be placed at the HEMS dual-match site documenting that boiler is not yet handled by the dispatcher and planner.
- **FR-009**: After R-02, R-03, and R-04 are applied, the VEN MUST start successfully with all existing YAML profiles (ven-1, ven-2, ven-3, test, no_pv_test). Additionally, if `Profile` is deserialized with an empty `assets` list, the VEN MUST refuse to start and emit an explicit error message (`"Profile has no assets — check for legacy 'devices:' key"`).
- **FR-010**: The `GET /capability/:asset_id` endpoint MUST return the same response shape as before (`max_import_kw`, `max_export_kw`, `is_fixed`).

**Structural improvements (R-05, R-06)**

- **FR-011**: The inline body of `spawn_sim_tick` MUST be decomposed into at least six named functions, one per logical phase group. `spawn_sim_tick` itself MUST act only as an orchestrator.
- **FR-012**: Each extracted phase function MUST be callable in a unit test without constructing an `AppCtx` or holding the sim mutex.
- **FR-013**: `InnerState` MUST be split into exactly three domain sub-structs, each with its own independent `Arc<RwLock<...>>`:
  - `PollingState` — `{ programs, events, reports }` — written by poll loops, read by routes.
  - `ControllerSimState` — `{ sensor, sim, inject_state, controller_trace }` — written by the sim tick, read by routes. (Named `ControllerSimState` in all implementation artifacts to avoid collision with `simulator::SimState`.)
  - `HemsState` — `{ active_plan, planned_tariffs, capacity_state, report_obligations, asset_ledger, active_requests, site_envelope, ev_session, heater_target, shiftable_loads, shiftable_runtimes, baseline_override, ev_settings }` — written by the HEMS controller, read by routes.
- **FR-014**: No function in the codebase MUST acquire more than one of the three sub-struct locks simultaneously (deadlock prevention invariant). This invariant MUST be documented with an `// INVARIANT:` comment at the top of `state.rs`. All accessors MUST follow the snapshot-and-release pattern: acquire lock → clone required fields → release lock → work on snapshot. No lock guard may be held across an `.await` point or a second lock acquisition. Enforcement is by code review.

**General**

- **FR-015**: No observable behaviour of the VEN MUST change as a result of this refactoring. All existing BDD tests and unit tests MUST pass.
- **FR-016**: R-08 (`AssetConfig` → `dyn Asset` dispatch) is explicitly OUT OF SCOPE for this refactoring.

### Key Entities

- **`Profile`**: Parsed YAML configuration for a VEN instance. After R-02, holds only `assets: Vec<AssetProfile>` for asset configuration (no `devices` field).
- **`AssetConfig`**: Runtime physics dispatch enum. After R-04, the only capability descriptor method is `capability()` returning `AssetCapability`.
- **`AssetCapability`**: Point-in-time feasible power range for an asset (current, retained as-is).
- **`InnerState`**: After R-06, replaced by three domain sub-structs each with its own lock: `PollingState` (OpenADR poll data), `ControllerSimState` (sensor, sim snapshot, inject state, controller trace), and `HemsState` (plans, sessions, obligations, ledger).
- **`ids` module**: New shared module holding string constants for all asset IDs.

---

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: After all R-01 through R-07 changes are applied, `cargo build --workspace` completes with zero errors and zero new warnings compared to the pre-refactoring baseline.
- **SC-002**: Dead code deleted: `controller/profile.rs` (22 KB), `DeviceConfig` + `devices` field, `AssetCapabilities` + `EnergyState` + `TimeWindow`, 5 × `capabilities()` implementations, `cancel_request` legacy `None =>` branch — all absent from the codebase.
- **SC-003**: All existing unit tests in `VEN/src/` pass without modification (except trivial import path updates caused by the `ids` module introduction or `InnerState` split).
- **SC-004**: All existing BDD feature tests pass against a VEN built from the refactored code.
- **SC-005**: The `spawn_sim_tick` function body is reduced to an orchestration call sequence of named functions; no single extracted phase function exceeds 60 lines.
- **SC-006**: At least one new unit test exercises an extracted sim-tick phase in isolation (without `AppCtx`).
- **SC-007**: Lock contention between `GET /programs` and the sim tick write path is eliminated: the two operations no longer share a lock.
- **SC-008**: No string literal matching `"battery"`, `"ev"`, `"heater"`, `"pv"`, or `"boiler"` appears as a standalone asset ID value in non-test production source files after R-03 is applied.

- **SC-009**: A VEN started with a YAML profile that has an empty `assets` list (e.g. a legacy `devices:`-only profile) MUST exit at startup with a non-zero status code and a human-readable error message identifying the cause.

- All YAML profiles in `VEN/profiles/` use the `assets:` format. This was verified against all five files: `ven-1.yaml`, `ven-2.yaml`, `ven-3.yaml`, `test.yaml`, `no_pv_test.yaml`. No `devices:` key is present in any of them.
- The `"boiler"` string in `hems.rs:275` is treated as an undocumented alias for `"heater"` rather than a distinct asset type. This assumption must be confirmed with the team before resolving FR-008.
- **Boiler is a confirmed distinct real asset type**: a 200 L domestic hot water (DHW) tank whose usage is weakly predictable via heuristic forecasting of past user behaviour plus constant isolation loss. The Heater is a 2000 L space-heating buffer tank with a 6 kW 3-phase resistive element, controlled by outside-temperature forecast. Both share the same HEMS session handling path for now. Boiler physics and planner integration are deferred to a future feature.
- `AssetProfile` and `AssetConfig` share variant names (`Ev`, `Battery`, etc.) but hold different inner types by design (documented in `profile.rs` Notes section). This naming is preserved; renaming `AssetProfile` → `AssetSpec` is noted as a future improvement but is out of scope here.
- The R-06 split uses exactly three sub-structs (`PollingState`, `SimState`, `HemsState`) as specified in FR-013. `controller_trace` and `inject_state` belong to `SimState` (both consumed/written during sim tick phases).
- R-08 (trait-object dispatch for `AssetConfig`) remains explicitly deferred.
