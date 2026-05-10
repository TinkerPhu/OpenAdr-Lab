# Data Model — MILP Asset Port (020-milp-asset-port)

*Phase 1 output for `/speckit.plan`.*

---

## New Types (to be created)

### `AssetKind` enum — `controller/milp_planner/asset_port.rs`

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AssetKind {
    Battery,
    Ev,
    Heater,
}
```

**Purpose**: Allows `declare_vars_into_pool()` dispatch and structured logging without concrete type imports.  
**Location**: `VEN/src/controller/milp_planner/asset_port.rs`, re-exported from `controller/milp_planner/mod.rs`.

---

### `BatteryScalars`, `EvScalars`, `HeaterScalars` — `controller/milp_planner/asset_port.rs`

Lightweight scalar parameter bundles returned by `AssetMilpContext::milp_params()`.

```rust
pub struct BatteryScalars {
    pub e_nom_kwh: f64,
    pub e_init_kwh: f64,
    pub e_min_kwh: f64,
    pub e_max_kwh: f64,
    pub p_ch_max_kw: f64,
    pub p_dis_max_kw: f64,
    pub eff_ch: f64,
    pub eff_dis: f64,
}

pub struct EvScalars {
    pub mode: MilpLoadMode,          // from types.rs (no assets:: import)
    pub a_ev: Vec<bool>,             // availability mask, len = n
    pub t_dead_step: Option<usize>,
    pub p_max_kw: f64,
    pub p_min_kw: f64,
    pub e_core_kwh: f64,
    pub e_extra_max_kwh: f64,
    pub v_extra_eur_kwh: f64,
}

pub struct HeaterScalars {
    pub mode: MilpLoadMode,
    pub t_dead_step: Option<usize>,
    pub p_mid_kw: f64,
    pub p_full_kw: f64,
    pub e_init_kwh: f64,
    pub e_max_kwh: f64,
    /// Constant per-step thermal demand [kW]: draw_kw + k_loss × (T_mid − ambient).
    /// Scalar, not a Vec — the heater model uses a fixed demand every slot.
    pub q_dem_kw: f64,
    /// Target tank energy at deadline [kWh above T_min].
    /// In MustRun mode: derived from target temperature. In MayRun/autonomous: equals e_max_kwh.
    /// Always Some after construction — never None.
    pub e_target_kwh: f64,
    pub lambda_sw_eur: f64,
    /// 1.0 if heater was at mid power on the last real tick; 0.0 otherwise.
    /// Used to anchor the switching penalty constraint at t=0.
    pub initial_z_mid: f64,
    /// 1.0 if heater was at full power on the last real tick; 0.0 otherwise.
    pub initial_z_full: f64,
}

pub enum AssetMilpParams {
    Battery(BatteryScalars),
    Ev(EvScalars),
    Heater(HeaterScalars),
    Unknown,
}
```

**Purpose**: Carry the scalar parameters that `build_milp_inputs()` currently extracts by directly constructing concrete `*MilpContext` objects. After Phase 3, `inputs.rs` pattern-matches on `AssetMilpParams` variants instead.  
**Location**: `VEN/src/controller/milp_planner/asset_port.rs`.

> **Note on `soc_init`**: The `soc_init` field was removed from `EvScalars` and `HeaterScalars`. Solution-reading (initial SoC readback for `SolveOutput`) is handled by `results.rs` directly via the `MilpVarPool` typed slots — this path is already architecturally compliant and does not go through `AssetMilpParams`. Adding a solution-reading field to the scalar structs would be dead code in this phase (see FR-003 scope note in spec.md).

---

### `AssetMilpContext` trait — `controller/milp_planner/asset_port.rs`

```rust
pub trait AssetMilpContext: Send + Sync {
    /// Stable identifier matching the SimSnapshot asset map key.
    fn asset_id(&self) -> &str;

    /// Discriminant used for pool-slot dispatch and logging.
    fn asset_kind(&self) -> AssetKind;

    /// Phase A — scalar extraction: return all MILP parameters for this asset,
    /// pre-computed for a planning cycle of `n` slots starting at `now`.
    fn milp_params(
        &self,
        n: usize,
        step_s: u64,
        now: DateTime<Utc>,
    ) -> AssetMilpParams;

