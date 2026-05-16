# Implementation Plan: Fix VtnClient References in Remaining Task Files

**Branch**: `028-fix-vtnclient-tasks` | **Date**: 2026-05-16 | **Spec**: [spec.md](spec.md)  
**Input**: Feature specification from `/specs/028-fix-vtnclient-tasks/spec.md`

## Summary

Replace the concrete `VtnClient` parameter type with `Arc<dyn VtnPort>` in four task-layer files
(`poll_programs.rs`, `poll_reports.rs`, `poll_events.rs`, `obligation.rs`) and update the four
corresponding `main.rs` spawn call sites to pass `vtn_port.clone()` instead of `vtn.clone()`.
This is a purely mechanical type substitution — no behavior changes, no new dependencies.
It satisfies Invariant 4: `grep -r "use crate::vtn::VtnClient" VEN/src/tasks` → empty.

## Technical Context

**Language/Version**: Rust stable 2021 edition  
**Primary Dependencies**: `tokio`, `axum`, `std::sync::Arc` (all existing — no new Cargo.toml entries)  
**Storage**: N/A — no persistence changes  
**Testing**: `cargo check` (local via WSL) + full BDD suite on Pi4-Server  
**Target Platform**: Linux ARM64 (Raspberry Pi 4), Docker Compose v2  
**Project Type**: Backend service (VEN — Virtual End Node)  
**Performance Goals**: N/A — no runtime path changes  
**Constraints**: `tasks/` files must stay under 200 lines (none of the changed files grow)  
**Scale/Scope**: 4 task files + 4 call sites in `main.rs`

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-checked after Phase 1 design.*

| Principle | Status | Notes |
|-----------|--------|-------|
| I — OpenADR Spec Fidelity | ✅ Pass | No field names or API shapes changed |
| II — BDD-First Testing | ✅ Pass | No behavior change; acceptance test is the invariant grep + compile. Existing BDD suite covers runtime behavior. |
| III — Upstream Compatibility | ✅ Pass | No changes to `openleadr-rs/` submodule |
| IV — Lean Architecture | ✅ Pass | Mechanical substitution; no new abstractions introduced |
| V — Infrastructure Parity | ✅ Pass | Full test gate runs on Pi4-Server via Docker |
| VI — VEN Hexagonal Architecture | ✅ Pass | This change **enforces** Principle VI: task files must not import infra (`VtnClient`). Removing the concrete dependency is exactly the point of the port abstraction. |

No violations → Complexity Tracking table not required.

## Project Structure

### Documentation (this feature)

```text
specs/028-fix-vtnclient-tasks/
├── plan.md              # This file
├── research.md          # Phase 0 — no unknowns (see below)
├── data-model.md        # Not generated — pure refactor, no new entities
├── contracts/           # Not generated — no interface shape changes
└── tasks.md             # Phase 2 output (/speckit.tasks command)
```

### Source Code (files changed)

```text
VEN/src/
├── main.rs                        # 4 spawn call sites: vtn.clone() → vtn_port.clone()
└── tasks/
    ├── poll_programs.rs           # param + import + cast removal
    ├── poll_reports.rs            # param + import + cast removal
    ├── poll_events.rs             # param + import + cast removal
    └── obligation.rs              # param + import + cast removal + pass vtn.as_ref()
```

## Phase 0: Research

No unknowns exist. The change is fully specified in `docs/plans/post_refactoring_fixes.md` (Item 1)
and confirmed by reading the four source files. All findings are documented inline below.

**research.md** is generated as a minimal record.

### Findings from source inspection

**poll_programs.rs** (38 lines):
- Imports: `use crate::vtn::VtnClient` (line 7), `use crate::controller::VtnPort` (line 5)
- Spawn fn: `spawn_program_poll(state: AppState, vtn: VtnClient, secs: u64)`
- Loop body: `let vtn_port: &dyn VtnPort = &vtn;` then `vtn_port.fetch_programs().await`
- No `use std::sync::Arc` → must be added
- Cast variable `vtn_port` conflicts with new param name — remove cast, call `vtn.fetch_programs()` directly

**poll_reports.rs** (33 lines):
- Same structure as poll_programs.rs
- `spawn_report_poll(state, vtn: VtnClient, secs)` and cast `let vtn_port: &dyn VtnPort = &vtn;`
- No `use std::sync::Arc` → must be added

**poll_events.rs** (341 lines — well within 500 limit):
- `use std::sync::Arc` already present (line 3)
- `use crate::vtn::VtnClient` at line 12
- `spawn_event_poll(state, vtn: VtnClient, secs, trigger_tx)` at line 117–122
- Loop body: `let vtn_port: &dyn VtnPort = &vtn;` then `vtn_port.fetch_events().await` at line 132–133

**obligation.rs** (tasks, 31 lines):
- `use crate::vtn::VtnClient` at line 11; NO `use crate::controller::VtnPort`
- `use std::sync::Arc` already present (line 4)
- `spawn_obligation_check(state, sim: Arc<Mutex<SimState>>, vtn: VtnClient, ven_name: String)`
- Body calls: `ObligationService::check_and_report(&state, &sim, &vtn, &ven_name, now).await`
- `check_and_report` in services/obligation.rs accepts `vtn: &dyn VtnPort` (verified)
- After change: pass `vtn.as_ref()` (coerces `Arc<dyn VtnPort>` → `&dyn VtnPort`)
- Must add `use crate::controller::VtnPort`

