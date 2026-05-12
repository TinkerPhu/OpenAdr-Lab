# Research: Deterministic Test Environment for MILP-Backed BDD Tests

**Branch**: `022-deterministic-test-env`
**Date**: 2026-05-12
**Status**: Complete — no NEEDS CLARIFICATION items; all decisions resolved in spec + clarifications.

---

## Decision 1: Where does `pv_plan_kw` live in the architecture?

**Decision**: In `SimInjectState` (infrastructure ring, `VEN/src/state.rs`).

**Rationale**: `SimInjectState` is the established home for all test-time inject overrides (`pv_irradiance`, `base_load_kw`, `battery_soc`, etc.). The domain ring reads the planning inputs via `build_milp_inputs` — a plain parameter function with no `SimInjectState` import. The new field is passed as `Option<f64>` through the call chain: `planning.rs` → `run_planner` → `build_milp_inputs`. No domain type is touched.

**Alternatives considered**:
- Adding `pv_plan_kw` to `PvParams` (domain asset params) — rejected: domain params are constructed at startup from the YAML profile; they are not runtime-mutable inject overrides.
- Passing via `SimSnapshot` — rejected: `SimSnapshot` reflects current physics state, not planner overrides; contaminating it would blur the infrastructure/domain boundary.

---

## Decision 2: How is `pv_plan_kw` applied in `build_milp_inputs`?

**Decision**: In the `p_pv` per-slot loop, check `pv_forecast_override: Option<f64>` first. If `Some(kw)`, use `kw.max(0.0)` for every slot. Otherwise fall through to the existing `assets.assets.get("pv")` irradiance model.

**Rationale**: The override is a constant across all 288 slots, which is the desired test behaviour. Clamping to ≥ 0.0 in `inputs.rs` ensures no negative PV generation regardless of the injected value.

**Alternatives considered**:
- Patching `irradiance_offset` on the `SimSnapshot` clone (like `pv_irradiance` is patched in `planning.rs`) — rejected: this would require computing the correct offset to cancel the forecast for all 288 future slots, which is complex and fragile. A direct override parameter is simpler and more explicit.

---

## Decision 3: Should `pv_plan_kw` trigger a replan?

**Decision**: No. It is excluded from `should_replan` in `routes/sim.rs`.

**Rationale**: The `pv_plan_kw` is set during BDD test setup *before* the assertion window. Triggering a replan on inject would cause a T1+T2 double-solve race (the same race that makes scenario:149 flaky). This is identical reasoning to `base_load_kw`, which is also excluded from `should_replan`.

---

## Decision 4: BDD step shape

**Decision**: New independent step `I set pv plan forecast to {kw:f} kW`, registered in `tests/features/steps/phase_a_physics_steps.py`.

**Rationale**: Composable, single-responsibility, can be used in any feature's Background alongside any other inject step. Avoids modifying the existing `I inject pv irradiance {irradiance:f} via sim inject` step (which sets physics state, not the planning forecast).

---

## Decision 5: "Near-zero" threshold for BDD assertions

**Decision**: Battery pre-discharge ≤ 0.1 kW.

**Rationale**: The MILP solver with `pv_plan_kw=0.0` and flat tariffs produces approximately 0 kW discharge for every slot — there is no PV incentive to pre-charge space. The 0.1 kW threshold is strict enough to catch any regression that re-introduces a PV-driven dispatch signal, while tolerant enough to ignore solver floating-point rounding.

---

## Decision 6: Override reset mechanism

**Decision**: Sending `null` or omitting `pv_plan_kw` in the inject payload clears the override (same `merge_f64!` macro pattern used by all other `Option<f64>` fields).

**Rationale**: Consistent with all other `SimInjectState` optional fields. Requires no dedicated endpoint or step. BDD scenarios are reset via `/sim/inject/reset` in `after_scenario` anyway, so explicit clearing is rarely needed mid-scenario.

---

## Decision 7: Suite-wide adoption scope

**Decision**: Audit all BDD feature files for MILP battery-dispatch sensitivity and add `pv_plan_kw=0.0` to their Backgrounds where needed.

**Primary target identified**: `deviation_absorber.feature` — Background already sets `pv_irradiance=0.0`; add `I set pv plan forecast to 0.0 kW` alongside it. Remove `@wip` from scenario at line 149.

**Features requiring audit** (checked by searching for `battery`, `battery_soc`, `bat_dispatch`, and MILP-plan assertions):
- `deviation_absorber.feature` — primary target ✅ confirmed
- `use_cases.feature` — uses `pv_irradiance=1.0` (full PV), not forecast-zeroing; assess if dispatch assertions depend on plan headroom
- `ven_dispatcher.feature` — battery dispatch assertions present; assess time-of-day sensitivity
- `ven_planner.feature` — MILP planner scenarios; assess
- `ven_uc_normal.feature`, `ven_uc_stress.feature` — use-case end-to-end; assess
- `asset_forecast.feature`, `ven_timeline.feature` — forecast endpoint tests; may need consistent forecast

Detailed per-file assessment is an implementation task (not blocking the design).
