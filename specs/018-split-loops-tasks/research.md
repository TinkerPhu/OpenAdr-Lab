# research.md

Decisions made during speckit.clarify (Session 2026-05-08)

1. Decision: Preserve existing locking semantics exactly
   - Rationale: Structural refactor must not change runtime behaviour or introduce timing/ordering changes that could create regressions. Concurrency improvements are deferred to a separate phase where they can be validated independently.
   - Alternatives considered: Implement snapshot-and-release for sim mutex (rejected due to behavioral risk in Phase 1).

2. Decision: Exclude #[cfg(test)] test modules from the 200-line file limit
   - Rationale: Co-locating tests with their subject code improves developer ergonomics and avoids unnecessary file splitting. The 200-line cap applies to production code only.
   - Alternatives considered: Count all lines (would force splitting tests into separate files; rejected).

3. Decision: Place helpers used by multiple spawn_* functions in `tasks/shared.rs` (pub(crate))
   - Rationale: Avoid duplication while keeping helpers internal to the tasks module and aligned with the Lean Architecture principle.
   - Alternatives considered: Put shared helpers at `VEN/src/common` (rejected to keep scope local and minimize API surface).

4. Decision: Use incremental, file-by-file migration with compatibility re-exports in `tasks/mod.rs`
   - Rationale: Minimizes risk and allows continuous verification (unit tests + BDD subset) after each move. `loops.rs` is removed only after all tests pass and re-exports are in place.

Notes
- No unresolved NEEDS CLARIFICATION markers remain in the feature spec. All clarifications required for Phase 1 were collected and recorded in the spec's Clarifications section.
