# Contract: `AssetMilpContext` Port

**Feature**: 020-milp-asset-port  
**Module**: `VEN/src/controller/milp/asset_port.rs`  
**Type**: Rust trait (hexagonal port — inner domain ring)

---

## Overview

`AssetMilpContext` is the port that separates the MILP constraint-builder and cross-asset interaction module from concrete asset types (`Battery`, `EvCharger`, `Heater`). Any asset that participates in the MILP energy plan implements this trait. The planner and interaction modules receive `Vec<Box<dyn AssetMilpContext>>` and call only trait methods.

> **Phase 3 scope**: This trait covers the *planning-side* lifecycle only — variable declaration, constraint generation, objective contribution, and kind identification. Solution-reading (setpoint extraction after the solver runs) is intentionally excluded: `results.rs` reads setpoints directly from the `MilpVarPool` typed slots, which already reside in the controller boundary. A solution-reading method may be added to the trait in a future phase.

---

## Trait Definition

```rust
pub trait AssetMilpContext: Send + Sync {
    fn asset_id(&self) -> &str;
    fn asset_kind(&self) -> AssetKind;
    fn milp_params(&self, n: usize, step_s: u64, now: DateTime<Utc>) -> AssetMilpParams;
    fn declare_vars_into_pool(
        &self,
        n: usize,
        c_startup_eur: f64,
        c_ramp_eur_kw: f64,
        vars: &mut ProblemVariables,
        pool: &mut MilpVarPool,
    );
    fn constraints(&self, pool: &MilpVarPool, n: usize, dt_h: f64) -> Vec<Constraint>;
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

---

## Method Contracts

### `asset_id() -> &str`

| | |
|---|---|
| **Returns** | Stable string key matching the `SimSnapshot.assets` map (e.g. `"battery"`, `"ev"`, `"heater"`). |
| **Pre** | Always callable. |
| **Post** | Non-empty. Must not change across calls for the same instance. |
| **Panics** | Never. |

---

### `asset_kind() -> AssetKind`

| | |
|---|---|
| **Returns** | `AssetKind::Battery`, `AssetKind::Ev`, or `AssetKind::Heater`. |
| **Pre** | Always callable. |
| **Post** | Must be consistent with `asset_id()`. For example, `asset_id() == "battery"` implies `asset_kind() == AssetKind::Battery`. |
| **Panics** | Never. |
| **Usage** | Callers use this to dispatch `declare_vars_into_pool()` to the correct pool slot, and for structured logging. Do NOT use to pattern-match on concrete asset behaviour — that belongs in the implementation. |

---

### `milp_params(n, step_s, now) -> AssetMilpParams`

| | |
|---|---|
| **Returns** | `AssetMilpParams::Battery(BatteryScalars)`, `AssetMilpParams::Ev(EvScalars)`, or `AssetMilpParams::Heater(HeaterScalars)`. Must match `asset_kind()`. |
| **Pre** | `n > 0`, `step_s > 0`. |
| **Post** | All returned scalar values are finite (`f64::is_finite()`). Vec fields have `len == n`. |
| **Panics** | Implementations MUST NOT panic. Return `AssetMilpParams::Unknown` on any internal error rather than panicking. |
| **Side effects** | None. Pure computation; no LP-model mutation. May be called multiple times with identical results. |
| **Semantics** | Called by `build_milp_inputs()` to populate `MilpInputs` scalar fields. Any live-state–dependent computation (SoC, session mode, EV horizon mask, heater target energy) is baked into the context object at construction time. The `now` parameter enables per-slot time-series construction (`EV availability mask`, `q_dem_kw`, etc.). |

---

### `declare_vars_into_pool(n, c_startup_eur, c_ramp_eur_kw, vars, pool)`

| | |
|---|---|
| **Pre** | `n > 0`. The pool slot for this asset (`pool.bat`, `pool.ev`, or `pool.heater`) is `None` — MUST be called at most once per pool per asset kind. |
| **Post** | The pool slot for `self.asset_kind()` is `Some(…)`. The number of LP variables added to `vars` is deterministic given `n`, `c_startup_eur`, `c_ramp_eur_kw`. |
| **Panics** | Implementations MUST NOT panic. |
| **Order constraint** | MUST be called before `constraints()` and `objective()`. |
| **Cross-asset rule** | Implementations MUST write ONLY to the pool slot matching their own `asset_kind()`. Writing to another slot is forbidden. |

---

### `constraints(pool, n, dt_h) -> Vec<Constraint>`

| | |
|---|---|
| **Pre** | `declare_vars_into_pool()` has been called for this asset. The pool slot matching `self.asset_kind()` is `Some(…)`. `dt_h > 0.0`. |
| **Post** | Returned `Vec` may be empty (valid for assets with no internal constraints). All constraints reference only LP variables declared by this asset (via the pool slot). Cross-asset constraints live in `AssetInteraction` implementations, not here. |
| **Panics** | Implementations MUST NOT panic. If the pool slot is `None` (precondition violated by caller), implementations SHOULD return an empty `Vec` or use `debug_assert!`. |

---

### `objective(pool, n, dt_h, c_wear_eur_kwh, c_startup_eur, c_ramp_eur_kw) -> Expression`

| | |
|---|---|
| **Pre** | Same as `constraints()`. |
| **Post** | Returns a `good_lp::Expression`. If the asset has no cost contribution, returns `Expression::from(0.0)`. Must not contain grid or cross-asset terms. |
| **Panics** | Implementations MUST NOT panic. |

---

## Implementing the Contract

A new asset type (e.g. `ElectricVehicleFleet`) implements `AssetMilpContext` as follows:

1. **Define** a `*MilpContext` struct in the asset's module (`assets/ev_fleet.rs`) containing pre-computed parameters.
2. **Define** a `*MilpVars` struct in `controller/milp_interactions.rs` containing `good_lp::Variable` handles.
3. **Add** a slot `pub ev_fleet: Option<EvFleetMilpVars>` to `MilpVarPool`.
4. **Implement** `AssetMilpContext` for the context struct:
   - `asset_id()` → `"ev_fleet"` (must match `SimSnapshot` key)
   - `asset_kind()` → `AssetKind::EvFleet` (add variant to `AssetKind`)
   - `milp_params()` → `AssetMilpParams::EvFleet(EvFleetScalars { … })` (add variant to `AssetMilpParams`, add `EvFleetScalars` struct)
   - `declare_vars_into_pool()` → adds LP variables, stores in `pool.ev_fleet`
   - `constraints()` / `objective()` → read from `pool.ev_fleet`
5. **Extend** `AssetConfig::build_milp_context()` in `assets/mod.rs` to handle the new asset.
6. **Verify**: `grep -r "use crate::assets::" VEN/src/controller/milp → empty` still passes.
7. **Test**: Add a `#[cfg(test)]` block in `assets/ev_fleet.rs` that exercises `milp_params()` and `declare_vars_into_pool()` in isolation (no planner needed).

