# Feature Specification: Fix VtnClient References in Remaining Task Files

**Feature Branch**: `028-fix-vtnclient-tasks`  
**Created**: 2026-05-16  
**Status**: Draft  
**Input**: User description: "item 1 of docs/plans/post_refactoring_fixes.md"

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Task Layer No Longer Depends on Concrete VTN Client (Priority: P1)

A developer working on the VEN backend can swap or mock the VTN connection in any task (poll_programs, poll_reports, poll_events, obligation) without touching the concrete `VtnClient` type. All four spawn functions accept the VTN capability as an abstract interface.

**Why this priority**: This is the primary architectural invariant being enforced. Until all four files are fixed, the invariant grep reports violations and the abstraction boundary is broken.

**Independent Test**: Run `grep -r "use crate::vtn::VtnClient" VEN/src/tasks` — result must be empty. Cargo compile must succeed.

**Acceptance Scenarios**:

1. **Given** the four task files each import `VtnClient` directly, **When** the refactor replaces the parameter type with `Arc<dyn VtnPort>` and removes the concrete import, **Then** the grep invariant returns no output and the codebase compiles without errors.
2. **Given** `main.rs` passes a concrete `vtn.clone()` to the four spawn sites, **When** the call sites are updated to pass `vtn_port.clone()` (already an `Arc<dyn VtnPort>`), **Then** all spawn calls are consistent with the port abstraction used by the planning and sim_tick tasks.

---

### User Story 2 - Internal Casts Removed from Task Closures (Priority: P2)

Inside each task's async closure, the intermediate cast `let vtn_port: &dyn VtnPort = &vtn;` is eliminated. Methods are called directly on the `Arc<dyn VtnPort>` without the manual dereference step.

**Why this priority**: Removing the intermediate cast is the direct consequence of receiving the abstract type at the boundary; it keeps the code idiomatic and avoids the confusion of holding a concrete type only to immediately cast it.

**Independent Test**: Inspect the four task files after the change — no intermediate cast variable to `&dyn VtnPort` should remain.

**Acceptance Scenarios**:

1. **Given** a task closure that previously called `let vtn_port: &dyn VtnPort = &vtn; vtn_port.fetch_programs().await`, **When** the parameter is already `Arc<dyn VtnPort>`, **Then** the call becomes `vtn.fetch_programs().await` directly.

---

### Edge Cases

- What if a task file imports `VtnClient` for reasons other than the spawn function parameter (e.g., a type alias or cfg block)? Each import must be verified and only the spawn-parameter use removed.
- What if `ObligationService::check_and_report` currently receives `&dyn VtnPort` — confirm the task-side call passes `vtn.as_ref()` or `&*vtn` correctly without double-borrow issues.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: `poll_programs.rs` spawn function MUST accept `vtn: Arc<dyn VtnPort>` instead of `vtn: VtnClient`, with `use crate::vtn::VtnClient` removed.
- **FR-002**: `poll_reports.rs` spawn function MUST accept `vtn: Arc<dyn VtnPort>` instead of `vtn: VtnClient`, with `use crate::vtn::VtnClient` removed.
- **FR-003**: `poll_events.rs` spawn function MUST accept `vtn: Arc<dyn VtnPort>` instead of `vtn: VtnClient`, with `use crate::vtn::VtnClient` removed.
- **FR-004**: `obligation.rs` spawn function MUST accept `vtn: Arc<dyn VtnPort>` instead of `vtn: VtnClient`, with `use crate::vtn::VtnClient` removed.
- **FR-005**: `main.rs` MUST pass `vtn_port.clone()` (the existing `Arc<dyn VtnPort>`) to all four spawn call sites instead of `vtn.clone()`.
- **FR-006**: After the change, `grep -r "use crate::vtn::VtnClient" VEN/src/tasks` MUST return empty output.
- **FR-007**: The codebase MUST compile without errors after all changes.

### Key Entities

- **VtnPort**: Trait defining the abstract VTN communication interface (`fetch_programs`, `fetch_events`, `fetch_reports_raw`, etc.). Lives in `controller/`.
- **VtnClient**: Concrete struct implementing `VtnPort`. Lives in `vtn.rs` (infra layer). Task files must not import it directly.
- **Arc\<dyn VtnPort\>**: The shared, heap-allocated trait object that tasks receive and call methods on.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: The invariant grep `grep -r "use crate::vtn::VtnClient" VEN/src/tasks` produces zero lines of output.
- **SC-002**: `cargo check` on the VEN crate completes with zero errors after all four files and `main.rs` are updated.
- **SC-003**: All four spawn functions have consistent signatures — no task file in `tasks/` receives a concrete VTN type.
- **SC-004**: No intermediate cast to `&dyn VtnPort` remains inside any of the four modified task closures.

## Assumptions

- `VtnPort` is already imported in each of the four task files (used for the now-redundant cast); only the `VtnClient` import needs removal.
- `main.rs` already constructs `vtn_port: Arc<dyn VtnPort>` for the planning and sim_tick tasks; the four additional spawn sites just need to use that same variable.
- No behavioral change is introduced — this is a purely mechanical type substitution at function boundaries.
