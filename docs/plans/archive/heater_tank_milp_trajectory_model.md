# Heater Tank MILP Trajectory Model

## Context

The current heater MILP uses an energy-budget approach: it sums total heater output over the horizon and checks `Σ P×dt ≥ e_req_kwh`. This is wrong in three ways:
1. It ignores heat losses during the planning horizon (the tank drains via `Q_dem` while the planner is working)
2. It cannot prevent overheating (no upper bound on tank state in the MILP)
3. There is no switching penalty, so the planner churns relay states freely

The fix introduces a per-step tank energy state `E[t]` (kWh above T_min) as a continuous variable. Thermal dynamics, upper/lower bounds, soft violation, and a switching cost are all formulated with **no new binaries** — only 3 new continuous variables per step. This keeps HiGHS solve time acceptable on Pi4 ARM64.

Reference: `docs/plans/heater_tank_milp_planning_model.md`

---

## Variable Count (n = 288 steps for 24 h at 5 min)

| Variable | Type | Count | Purpose |
|---|---|---|---|
| `z_heat_mid[t]` | binary | 288 | mid power tier — **existing, unchanged** |
| `z_heat_full[t]` | binary | 288 | full power tier — **existing, unchanged** |
| `z_heat_ready` | binary | 1 | deadline met flag (MayRun only) — **existing, kept** |
| `e_tank[t]` | continuous | 288 | usable tank energy above T_min [kWh] — **new** |
| `s_low[t]` | continuous ≥ 0 | 288 | below-min soft violation slack [kWh] — **new** |
| `sw[t]` | continuous ≥ 0 | 288 | switching indicator per step (sw[0] = 0 fixed) — **new** |

Total binaries: 577 (vs 577 before). Total new constraints: ~2300 additional (dynamics + bounds + switching).

---

## Key Design Decisions

- **`q_dem_kw` is a scalar constant** (not an array): `draw_kw + k_loss × (T_mid − ambient_temp_c)` where T_mid = (T_min + T_max) / 2. Simple conservative estimate; no weather forecast integration in v1.
- **M_low = 10.0 EUR/kWh** hardcoded — much larger than any tariff, so soft violations are always penalized out.
- **`switching_penalty_eur`** lives in `HeaterConfig` (relay wear is an asset property), not `PlannerConfig`.
- **HeaterTarget → always MustRun** (hard deadline constraint on `e_tank`). No `soft_deadline` field exists on `HeaterTarget`.
- **Autonomous → always MayRun**, no hard deadline. The soft violation penalty + tariff-driven dynamics handle recovery and opportunistic heating without a z_heat_ready reward. `z_heat_ready` is fixed to 0 in autonomous mode (reserved for future soft-deadline HeaterTarget).
- **`e_init_kwh`** can be negative when tank is below T_min — the `s_low[0]` slack absorbs it immediately.

---

## Thermal Model

```
E[t] = usable thermal energy above T_min [kWh]
E[0]  = (T_current − T_min) × thermal_mass        // may be negative
E_max = (T_max − T_min) × thermal_mass
q_dem = draw_kw + k_loss × ((T_min+T_max)/2 − ambient_temp_c)

dynamics:  E[t+1] = E[t] + (p_mid×z_mid[t] + p_full×z_full[t] − q_dem) × dt_h
upper:     E[t] ≤ E_max
soft low:  E[t] + s_low[t] ≥ 0,  s_low[t] ≥ 0
switching: sw[t] ≥ ±(z_mid[t] − z_mid[t−1]),  sw[t] ≥ ±(z_full[t] − z_full[t−1])
MustRun:   E[t_dead] ≥ e_target  (= (target_temp_c − T_min) × thermal_mass)
```

Objective additions: `M_low × Σ s_low[t]  +  lambda_sw × Σ sw[t]`

---

## Files to Modify

