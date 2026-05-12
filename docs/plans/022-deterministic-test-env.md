# Plan: 022 — Deterministic Test Environment for MILP-Backed BDD Tests

## Context

The BDD suite contains scenarios that are non-deterministic because the MILP
planner's 24-hour forecast uses the real system clock. The primary cause is the
PV forecast: injecting `pv_irradiance=0.0` zeros PV for the current physics
tick, but the irradiance offset decays back to the natural sin-model across the
24-hour planning horizon. At solar-prep hours (late afternoon), the MILP
pre-discharges the battery to make room for tomorrow's PV, leaving insufficient
headroom for absorber tests. The same non-determinism affects any MILP planner
test that does not set up a tariff event — the battery dispatch strategy changes
with time of day.

Two concrete failures documented in `021-decouple-profile-domain`:

1. `deviation_absorber.feature:149` — `DeviceDeviation does not fire for
   transient deviations` (marked `@wip`): MILP plans battery at up to −4.175 kW
   (close to max_discharge_kw=5.0); absorber headroom is 0.825 kW < 1.5 kW
   required by the assertion. Fails at peak solar-prep hours.

2. The same race condition (trigger T1 from Background pv_irradiance inject +
   trigger T2 from explicit plan/trigger) causes a second MILP solve that
   invalidates the assertion window — but even fixing the race, the headroom
   issue persists independently.

## What Needs to Be Built

### 1. `pv_plan_kw` — fixed PV forecast override for MILP

A new field on `SimInjectState` that, when set, replaces the decaying
irradiance model in `milp_planner/inputs.rs` with a constant value for **all**
24 horizon slots.

**Rust changes:**
- `VEN/src/state.rs`: add `pub pv_plan_kw: Option<f64>` to `SimInjectState`
- `VEN/src/routes/sim.rs`: add `pv_plan_kw` to `PostSimInjectBody`; add to
  `merge_inject`; exclude from `should_replan` (same reasoning as
  `base_load_kw` — test-only override, not a MILP input change)
- `VEN/src/controller/milp_planner/inputs.rs`: in the `pv_kw` calculation,
  check `inject_snap.pv_plan_kw` first:
  ```rust
  let pv_kw = if let Some(forced_kw) = inject_snap.pv_plan_kw {
      forced_kw  // constant for all slots — deterministic
  } else {
      // existing natural + decayed_offset logic
      (natural + decayed_offset).clamp(0.0, 1.0) * rated_kw
  };
  ```
- `VEN/src/tasks/planning.rs`: pass `inject_snap.pv_plan_kw` into the planner
  (it already passes the full inject snapshot, so this is likely zero-change
  once the field exists on the struct)

**BDD change:**
- `tests/features/steps/phase_a_physics_steps.py`: update
  `step_given_inject_pv_irradiance` to also set `pv_plan_kw` when a zeroing
  inject is detected, OR add a new step `I inject pv irradiance {v} with zero
  plan forecast` that sets both fields
- `tests/features/deviation_absorber.feature`: update Background to set
  `pv_plan_kw=0.0` alongside `pv_irradiance=0.0`; remove `@wip` from
  `DeviceDeviation does not fire for transient deviations` and verify it passes

### 2. Verify the two coupled fixes resolve the scenario

Once `pv_plan_kw=0.0` is in effect:
- Every MILP solve (T1, T2, or periodic) produces the **same plan** for the
  battery — near 0 kW (no PV pre-discharge incentive, flat default tariffs)
- The race condition (T1+T2 back-to-back solves) becomes harmless: T2's plan
  is identical to T1's plan
- Battery headroom is ~5.0 kW, well above the 1.5 kW assertion threshold

### 3. EV departure guard scenario (deviation_absorber.feature:106)

This scenario (`EV departure guard prevents reduction near departure`) also
uses `I wait for a fresh plan`. Verify it continues to pass with `pv_plan_kw`
in effect — no behavioural change expected since the EV departure guard test
does not depend on battery discharge headroom.

## What This Is NOT

- Not a change to how production PV tracking works. The `pv_irradiance` /
  `irradiance_offset` / decay mechanism is unchanged. `pv_plan_kw` is an
  additional opt-in override used only when set.
- Not a virtual time system. Clock-based non-determinism from EV departure
  slot positions (minor) and tariff event window evaluation is not addressed
  here; those are smaller effects and can be tackled later.
- Not a plan injection endpoint. `POST /plan/inject` (bypassing MILP
  entirely for absorber tests) is a larger change deferred to when Phase 5
  (PlanningService) is in place.

## Acceptance Criteria

- `grep "pv_plan_kw" VEN/src/` matches `state.rs`, `routes/sim.rs`,
  `milp_planner/inputs.rs`
- BDD suite: `deviation_absorber.feature:149` (`DeviceDeviation does not fire
  for transient deviations`) passes without `@wip` tag in at least 3
  consecutive runs at different times of day
- All other scenarios that were passing before this change continue to pass
- `cargo test --workspace` passes with zero failures

## Architectural note

`pv_plan_kw` belongs in the infrastructure ring (`SimInjectState` lives in
`state.rs`), not the domain ring. The MILP planner reads it via the inject
snapshot passed at planning time — consistent with how all other inject overrides
flow. No domain type needs to change.

The field should not trigger a replan when changed (same as `base_load_kw`):
it is a forecast override for the *next* solve, not a state change that
requires immediate replanning.
