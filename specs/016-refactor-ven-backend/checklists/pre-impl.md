# Pre-Implementation Checklist: VEN Backend Refactoring

**Purpose**: Implementer self-check — resolve all open requirements-quality issues identified by `speckit.analyze` before writing any code. Every item is a question about what is *written* in the spec/plan/tasks, not about runtime behaviour.
**Created**: 2026-04-29
**Feature**: [spec.md](../spec.md)
**Audience**: Implementer (author self-check, pre-code)
**Basis**: 13 findings from `speckit.analyze` run 2026-04-29 (1 CRITICAL, 4 HIGH, 5 MEDIUM, 3 LOW)

---

## Constitution Compliance

- [x] CHK001 - Is every significant implementation step covered by at least one task that updates `docs/history/project_journal.md`? No such task existed in tasks.md Phase 9. [Gap, Constitution §Dev-Workflow-1] — **RESOLVED**: T050 added to Phase 9.
- [x] CHK002 - Is every hard-won lesson from this refactoring covered by a task that records it to `docs/reference/KEY_LEARNINGS.md`? No such task existed in tasks.md. [Gap, Constitution §Dev-Workflow-2] — **RESOLVED**: T051 added to Phase 9.

---

## Requirement Consistency

- [x] CHK003 - Is the sub-struct name used in spec.md FR-013 (`SimState`) consistent with all downstream artifacts (plan.md, tasks.md T034/T036/T039, data-model.md), which uniformly use `ControllerSimState`? One of the two must be authoritative before T033 is started. [Inconsistency, Spec §FR-013 vs tasks.md:T034] — **RESOLVED**: spec.md FR-013 and Key Entities updated to use `ControllerSimState` with a note on the naming reason.
- [x] CHK004 - Does plan.md Summary still state that "`serde(flatten)` makes the `state.json` format transparent"? This description was superseded by the decision to use three separate `Arc<RwLock<T>>` with a `PersistedVenState` assembly helper (per FR-013). If so, the sentence must be corrected before implementation begins — the plan is the technical reference document consulted during the work. [Inconsistency, plan.md:Summary vs Spec §FR-013] — **RESOLVED**: plan.md Summary and Constraints updated.
- [x] CHK005 - Does plan.md Constraints still reference "FR-013 via `serde(flatten)`"? If so, is this corrected to describe the `PersistedVenState` helper approach that T041 will implement? [Inconsistency, plan.md:Constraints vs Spec §FR-013] — **RESOLVED**: plan.md Constraints updated; Principle IV note updated.
- [x] CHK006 - Does US5 acceptance scenario 4 ("Given a VEN YAML profile with `id: boiler`, When a user-request targets that ID, Then the dispatcher and planner behave consistently with the HEMS route") conflict with FR-008, which explicitly declares boiler dispatcher and planner propagation OUT OF SCOPE? [Scope Conflict, Spec §US5-SC4 vs Spec §FR-008] — **RESOLVED**: US5-SC4 rewritten to test only the HEMS session path acceptance via `ids::BOILER`; dispatcher/planner scope conflict removed.

---

## Requirement Clarity

- [x] CHK007 - Is the type of `session_type` in `UserRequest` explicitly documented in FR-002 or US2 acceptance criteria as `Option<SessionType>` or `SessionType` (non-optional)? US2 SC4 treats `session_type: None` as possible; tasks.md T004 stated "it should be non-Option". [Ambiguity, Spec §FR-002, US2-SC4 vs tasks.md:T004] — **RESOLVED**: Confirmed `session_type: Option<SessionType>` in `VEN/src/entities/user_request.rs`; T004 and T005 corrected to be definitive.
- [x] CHK008 - Does FR-007 ("All production call sites MUST reference these constants") explicitly state whether `default_asset_id_*()` free functions are included or exempted? tasks.md T022 previously exempted them. [Ambiguity, Spec §FR-007 vs tasks.md:T022-notes] — **RESOLVED**: FR-007 updated ("no exemptions for production call sites"); T022 exemption removed.
- [x] CHK009 - Is the exact `SessionType` variant name in US2 acceptance scenario 3 — `ShiftableLoad` or `Shiftable`? tasks.md T006 used `Shiftable`. [Terminology Drift, Spec §US2-SC3 vs tasks.md:T006] — **RESOLVED**: T006 corrected to `SessionType::ShiftableLoad` (confirmed in source).

---

## Requirement Completeness

- [x] CHK010 - Does FR-007's list of asset IDs include `"base_load"`? tasks.md T020 creates `ids::BASE_LOAD` but FR-007 did not enumerate it. [Gap, Spec §FR-007 vs tasks.md:T020] — **RESOLVED**: FR-007 updated to include `"base_load"`.
- [x] CHK011 - Are the three new types introduced by US6 (`ClearedInjectField`, `Setpoints`, `DeviationState`) specified anywhere in the design artifacts? [Gap, Spec §FR-011 vs tasks.md:T026-T028] — **RESOLVED**: data-model.md §5 added with field sketches for all three types.
- [x] CHK012 - Does FR-014 require an accessor audit step, not just adding the INVARIANT comment? [Completeness, Spec §FR-014 vs tasks.md:T036-T043] — **RESOLVED**: T043 extended with FR-014 accessor audit requirement.

---

## Acceptance Criteria Quality

- [x] CHK013 - Is SC-001's "zero new warnings vs pre-refactoring baseline" criterion measurable without a documented procedure for capturing the warning count? [Measurability, Spec §SC-001 vs tasks.md:T001] — **RESOLVED**: T001 updated with explicit `cargo check 2>&1 | tee` baseline capture command.
- [x] CHK014 - Is SC-007 ("lock contention eliminated") verifiable by a concrete documented step? [Measurability, Spec §SC-007] — **RESOLVED**: T045 extended with explicit SC-007 inspection step and commit message note.
- [x] CHK015 - Is T025 ("annotate phase boundaries") defined as producing a verifiable, committed artefact? [Measurability, tasks.md:T025] — **RESOLVED**: T025 rewritten to require committed inline `// PHASE N:` comments before T026 begins.

---

## Scenario Coverage

- [x] CHK016 - Is the literal-sweep grep command in T024 precise enough to avoid false positives from YAML profile file path strings? [Precision, tasks.md:T024-Independent-Test] — **RESOLVED**: T022 grep updated to add `| grep -v "\.yaml"` filter; Phase 6 Independent Test already had this filter.

---

## Notes

- All items derive directly from speckit.analyze findings (IDs C1, H1–H4, M1–M5, L1–L3).
- All 16 items resolved in a single remediation pass on 2026-04-29.
- Changes applied to: `spec.md` (FR-007, FR-013, Key Entities, US5-SC4), `plan.md` (Summary, Constraints, Principle IV), `tasks.md` (T001, T004, T005, T006, T022, T025, T043, T045, T050+T051 added), `data-model.md` (§4 rewritten, §5 added for US6 types).
- **All checklist items complete** — spec is now implementation-ready. Proceed with `/speckit.implement` or start T001.
