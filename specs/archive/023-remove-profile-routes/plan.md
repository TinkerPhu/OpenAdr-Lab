# Implementation Plan: Remove Profile from Routes Layer (AB-06)

**Branch**: `023-remove-profile-routes` | **Date**: 2026-05-13 | **Spec**: `specs/023-remove-profile-routes/spec.md`  
**Input**: Feature specification from `/specs/023-remove-profile-routes/spec.md`

## Summary

Pre-compute the simulator schema once at application startup (in `main.rs`) from `Profile` and store it as `Arc<HashMap<String, Vec<ControlDescriptor>>>` on `AppCtx`. The `GET /sim/schema` route handler is updated to clone from `AppCtx` rather than call `schema_from_profile`. A `#[test]` in `VEN/tests/architecture.rs` enforces the boundary permanently. `AppCtx.profile` is retained with a doc-comment naming `sim_tick` as its owner.

## Technical Context

**Language/Version**: Rust stable 2021  
**Primary Dependencies**: axum, tokio, serde_json (no new deps)  
**Storage**: N/A — pre-computed value lives in memory; no persistence changes  
**Testing**: `cargo test` — integration tests in `VEN/tests/`; unit test in `VEN/src/simulator/`  
**Target Platform**: Linux ARM64 (Raspberry Pi 4), Docker Compose v2  
**Project Type**: Service (VEN backend)  
**Performance Goals**: No per-request overhead — schema built once at startup  
**Constraints**: No file may exceed 500 lines (Principle VI); no new crate dependencies  
**Scale/Scope**: Single struct field change + two new test files + one route simplification

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-checked after Phase 1 design.*

| Principle | Status | Notes |
|-----------|--------|-------|
| I. OpenADR Spec Fidelity | ✅ Pass | No field names or API shapes change |
| II. BDD-First Testing | ✅ Pass | FR-006 requires full BDD suite passes; SC-003 confirms it |
| III. Upstream Compatibility | ✅ Pass | VEN is not a submodule; no DCO concern |
| IV. Lean Architecture | ✅ Pass | One field added, one call site simplified, zero new abstractions |
| V. Infrastructure Parity | ✅ Pass | No Compose changes; same deploy flow |
| VI. VEN Backend Hexagonal | ✅ **Direct fix** | This change closes AB-06: removes `use crate::profile` from `routes/`; architecture test enforces it permanently |

No violations. Complexity Tracking table not required.

## Project Structure

### Documentation (this feature)

```text
specs/023-remove-profile-routes/
├── plan.md              # This file
├── research.md          # Phase 0 output
├── data-model.md        # Phase 1 output
├── quickstart.md        # Phase 1 output
├── contracts/           # Phase 1 output
└── tasks.md             # Phase 2 output (speckit.tasks)
```

### Source Code

```text
VEN/
├── src/
│   ├── main.rs                    # MODIFY: add sim_schema to AppCtx, pre-compute at startup
│   ├── routes/
│   │   └── sim.rs                 # MODIFY: replace schema_from_profile call with ctx.sim_schema.clone()
│   └── simulator/
│       └── mod.rs                 # MODIFY: change schema_from_profile pub(crate) → pub
└── tests/
    ├── architecture.rs            # ADD: boundary check — grep routes/ for Profile imports
    └── schema_snapshot.rs         # ADD: snapshot test — schema from fixture equals captured JSON
```

**Structure Decision**: Single-project Rust service. All changes are in `VEN/`. No new modules or crates.

## Phase 0: Research

No unknowns requiring external research. All decisions were resolved during clarification:

- **Boundary check approach**: `#[test]` in `VEN/tests/architecture.rs` reads all `.rs` files under `VEN/src/routes/` and asserts none contain `use crate::profile` (or equivalent pattern). Rationale: runs with existing `cargo test`, no CI config changes, fails fast with clear diagnostic.

- **`AppCtx.profile` retention**: Retained. `sim_tick` task still requires it. Annotated with doc-comment. Removal deferred to Phase 4/5.

- **Startup error propagation**: `.expect("failed to build sim schema from profile: ...")` — panics at startup with descriptive message. Consistent with idiomatic fatal-init errors in this codebase.

- **Schema identity test**: Cargo integration test in `VEN/tests/schema_snapshot.rs`. Calls `schema_from_profile` directly (function must be `pub`), serialises to JSON, and compares against a committed fixture. `schema_from_profile` visibility change from `pub(crate)` to `pub` is required to make it accessible from integration test crate.

**Output**: `research.md` — no open questions.

## Phase 1: Design & Contracts

### Data Model Changes

#### AppCtx — modified struct (`VEN/src/main.rs`)

| Field | Type | Change | Notes |
|-------|------|--------|-------|
| `profile` | `Arc<Profile>` | Retain | Add doc-comment: "Used by sim_tick task; removal deferred to Phase 4/5" |
| `sim_schema` | `Arc<HashMap<String, Vec<ControlDescriptor>>>` | **ADD** | Pre-computed once at startup from profile |

No other struct or entity changes.

#### `schema_from_profile` visibility (`VEN/src/simulator/mod.rs`)

| Item | Before | After | Reason |
|------|--------|-------|--------|
| `schema_from_profile` | `pub(crate)` | `pub` | Integration test in `VEN/tests/` cannot access `pub(crate)` items |

### Contracts

`GET /sim/schema` response shape is unchanged. The contract is already defined by the existing BDD suite. No new contract documents required.

### Implementation Notes

**`VEN/src/main.rs`**:
```rust
// Add to imports:
use std::collections::HashMap;
use crate::assets::ControlDescriptor;

// In AppCtx struct, after profile:
/// Used by sim_tick task; profile removal from AppCtx is deferred to Phase 4/5.
pub profile: Arc<Profile>,
pub sim_schema: Arc<HashMap<String, Vec<ControlDescriptor>>>,

// At AppCtx construction (after profile is built):
let sim_schema = Arc::new(
    simulator::schema_from_profile(&profile)
        .expect("failed to build sim schema from profile")  // or handle the return type
);
let ctx = AppCtx {
    ...
    sim_schema,
    ...
};
```

Note: `schema_from_profile` currently returns `HashMap` directly (no `Result`), so `.expect()` is not needed on the call — it infallibly builds. The startup guard is implicit via profile YAML parsing which happens earlier and already fails fast.

**`VEN/src/routes/sim.rs`**:
```rust
pub async fn get_sim_schema(State(ctx): State<AppCtx>) -> impl IntoResponse {
    debug!("GET /sim/schema");
    Json((*ctx.sim_schema).clone())
}
```

**`VEN/tests/architecture.rs`**:
```rust
#[test]
fn routes_must_not_import_profile() {
    let routes_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/routes");
    for entry in walkdir::WalkDir::new(&routes_dir) { ... }
    // or use std::fs::read_dir recursively — no new deps
}
```
Use `std::fs` only — no new crate dependencies.

**`VEN/tests/schema_snapshot.rs`**:
```rust
// Load test profile fixture, call schema_from_profile, serialize, compare to committed JSON.
```
A committed fixture JSON at `VEN/tests/fixtures/schema_snapshot.json` captures the expected output.

### Agent Context Update

Run after Phase 1 to update agent context:
```powershell
.specify/scripts/powershell/update-agent-context.ps1 -AgentType claude
```
No new technologies to add; existing Rust entry covers this change.