**main.rs** (spawn sites):
- `vtn_port: Arc<dyn VtnPort>` constructed at line 162 (already exists)
- `vtn.clone()` at lines 188, 191, 195, 216 → change to `vtn_port.clone()`
- `vtn` at line 242 (`AppCtx { vtn, ... }`) → remains as `VtnClient` (used in routes)
- No unused-variable warning after change: `vtn` still consumed at line 242

## Phase 1: Design & Contracts

No data model or contracts generated — this is a pure refactoring. The port trait `VtnPort`
and the concrete `VtnClient` already exist; their shapes do not change.

### Change map (file-by-file)

#### `VEN/src/tasks/poll_programs.rs`

Remove:
```rust
use crate::vtn::VtnClient;
```

Add after existing imports:
```rust
use std::sync::Arc;
```

Change function signature:
```rust
// Before
pub(crate) fn spawn_program_poll(state: AppState, vtn: VtnClient, secs: u64)
// After
pub(crate) fn spawn_program_poll(state: AppState, vtn: Arc<dyn VtnPort>, secs: u64)
```

Replace inside loop:
```rust
// Before
let vtn_port: &dyn VtnPort = &vtn;
match vtn_port.fetch_programs().await {
// After
match vtn.fetch_programs().await {
```

#### `VEN/src/tasks/poll_reports.rs`

Same pattern as poll_programs.rs:
- Remove `use crate::vtn::VtnClient`
- Add `use std::sync::Arc`
- `vtn: VtnClient` → `vtn: Arc<dyn VtnPort>`
- Remove cast; `vtn_port.fetch_reports_raw()` → `vtn.fetch_reports_raw()`

#### `VEN/src/tasks/poll_events.rs`

Remove:
```rust
use crate::vtn::VtnClient;
```

Change function signature:
```rust
// Before
pub(crate) fn spawn_event_poll(state: AppState, vtn: VtnClient, secs: u64, trigger_tx: ...)
// After
pub(crate) fn spawn_event_poll(state: AppState, vtn: Arc<dyn VtnPort>, secs: u64, trigger_tx: ...)
```

Replace inside loop (lines 132–133):
```rust
// Before
let vtn_port: &dyn VtnPort = &vtn;
match vtn_port.fetch_events().await {
// After
match vtn.fetch_events().await {
```

(`Arc` already imported; `VtnPort` already imported at line 8)

#### `VEN/src/tasks/obligation.rs`

Remove:
```rust
use crate::vtn::VtnClient;
```

Add:
```rust
use crate::controller::VtnPort;
```

Change function signature:
```rust
// Before
pub(crate) fn spawn_obligation_check(state: AppState, sim: Arc<Mutex<SimState>>, vtn: VtnClient, ven_name: String)
// After
pub(crate) fn spawn_obligation_check(state: AppState, sim: Arc<Mutex<SimState>>, vtn: Arc<dyn VtnPort>, ven_name: String)
```

Change call (pass arc as dyn ref):
```rust
// Before
ObligationService::check_and_report(&state, &sim, &vtn, &ven_name, now).await
// After
ObligationService::check_and_report(&state, &sim, vtn.as_ref(), &ven_name, now).await
```

#### `VEN/src/main.rs`

Four call site changes (pass `vtn_port.clone()` instead of `vtn.clone()`):

```rust
// Line 188 — Before
tasks::spawn_program_poll(state.clone(), vtn.clone(), cfg.poll_programs_secs);
// After
tasks::spawn_program_poll(state.clone(), vtn_port.clone(), cfg.poll_programs_secs);

// Lines 189–194 — Before
tasks::spawn_event_poll(state.clone(), vtn.clone(), cfg.poll_events_secs, trigger_tx.clone());
// After
tasks::spawn_event_poll(state.clone(), vtn_port.clone(), cfg.poll_events_secs, trigger_tx.clone());

// Line 195 — Before
tasks::spawn_report_poll(state.clone(), vtn.clone(), cfg.poll_reports_secs);
// After
tasks::spawn_report_poll(state.clone(), vtn_port.clone(), cfg.poll_reports_secs);

// Lines 213–218 — Before
tasks::spawn_obligation_check(state.clone(), sim_state.clone(), vtn.clone(), cfg.ven_name.clone());
// After
tasks::spawn_obligation_check(state.clone(), sim_state.clone(), vtn_port.clone(), cfg.ven_name.clone());
```

`vtn` remains in scope and is consumed at line 242 (`AppCtx { vtn, ... }`) — no dead-variable warning.

### Agent context update

Run after Phase 1 design is complete.

## Verification (post-implementation)

```bash
# Invariant 4 must be empty:
wsl bash -c "grep -r 'use crate::vtn::VtnClient' VEN/src/tasks"

# Compile check:
wsl cargo check --manifest-path VEN/Cargo.toml

# Full invariant suite:
wsl bash -c "grep 'use crate::simulator\|use crate::assets' VEN/src/controller/reporter.rs"
wsl bash -c "grep 'use crate::assets' VEN/src/controller/timeline.rs"
wsl bash -c "grep -r 'use crate::profile' VEN/src/tasks"
wsl bash -c "grep -r 'use crate::vtn::VtnClient' VEN/src/tasks"
wsl bash -c "grep -r 'use crate::assets\|use crate::simulator' VEN/src/services"

# BDD suite on Pi4-Server:
ssh Pi4-Server "cd /srv/docker/openadr_lab && docker compose run --rm ven-test 2>&1 | tail -30"
```