---

## Negative Contract (what the trait MUST NOT do)

| Forbidden | Reason |
|-----------|--------|
| Import `crate::assets::Battery`, `EvCharger`, `Heater` in `asset_port.rs` | Violates the port boundary |
| Access pool slots belonging to other assets in `constraints()` or `objective()` | Cross-asset coupling belongs in `AssetInteraction` |
| Perform IO or async operations inside any method | Trait methods are sync; async context is the caller's responsibility |
| Retain a reference to `ProblemVariables` after `declare_vars_into_pool()` returns | LP variable handles are captured in the pool; the vars object is moved into the solver |
| Return `MilpLoadMode::MustRun` when the asset is absent from the SimSnapshot | Mode resolution is the implementation's responsibility; absent asset → `MustNotRun` |

---

## Existing Implementations (Phase 3 deliverables)

| Type | Module | `asset_id()` | `asset_kind()` |
|------|--------|-------------|---------------|
| `BatteryMilpContext` | `assets/battery.rs` | `"battery"` | `AssetKind::Battery` |
| `EvMilpContext` | `assets/ev.rs` | `"ev"` | `AssetKind::Ev` |
| `HeaterMilpContext` | `assets/heater.rs` | `"heater"` | `AssetKind::Heater` |

---

## Constitution Compliance

After Phase 3:
```sh
# Must return empty
grep -r "use crate::assets::" VEN/src/controller/milp

# Must return empty (for Battery, EvCharger, Heater — not BatteryMilpContext which lives in assets/)
grep -n "crate::assets::battery::Battery\b\|crate::assets::ev::EvCharger\b\|crate::assets::heater::Heater\b" \
    VEN/src/controller/milp_interactions.rs
```
