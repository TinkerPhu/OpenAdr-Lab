# Checklist: Refactor - Split loops.rs into tasks/

Created: 2026-05-08
Purpose: Requirements-quality checklist for Phase 1 refactor (Unit tests for requirements)
Generated-by: speckit.checklist

## Requirement Completeness
- [ ] CHK001 - Are all spawn_* functions enumerated and mapped to target task files (poll_events, poll_programs, poll_reports, obligation, planning, sim_tick, state_persist)? [Completeness, Spec §FR-002]
- [ ] CHK002 - Are all #[cfg(test)] test modules and test functions from loops.rs identified and assigned to their owning task files? [Completeness, Spec §FR-007]
- [ ] CHK003 - Is the location and visibility of shared helper functions specified (tasks/shared.rs with pub(crate)) and are examples provided? [Completeness, Spec §FR-003a]
- [ ] CHK004 - Is the 200-line production-code file-size rule documented, including the exclusion of #[cfg(test)] modules? [Completeness, Spec §FR-006]

## Requirement Clarity
- [ ] CHK005 - Is 'preserve existing locking semantics exactly' defined with enough precision to verify (which locks, expected ordering, and verification method)? [Clarity, Spec §NFR-001]
- [ ] CHK006 - Is the phrase 'readable in full without scrolling' tied to the 200-line rule or otherwise quantified for reviewer verification? [Clarity, Spec §US1]
- [ ] CHK007 - Are the acceptance criteria for 'no behaviour changes' measurable and linked to specific checks (unit test counts, BDD scenario count)? [Clarity, Spec §SC-001, SC-004]

## Requirement Consistency
- [ ] CHK008 - Do FR-004 (re-exports in tasks/mod.rs) and the migration approach together guarantee that main.rs requires only a module path change with zero logic edits? [Consistency, Spec §FR-004 & SC-005]
- [ ] CHK009 - Is the exclusion of tests from the 200-line cap consistent with the 'one file per concern' readability goal, or does it create contradictory expectations? [Consistency, Spec §FR-006/FR-007]

## Acceptance Criteria Quality
- [ ] CHK010 - Is the baseline test-recording procedure for cargo test explicitly specified (commands, flags, and where to record the passing count)? [Measurability, Spec §SC-001]
- [ ] CHK011 - Is the final BDD gate (232 scenarios on Pi4-Server) operationally described (exact commands, environment variables, and runbook)? [Measurability, Spec §SC-004]

## Scenario Coverage
- [ ] CHK012 - Is there a documented rollback/revert procedure if a migration step causes test regressions (git commands, how to revert the single move)? [Coverage, Quickstart]
- [ ] CHK013 - Are the unit test and BDD subset selections defined for incremental verification after each moved file? [Coverage, Quickstart]
- [ ] CHK014 - Are concurrency/timing risk areas explicitly marked out-of-scope for Phase 1 and tracked as separate follow-up work? [Coverage, Spec §NFR-001 & research.md]

## Edge Case Coverage
- [ ] CHK015 - Does the spec define how to split sim_tick into a submodule (tasks/sim_tick/mod.rs) if production code exceeds 200 lines? [Edge Case, Spec §FR-006]
- [ ] CHK016 - Are exceptional call sites such as spawn_report_poll() (not in primary inventory) explicitly included in the migration checklist? [Edge Case, Spec 'Edge Cases']

## Non-Functional Requirements
- [ ] CHK017 - Are performance/latency constraints for background loops declared as preserved and is a measurement method specified to detect regressions? [NFR, Spec §NFR-001]
- [ ] CHK018 - Are Pi4 test environment instructions and dependencies documented for reproducible final verification (DOCKER_HOST, run_all_tests.sh usage)? [NFR, Quickstart]

## Dependencies & Assumptions
- [ ] CHK019 - Are assumptions about openleadr-rs submodule handling (clone --recursive) and SQLx offline caching documented as prerequisites? [Assumption, Build & Deploy]
- [ ] CHK020 - Is the requirement that main.rs edits are limited to module path changes enforced by code review criteria? [Dependency, Spec §SC-005 & FR-004]

## Ambiguities & Conflicts
- [ ] CHK021 - Are there ambiguous terms (e.g., 'readable', 'preserve semantics') that need disambiguation before migration begins? [Ambiguity, Spec §US1/FR-006]

## Traceability & Documentation
- [ ] CHK022 - Is there a requirement ID and acceptance-criteria traceability scheme referenced by the spec and checklist items? [Traceability]
- [ ] CHK023 - Are quickstart.md and plan.md updated with the concrete commands and verification steps for each migration step? [Completeness, Quickstart]

## Surface & Resolve Issues
- [ ] CHK024 - Is the procedure for updating shared helpers during migration specified (compatibility wrappers, atomic edits, or staged updates)? [Assumption, Spec §FR-003a]

--
Notes:
- Focus: migration/navigability, test co-location, behaviour preservation
- Depth: Standard (PR-review checklist)
- Actor: Reviewer (PR)
