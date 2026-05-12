# Feature Specification: Deterministic Test Environment for MILP-Backed BDD Tests

**Feature Branch**: `022-deterministic-test-env`
**Created**: 2026-05-12
**Status**: Draft
**Builds on**: `021-decouple-profile-domain`

## Clarifications

### Session 2026-05-12

- Q: Should the BDD step for `pv_plan_kw` extend the existing irradiance step, use a new combined step, or be a new independent step? → A: New independent step: `I set pv plan forecast to {value} kW` — composable, single-responsibility, usable in any scenario regardless of irradiance setting.
- Q: What numeric threshold defines "near-zero pre-discharge" in the battery plan when `pv_plan_kw=0.0` is active? → A: ≤ 0.1 kW (strict near-idle tolerance).
- Q: How is `pv_plan_kw` cleared mid-scenario? → A: Send `null` (or omit the field) in the inject payload — consistent with how all other optional override fields behave. Additionally: `pv_plan_kw` must be adopted across ALL MILP-backed BDD feature files, not only `deviation_absorber.feature`, to eliminate suite-wide time-of-day non-determinism.

## Background

BDD scenarios that exercise MILP-backed HEMS behaviour (battery dispatch, deviation absorber, planner triggers) are non-deterministic because the planner's 24-hour PV forecast is derived from the real system clock. When tests inject `pv_irradiance=0.0` to zero out current-tick PV power, the planning horizon still sees the natural sin-model irradiance curve, so at solar-prep hours the planner pre-discharges the battery and leaves insufficient headroom for assertions. The same clock sensitivity affects any MILP-backed scenario that does not explicitly fix the forecast.

This feature introduces an opt-in PV forecast override (`pv_plan_kw`) that replaces the time-varying PV forecast with a constant value for the entire planning horizon. It is adopted broadly across **all** MILP-backed BDD feature files, making every planner-sensitive scenario produce identical results regardless of time of day and eliminating suite-wide non-determinism.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Freeze PV Forecast for BDD Scenarios (Priority: P1)

A test author writing a BDD scenario that relies on stable battery headroom can declare a fixed PV forecast value alongside the existing irradiance override. Once set, every MILP solve during that scenario produces the same battery plan regardless of when the test runs.

**Why this priority**: This directly unblocks the `deviation_absorber.feature` transient-deviation scenario that is currently marked `@wip` and fails non-deterministically at peak solar-prep hours. It is the core deliverable of this feature.

**Independent Test**: Can be fully tested by running `deviation_absorber.feature` at different times of day (morning, afternoon, evening) and confirming the scenario tagged `DeviceDeviation does not fire for transient deviations` passes consistently in all three runs without the `@wip` tag.

**Acceptance Scenarios**:

1. **Given** a BDD scenario injects `pv_irradiance=0.0` and `pv_plan_kw=0.0`, **When** the MILP planner solves (at any time of day), **Then** the battery plan shows pre-discharge ≤ 0.1 kW, and available headroom is ≥ 1.5 kW.
2. **Given** the PV forecast override is set to `0.0`, **When** a second MILP solve is triggered within the same scenario (race-condition path), **Then** the resulting plan is identical to the first solve (no net change to battery headroom).
3. **Given** `pv_plan_kw=0.0` is active, **When** the `DeviceDeviation does not fire for transient deviations` scenario runs, **Then** it passes without the `@wip` tag.

---

### User Story 2 - Preserve Existing Scenarios (Priority: P2)

All BDD scenarios that were passing before this change continue to pass. The PV forecast override is entirely opt-in — scenarios that do not set it are unaffected.

**Why this priority**: Non-regression is a hard requirement. The override must not interfere with production physics behaviour or scenarios that intentionally test time-varying dispatch.

**Independent Test**: Can be tested by running the full BDD suite (excluding the newly unblocked `@wip` scenario) and confirming zero regressions.

**Acceptance Scenarios**:

1. **Given** no `pv_plan_kw` value is set in a scenario, **When** the MILP planner solves, **Then** it uses the existing natural irradiance + decay model unchanged.
2. **Given** the `EV departure guard prevents reduction near departure` scenario runs with `pv_plan_kw` present in the Background, **When** the planner solves, **Then** the EV departure guard behaviour is unaffected and the scenario passes.
3. **Given** the full BDD suite runs, **When** `pv_plan_kw=0.0` is in effect for all audited MILP-backed feature Backgrounds (as applied in this user story), **Then** all previously-passing scenarios continue to pass.

---

### User Story 3 - Override Does Not Trigger Replanning (Priority: P3)

Setting or changing the `pv_plan_kw` value via the simulation inject endpoint does not cause an immediate MILP replan. The override applies only on the next scheduled or event-triggered solve.

**Why this priority**: Triggering an unintended replan when the override is set would invalidate the assertion window in timing-sensitive BDD steps. Consistent with how the existing `base_load_kw` override behaves.

**Independent Test**: Can be tested by injecting `pv_plan_kw` and confirming no replan event is emitted within 2 seconds.