    /// Phase B — LP variable declaration: add LP variables for this asset to
    /// `vars` and store the resulting typed handles in the appropriate slot of
    /// `pool`. Called once per planning cycle, before constraint/objective building.
    fn declare_vars_into_pool(
        &self,
        n: usize,
        c_startup_eur: f64,
        c_ramp_eur_kw: f64,
        vars: &mut ProblemVariables,
        pool: &mut MilpVarPool,
    );

    /// Phase B — constraints: generate all LP constraints for this asset,
    /// reading its typed vars from `pool`.
    fn constraints(
        &self,
        pool: &MilpVarPool,
        n: usize,
        dt_h: f64,
    ) -> Vec<Constraint>;

    /// Phase B — objective contribution: return the cost/comfort expression
    /// for this asset's variables.
    fn objective(
        &self,
        pool: &MilpVarPool,
        n: usize,
        dt_h: f64,
        c_wear_eur_kwh: f64,
        c_startup_eur: f64,
        c_ramp_eur_kw: f64,
    ) -> Expression;
}
```

**Invariants**:
- `declare_vars_into_pool()` MUST be called before `constraints()` and `objective()`.
- `constraints()` and `objective()` access only the pool slot that matches `self.asset_kind()`. Accessing another asset's slot is undefined behaviour.
- `milp_params()` is pure (no LP-model side effects). May be called multiple times safely.
- Implementations MUST NOT import concrete sibling asset types.

> **Scope note — solution-reading**: This trait deliberately omits a solution-reading / setpoint-extraction method. `results.rs` reads slot-by-slot setpoints from the `MilpVarPool` typed fields (`pool.bat`, `pool.ev`, `pool.heater`), which reside within the controller boundary. This is architecturally compliant (no `crate::assets::*` import in `results.rs`). A trait method for setpoint extraction may be added in a future phase once the port stabilises.

> **Note on `milp_params` parameters**: The `n`, `step_s`, and `now` arguments allow implementations to lazily compute per-slot vectors (e.g., EV availability mask from a calendar). Implementations that pre-compute all fields at construction time (e.g., `BatteryMilpContext`) may ignore these parameters. The signature is forward-compatible — do not simplify it to zero-arg.

**Location**: `VEN/src/controller/milp_planner/asset_port.rs`.

---

## Moved Types

### `BatteryMilpVars` — moves from `assets/battery.rs` → `controller/milp_planner/asset_port.rs`

No field changes. Placed in `asset_port.rs` alongside `BatteryMilpContext` — context and vars for the same asset stay in the same file. `milp_interactions.rs` imports them from `crate::controller::milp_planner::asset_port`.

### `EvMilpVars` — moves from `assets/ev.rs` → `controller/milp_planner/asset_port.rs`

No field changes.

### `HeaterMilpVars` — moves from `assets/heater.rs` → `controller/milp_planner/asset_port.rs`

No field changes.

**Dependency reversal**: After the move, `assets/battery.rs` gains `use crate::controller::milp_planner::asset_port::BatteryMilpVars;` (outer ring → inner ring — permitted in hexagonal architecture). `milp_interactions.rs` loses its three `use crate::assets::*` imports and instead imports from `crate::controller::milp_planner::asset_port`.

---

## Changed Types

### `MilpVarPool` — `controller/milp_interactions.rs` (unchanged fields)

```rust
pub struct MilpVarPool {
    pub grid: GridMilpVars,
    pub bat: Option<BatteryMilpVars>,    // unchanged
    pub ev: Option<EvMilpVars>,          // unchanged
    pub heater: Option<HeaterMilpVars>,  // unchanged
    pub shiftable: Vec<ShiftableLoadMilpVars>,
}
```

**Change**: `BatteryMilpVars` / `EvMilpVars` / `HeaterMilpVars` are no longer imported from `assets/`; they are now imported from `crate::controller::milp_planner::asset_port`. No structural change to the pool itself.

### `build_milp_inputs()` signature — `controller/milp_planner/inputs.rs`

**Before**:
```rust
pub(crate) fn build_milp_inputs(
    assets: &SimSnapshot,
    tariffs: &TariffTimeSeries,
    capacity: &OadrCapacityState,
    profile: &Profile,
    now: DateTime<Utc>,
    ev_session: Option<&EvSession>,
    heater_target: Option<&HeaterTarget>,
    shiftable_loads: &[ShiftableLoad],
    baseline_override: Option<&BaselineOverride>,
) -> MilpInputs
```

**After** (adds `asset_contexts` parameter, drops per-asset imports):
```rust
pub(crate) fn build_milp_inputs(
    asset_contexts: &[Box<dyn AssetMilpContext>],   // NEW — replaces Battery/EV/Heater imports
    tariffs: &TariffTimeSeries,
    capacity: &OadrCapacityState,
    profile: &Profile,
    now: DateTime<Utc>,
    shiftable_loads: &[ShiftableLoad],
    baseline_override: Option<&BaselineOverride>,
) -> MilpInputs
```

**Note**: `ev_session` and `heater_target` session data moves into the asset context objects — the `EvMilpContext::from_state()` / `HeaterMilpContext` construction that currently uses them happens in `EvMilpContext::milp_params()` instead. The `SimSnapshot` (`assets` parameter) is no longer needed in `inputs.rs` because each context object was constructed with live state already baked in. `profile` remains for grid parameters (PV config, base load, grid limits).

### `solve_phase1()` / `solve_phase2()` signatures — `controller/milp_planner/solver_phase1.rs`, `solver_phase2.rs`

**Added parameter**: `asset_contexts: &[Box<dyn AssetMilpContext>]`

Solvers call `asset.declare_vars_into_pool(…)` per context rather than reconstructing `BatteryMilpContext` / `EvMilpContext` / `HeaterMilpContext` from `MilpInputs` scalar fields.

### `AssetConfig::build_milp_context()` — `assets/mod.rs`

**Before**: `pub fn build_milp_context(&self, …) -> Option<AnyMilpContext>`  
**After**: `pub fn build_milp_context(&self, …) -> Option<Box<dyn AssetMilpContext>>`

`AnyMilpContext` enum is retained as `pub(crate)` internal helper for construction dispatch.

---

## Call Flow (post-Phase 3)

```
tasks/planning.rs (or loops.rs)
  ├── builds Vec<Box<dyn AssetMilpContext>> via AssetConfig::build_milp_context()
  └── calls run_planner(asset_contexts, tariffs, capacity, profile, …)
        │
        ├── build_milp_inputs(&asset_contexts, tariffs, capacity, profile, …)
        │     └── for each ctx: match ctx.milp_params(n, step_s, now) {
        │               AssetMilpParams::Battery(b) => fill MilpInputs.e_bat_nom_kwh, …
        │               AssetMilpParams::Ev(e)      => fill MilpInputs.a_ev, …
        │               AssetMilpParams::Heater(h)  => fill MilpInputs.p_heat_full_kw, …
        │         }
        │
        ├── build_phase1_weights(profile) → Phase1Weights   (unchanged)
        │
        └── solve_phase1(&inputs, &p1w, &asset_contexts)
              ├── builds MilpVarPool (grid vars declared inline, asset vars via trait)
              │     └── for each ctx: ctx.declare_vars_into_pool(n, startup, ramp, vars, &mut pool)
              ├── for each ctx: cs.extend(ctx.constraints(&pool, n, dt_h))
              ├── for each ctx: obj += ctx.objective(&pool, n, dt_h, …)
              └── solve → SolveOutput
```

---

## `MilpInputs` — unchanged fields

`MilpInputs` in `types.rs` is **not changed** in Phase 3. All 20+ existing unit tests that construct `MilpInputs` directly continue to compile without modification. This is by design — Phase 4 is earmarked for possible `MilpInputs` restructuring.

---

## Test Profile — n=48 (new)

**Location**: `VEN/src/controller/milp_planner/tests/profiles/test48.yaml`  
**Parameters**:

```yaml
plan_horizon_h: 24
plan_step_s: 1800        # 48 slots total

battery:
  capacity_kwh: 10.0
  max_charge_kw: 5.0
  max_discharge_kw: 5.0
  min_soc: 0.1
  initial_soc: 0.5
  round_trip_efficiency: 0.92

ev:
  battery_kwh: 40.0
  max_charge_kw: 7.2
  min_charge_kw: 0.0
  soc_target: 0.9
  initial_soc: 0.5
  plugged_fraction: 1.0  # always plugged for test

heater:
  p_full_kw: 2.0
  p_mid_kw: 1.0
  tank_kwh: 5.0
  min_tank_kwh: 0.5
  initial_tank_kwh: 2.5

pv:
  rated_kw: 6.0
```