| File | Change |
|---|---|
| `VEN/src/profile.rs` | Add `switching_penalty_eur: Option<f64>` + `effective_switching_penalty()` to `HeaterConfig` |
| `VEN/src/assets/heater.rs` | Add `forecast_demand_kw()`, redesign `HeaterMilpContext/Vars/SolOutput`, update delegating methods on `Heater` |
| `VEN/src/controller/milp_planner.rs` | Update `MilpInputs`, `build_milp_inputs()`, `solve_milp()`, `SolveOutput`, `translate_to_plan()`, `build_plan_envelopes()` |
| `tests/features/ven_heater_tank.feature` | **NEW** — write first |
| `tests/steps/heater_tank_steps.py` | **NEW** — step definitions for new BDD scenarios |

**Do NOT touch:** `Heater` sim struct, `step_inner()`, `forecast()`, `HeaterState`, `HeaterMilpMode`, `HeaterTarget`, `milp_interactions.rs`, EV/battery/grid MILP code, dispatcher, routes, UI.

---

## Struct Definitions

### `HeaterMilpContext` (replaces current)

```rust
pub struct HeaterMilpContext {
    pub mode: HeaterMilpMode,
    pub t_dead_step: Option<usize>,   // None = no hard deadline (autonomous)
    pub p_mid_kw: f64,
    pub p_full_kw: f64,
    pub e_init_kwh: f64,              // (T_current − T_min) × thermal_mass; may be < 0
    pub e_max_kwh: f64,               // (T_max − T_min) × thermal_mass
    pub q_dem_kw: f64,                // constant per-step demand [kW]
    pub e_target_kwh: f64,            // tank energy at deadline; = e_max in autonomous
    pub lambda_sw_eur: f64,           // switching penalty [EUR/event]
}
```

### `HeaterMilpVars` (replaces current)

```rust
pub struct HeaterMilpVars {
    pub z_heat_mid: Vec<Variable>,   // binary, len = n
    pub z_heat_full: Vec<Variable>,  // binary, len = n
    pub z_heat_ready: Variable,      // binary (0 fixed in autonomous; MustRun path reserved)
    pub e_tank: Vec<Variable>,       // continuous [-e_max, e_max], len = n
    pub s_low: Vec<Variable>,        // continuous ≥ 0, len = n
    pub sw: Vec<Variable>,           // continuous ≥ 0, sw[0] fixed at 0, len = n
}
```

### `HeaterSolOutput` (replaces current)

```rust
pub struct HeaterSolOutput {
    pub z_heat_mid: Vec<f64>,
    pub z_heat_full: Vec<f64>,
    pub z_heat_ready: f64,
    pub e_tank_kwh: Vec<f64>,   // len = n
    pub s_low_kwh: Vec<f64>,    // len = n
    pub sw: Vec<f64>,           // len = n
}
```

---

## New `MilpInputs` Fields (heater section)

Remove: `e_heat_req_kwh: f64`

Add:
```rust
e_heat_init_kwh: f64,       // (T_current − T_min) × thermal_mass
e_heat_max_kwh: f64,        // (T_max − T_min) × thermal_mass
q_heat_dem_kw: f64,         // draw + k_loss × (T_mid − ambient)
e_heat_target_kwh: f64,     // energy at deadline (= e_heat_max_kwh in autonomous)
lambda_heat_sw_eur: f64,    // from HeaterConfig::effective_switching_penalty()
```

`w_tier_penalty_eur` and all other planner weight fields: unchanged.

---

## Constraints Detail (inside `HeaterMilpContext::constraints()`)

```
C0  z_mid[t] + z_full[t] ≤ 1                          for all t   (288 constraints, existing)
C1  e_tank[0] == e_init_kwh                            (2 inequalities, pin initial state)
C2  e_tank[t+1] == e_tank[t]                           for t in 0..n-1
        + (p_mid×dt_h)×z_mid[t]
        + (p_full×dt_h)×z_full[t]
        − q_dem×dt_h                                   (2×287 inequalities)
C3  e_tank[t] ≤ e_max                                  for all t   (288)
C4  e_tank[t] + s_low[t] ≥ 0                           for all t   (288)
C5  sw[t] ≥  (z_mid[t] − z_mid[t−1])                  for t in 1..n
    sw[t] ≥ −(z_mid[t] − z_mid[t−1])
    sw[t] ≥  (z_full[t] − z_full[t−1])
    sw[t] ≥ −(z_full[t] − z_full[t−1])                (4×287 = 1148 constraints)
C6  MustRun:  e_tank[t_dead] ≥ e_target                (1 constraint)
    MayRun:   e_tank[t_dead] ≥ e_target × z_heat_ready (1 constraint; linear since e_target is scalar)
```