**Acceptance Scenarios**:

1. **Given** the system is idle (no pending replan), **When** `pv_plan_kw` is injected via the sim override endpoint, **Then** no replan is triggered within 2 seconds.
2. **Given** `pv_plan_kw` is already set, **When** a tariff event or other normal replan trigger fires, **Then** the subsequent solve uses the overridden forecast value.

---

### Edge Cases

- What happens when `pv_plan_kw` is set to a negative value? → The value should be clamped to 0.0 (PV cannot consume power).
- What happens when `pv_plan_kw` is set while a MILP solve is already in progress? → The in-progress solve completes with the previous forecast; the override applies from the next solve onward.
- What happens when `pv_plan_kw` is cleared (reset to unset)? → Sending `null` or omitting the field in the inject payload clears the override; the planner reverts to the natural irradiance model for subsequent solves.
- What happens when both `pv_irradiance` and `pv_plan_kw` are set simultaneously? → They operate on different rings: `pv_irradiance` affects the current physics tick; `pv_plan_kw` replaces the planning forecast. Both can coexist independently.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: The simulation inject state MUST support an optional fixed PV forecast power value (in kW) that overrides the time-varying irradiance model for MILP planning purposes.
- **FR-002**: When the PV forecast override is set, the planner MUST use that constant value for every slot across the full planning horizon.
- **FR-003**: When the PV forecast override is not set, the planner MUST use the existing natural irradiance + decayed offset model unchanged.
- **FR-004**: The simulation inject endpoint MUST accept the PV forecast override field alongside existing override fields; sending `null` or omitting the field MUST clear the override and revert the planner to the natural irradiance model.
- **FR-005**: Setting the PV forecast override MUST NOT trigger an immediate replan.
- **FR-006**: ALL MILP-backed BDD feature files MUST set `pv_plan_kw=0.0` in their Background (or per-scenario where applicable) to eliminate time-of-day non-determinism across the entire suite. The `deviation_absorber.feature` Background is the primary instance; all other features whose scenarios depend on MILP battery dispatch must be audited and updated in the same change. Apply at Background level when all scenarios in the file exercise MILP battery dispatch; apply per-scenario only in files where some scenarios intentionally test time-varying dispatch.
- **FR-007**: The BDD step vocabulary MUST provide a new independent step `I set pv plan forecast to {value} kW` that sets only `pv_plan_kw`, composable alongside any other inject step.
- **FR-008**: The unit test suite MUST pass without failures after these changes.

### Key Entities

- **SimInjectState**: The in-memory structure holding all test-time overrides for VEN physics and planning. Gains an optional `pv_plan_kw` field that the MILP planner reads when building its input forecast.
- **PV Forecast Override**: An optional constant power value (kW, ≥ 0) that, when present, replaces the irradiance-derived per-slot PV generation estimate across the entire planning horizon.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: The `DeviceDeviation does not fire for transient deviations` scenario passes in at least 3 consecutive runs executed at different times of day (morning, afternoon, evening), with no `@wip` tag.
- **SC-002**: Battery pre-discharge is ≤ 0.1 kW and available headroom is ≥ 1.5 kW during the transient-deviation assertion window in all runs where `pv_plan_kw=0.0` is in effect, regardless of time of day.
- **SC-003**: Zero regressions in all BDD scenarios that were passing before this change.
- **SC-004**: The unit test suite reports zero failures after the changes are applied.
- **SC-005**: The PV forecast override field is present in the inject endpoint contract and confirmed absent from all domain-ring types (the override lives exclusively in the infrastructure/inject layer).
- **SC-006**: Every MILP-backed BDD feature file has `pv_plan_kw=0.0` applied in its Background or per-scenario setup; no scenario that exercises battery dispatch is left without a fixed forecast override. The full BDD suite produces the same pass/fail result when run at any time of day.

## Assumptions

- The `pv_plan_kw` field is a test-only override. No UI, no persistence, no migration needed. It is carried in the same in-memory inject snapshot structure that already handles `base_load_kw`, `pv_irradiance`, and similar overrides.
- An audit of all BDD feature files is required to identify every scenario that exercises MILP battery dispatch; `pv_plan_kw=0.0` must be applied to each.
- The `EV departure guard prevents reduction near departure` scenario (in `deviation_absorber.feature`, line 106) is covered by the `deviation_absorber.feature` Background already receiving `pv_plan_kw=0.0`; no separate feature file audit is required.
- Clock-based non-determinism from EV departure slot positions and tariff event window evaluation is out of scope for this feature (smaller effects, deferred to a later change).
- `POST /plan/inject` (bypassing MILP entirely) is out of scope; deferred until Phase 5 PlanningService is in place.

## Out of Scope

- Changes to production PV tracking logic (`pv_irradiance` / `irradiance_offset` / decay mechanism).
- A virtual or mocked time system.
- Any new persistence, database migration, or UI surface.
- General-purpose plan injection (bypassing the MILP solver).