---

## `build_milp_inputs()` Heater Block (updated logic)

```
thermal_mass = heater.thermal_mass_kwh_per_c  (from SimState live config)
T_min        = cfg.temp_min_c
T_max        = cfg.temp_max_c
T_mid        = (T_min + T_max) / 2.0
ambient      = assets.heater_config().map(|h| h.ambient_temp_c).unwrap_or(10.0)
draw         = cfg.effective_draw_kw()
k_loss       = cfg.effective_k_loss()

e_init       = (T_current − T_min) × thermal_mass       // can be negative
e_max        = (T_max − T_min) × thermal_mass
q_dem        = draw + k_loss × (T_mid − ambient)
lambda_sw    = cfg.effective_switching_penalty()

HeaterTarget path:
  e_target = ((target.target_temp_c − T_min) × thermal_mass).clamp(0.0, e_max)
  mode     = MustRun   (HeaterTarget has no soft_deadline field)
  t_dead   = Some(deadline_to_step(target.ready_by, now, step_s, n))

Autonomous path:
  e_target = e_max
  mode     = MayRun
  t_dead   = None      (no deadline constraint added to MILP)
```

---

## `profile.rs` Change

```rust
pub struct HeaterConfig {
    // ... existing fields unchanged ...
    #[serde(default)]
    pub switching_penalty_eur: Option<f64>,
}

impl HeaterConfig {
    pub fn effective_switching_penalty(&self) -> f64 {
        self.switching_penalty_eur.unwrap_or(0.01)
    }
}
```

---

## `SolveOutput` Addition

```rust
e_heat_tank_kwh: Vec<f64>,   // len = n; empty ([]) when heater absent
```

`translate_to_plan()`: heat_kw formula unchanged (`z_mid×p_mid + z_full×p_full`).  
`build_plan_envelopes()`: replace `inputs.e_heat_req_kwh` → `inputs.e_heat_target_kwh`.

---

## Implementation Order (test-first)

### Step 1 — BDD feature file (write first, before any Rust changes)

**File:** `tests/features/ven_heater_tank.feature`

```gherkin
Feature: Heater tank MILP trajectory model

  Background:
    Given ven-2 profile (hot water tank: 200 L, max_kw=6, mid_kw=3, T_min=40, T_max=80)

  Scenario: Tank near T_max gets no heater allocation
    Given the heater temperature is injected to 79.0
    When I wait for the planner to run
    Then the plan has zero or one heater allocation slots in the first 6 slots

  Scenario: Tank at T_min triggers full-power recovery in plan
    Given the heater temperature is injected to 40.0
    When I wait for the planner to run
    Then the first heater allocation slot has power_kw 6.0

  Scenario: Cheap tariff window attracts heater scheduling
    Given the heater temperature is injected to 55.0
    And there is a PRICE event at 0.05 EUR/kWh for the next 3 hours
    When I wait for the planner to run
    Then the heater is allocated in at least one of the first 36 plan slots
```

**File:** `tests/steps/heater_tank_steps.py` — new step definitions:
- "the plan has zero or one heater allocation slots in the first N slots"  → GET /plan, count allocations with asset_id=heater in slots 0..N
- "the first heater allocation slot has power_kw X" → GET /plan, find first slot with heater allocation, assert power_kw == X ± 0.01
- "the heater is allocated in at least one of the first N plan slots" → GET /plan, any of slots 0..N has heater allocation

### Step 2 — Unit test stubs (add `#[ignore]` tests to existing test modules)

In `heater.rs` mod tests (9 new tests, all `#[ignore]` initially):
- `heater_milp_context_declares_e_tank_s_low_sw`
- `heater_milp_sw0_fixed_at_zero`
- `heater_milp_must_not_run_all_vars_zero`
- `heater_milp_constraints_initial_energy_pin`
- `heater_milp_constraints_dynamics_count`
- `heater_milp_constraints_upper_bound`
- `heater_milp_constraints_soft_low`
- `heater_milp_constraints_switching_four_per_step`
- `forecast_demand_kw_equals_draw_plus_loss_at_midpoint`

In `milp_planner.rs` mod tests (10 new tests, all `#[ignore]` initially):
- `heater_inputs_e_init_positive_above_min`
- `heater_inputs_e_init_negative_below_min`
- `heater_inputs_e_max_formula`
- `heater_inputs_q_dem_scalar`
- `heater_inputs_e_target_from_heater_target`
- `heater_inputs_autonomous_e_target_is_e_max`
- `heater_inputs_autonomous_mode_is_may_run`
- `heater_inputs_switching_penalty_defaults`
- `solve_heater_dynamics_respected`
- `solve_heater_must_run_meets_e_target`
- `solve_heater_soft_low_positive_when_below_min`
- `solve_heater_switching_reduces_with_penalty`
- `solve_heater_upper_bound_not_exceeded`

In `profile.rs` mod tests (3 new tests):
- `heater_config_switching_penalty_default`
- `heater_config_switching_penalty_explicit`
- `heater_config_yaml_without_penalty_field`

### Step 3 — `profile.rs`
Add `switching_penalty_eur: Option<f64>` + `effective_switching_penalty()`.  
Run: `cargo test -p ven profile` — profile tests go green.

### Step 4 — `assets/heater.rs`
- Add `Heater::forecast_demand_kw(ambient_temp_c: f64) -> f64` (returns scalar, not Vec)
- Replace `HeaterMilpContext`, `HeaterMilpVars`, `HeaterSolOutput` structs
- Implement `declare_vars()`, `constraints()`, `objective()`, `power_expr()`, `read_solution()`
- Update delegating methods on `Heater` to match new signatures
- Remove `energy_expr()` (no longer needed)
- Remove `#[ignore]` from heater.rs unit tests; run until green

### Step 5 — `controller/milp_planner.rs`
- Update `MilpInputs`: remove `e_heat_req_kwh`, add 5 new heater fields
- Update `build_milp_inputs()` heater block (both HeaterTarget and autonomous paths)
- Update `solve_milp()`: new `HeaterMilpContext` construction, updated variable pool wiring, objective additions (`M_low × s_low`, `lambda_sw × sw`), updated solution readback
- Update `SolveOutput`: add `e_heat_tank_kwh: Vec<f64>`
- Update `translate_to_plan()`: no change to heat_kw formula; update `build_plan_envelopes()` reference
- Update test helper `make_solver_inputs()` to fill new fields (MustNotRun path: all 0.0)
- Remove `#[ignore]` from milp_planner.rs tests; run until green

### Step 6 — Full cargo test

```bash
cargo test --workspace --jobs 2
```

All existing tests must pass. All new tests must pass.

### Step 7 — BDD tests

Deploy to Pi4, run new feature file:

```bash
ssh Pi4-Server "cd /srv/docker/openadr_lab && docker compose -f tests/docker-compose.test.yml run --build --rm test-runner features/ven_heater_tank.feature"
```

Fix step definitions and assertions until all 3 scenarios pass.

### Step 8 — Full regression BDD

```bash
docker compose -f tests/docker-compose.test.yml run --build --rm test-runner
```

All existing scenarios must continue to pass.

---

## Backward Compatibility

- All new `HeaterConfig` fields: `Option<f64>` + `#[serde(default)]` → existing YAMLs parse unchanged
- `MilpInputs` is `struct` (not `pub`) — private to the module; only `make_solver_inputs()` in tests needs updating (mechanical field additions, all `0.0` for MustNotRun)
- `MilpVarPool::heater: Option<HeaterMilpVars>` field name unchanged — only `HeaterMilpVars` contents change
- `translate_to_plan()` heat_kw formula unchanged — dispatcher and plan API output unaffected
- All existing tests that use `MustNotRun` path (no heater) are unaffected since the `else { None }` branch in `solve_milp()` is unchanged

## Constants to Hardcode (not profile-configurable in v1)

- `M_LOW_EUR_PER_KWH = 10.0` — soft violation penalty weight in objective
- Default `switching_penalty_eur = 0.01` — in `effective_switching_penalty()`
