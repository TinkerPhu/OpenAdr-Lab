# Research — MILP Asset Port (020-milp-asset-port)

*Phase 0 output for `/speckit.plan`. All unknowns resolved; no NEEDS CLARIFICATION items remain.*

---

## D1 — Trait location: where does `AssetMilpContext` live?

**Decision**: New file `VEN/src/controller/milp_planner/asset_port.rs`, re-exported from `controller/milp_planner/mod.rs`.

**Rationale**: 
- The trait is a *port* definition, owned by the domain controller layer. Placing it inside `controller/milp_planner/` makes it immediately visible to all solver sub-modules without extra import paths.
- Keeping it in a dedicated file (not tacked onto `types.rs`) preserves the 500-line file limit and makes the abstraction boundary explicit.
- `mod.rs` re-exports (`pub use asset_port::{AssetKind, AssetMilpContext, AssetMilpParams}`) so callers outside the module only need one import path.

**Alternatives considered**:
- *Extend `milp_interactions.rs`*: Rejected — that file is already complex; mixing port definitions with interaction logic worsens cohesion.
- *Extend `types.rs`*: Rejected — `types.rs` contains MILP numeric types; the trait is an architectural boundary, not a data type.
- *Top-level `controller/asset_milp_port.rs`*: Rejected — unnecessarily broad visibility; the trait is consumed only by `milp_planner/` sub-modules.

---

## D2 — How does `inputs.rs` shed concrete asset imports?

**Decision**: Introduce a narrow `AssetMilpParams` enum (variants: `Battery(BatteryScalars)`, `Ev(EvScalars)`, `Heater(HeaterScalars)`, `Unknown`) on the `AssetMilpContext` trait via a required method `fn milp_params(&self, n: usize, step_s: u64, now: DateTime<Utc>) -> AssetMilpParams`. `build_milp_inputs()` iterates `Vec<Box<dyn AssetMilpContext>>`, pattern-matches on the variant, and fills `MilpInputs` scalar fields exactly as before.

**Rationale**:
- `MilpInputs` (flat scalar struct) must remain unchanged to avoid breaking the 20+ existing unit tests.  
- The scalar parameter structs (`BatteryScalars`, `EvScalars`, `HeaterScalars`) are lightweight data containers defined in `asset_port.rs` alongside the trait — they carry no LP-variable handles.
- The match in `build_milp_inputs()` replaces the per-asset `if let Some(cfg) = profile.battery_config()` blocks with `if let AssetMilpParams::Battery(b) = asset.milp_params(…)`. The algorithmic content (mode resolution, EV horizon mask, heater target energy) moves into each asset's `milp_params()` implementation.
- This approach passes the constitution invariant (`grep -r "use crate::assets::" VEN/src/controller/milp_planner → empty`) without altering `MilpInputs`.

**Alternatives considered**:
- *Pass `MilpInputs` by mutable reference into a trait method `fn populate_milp_inputs(&self, inputs: &mut MilpInputs, …)`*: Rejected — couples the trait contract tightly to `MilpInputs` layout; any future MilpInputs field change forces trait changes.
- *Remove per-asset scalar fields from `MilpInputs` and pass assets directly into the solver*: Rejected — would break all 20+ existing unit tests that construct `MilpInputs` directly; Phase 4 is the right time for that deeper restructuring.
- *Keep `build_milp_inputs()` with concrete imports but move it out of `controller/milp_planner/`*: Rejected — the constitution invariant target is `VEN/src/controller/milp_planner/`, which includes `inputs.rs`.

---

## D3 — How do solver files shed `*MilpContext` reconstructions?

**Decision**: `solver_phase1.rs` and `solver_phase2.rs` receive `&[Box<dyn AssetMilpContext>]` in addition to `&MilpInputs`. They call `asset.declare_vars_into_pool(n, c_startup_eur, c_ramp_eur_kw, vars, &mut pool)` for each asset instead of manually reconstructing `BatteryMilpContext`/`EvMilpContext`/`HeaterMilpContext` from scalar fields. The internal `declare_vars` on each asset's `*MilpContext` struct is promoted to implement the `AssetMilpContext::declare_vars_into_pool()` method.

**Rationale**:
- The current solver pattern (reconstruct `BatteryMilpContext` from `MilpInputs` scalars, call `.declare_vars()`, store in pool) is a two-step round-trip: `asset → scalar → context → vars`. Phase 3 eliminates the middle step for the *solver entry point*.  
- `MilpInputs` scalar fields remain authoritative for grid-balance constraint construction (which uses `p_bat_ch_max_kw`, `p_ev_max_kw`, etc. directly). Assets populate those fields via `milp_params()` (D2); the pool is populated via `declare_vars_into_pool()`. The two calls happen in sequence: inputs built first, then vars declared.
- The `BatteryMilpContext::declare_vars()` / `constraints()` / `objective()` implementation bodies are NOT moved — only the dispatch path changes. Existing logic is preserved verbatim, now behind the trait interface.

**Alternatives considered**:
- *Keep solver files unchanged, only fix `inputs.rs`*: Rejected — solver files import `BatteryMilpContext` from `crate::assets`, violating the constitution invariant.
- *Inline all constraint logic into the trait methods, removing `*MilpContext` structs*: Rejected — too large a change for one phase; risks introducing bugs. Phase 4 is earmarked for further decomposition.

---

## D4 — Where do `*MilpVars` types live after the move?

**Decision**: `BatteryMilpVars`, `EvMilpVars`, `HeaterMilpVars` move from `assets/battery.rs`, `assets/ev.rs`, `assets/heater.rs` to **`controller/milp_planner/asset_port.rs`**, alongside their corresponding `*MilpContext` structs. `milp_interactions.rs` imports them from `crate::controller::milp_planner::asset_port`. Assets import them back via `use crate::controller::milp_planner::asset_port::{BatteryMilpVars, …}`.

**Rationale**:
- `BatteryMilpContext` and `BatteryMilpVars` are a natural pair — the context produces the vars, and the constraint methods read back from them. Co-locating them in `asset_port.rs` means a reader of that file sees the complete per-asset MILP type contract in one place.
- `milp_interactions.rs` is not the right owner: it defines cross-asset infrastructure (`MilpVarPool`, `AssetInteraction`), not per-asset types. Adding three unrelated type definitions there would weaken its cohesion.
- The dependency reversal (`assets/ → controller/milp_planner/`) is explicitly permitted in hexagonal architecture: the outer adapter ring is allowed to import from the inner domain port ring.
- `milp_interactions.rs` currently imports `BatteryMilpVars` FROM `assets/`. After the move, those imports are replaced with imports from `milp_planner::asset_port` — the assets dependency is gone.

**Alternatives considered**:
- *Move `*MilpVars` to `controller/milp_interactions.rs`*: Rejected — `milp_interactions.rs` defines cross-asset infrastructure; mixing in per-asset type definitions adds unrelated content. `asset_port.rs` is the more cohesive owner since both the context and vars for each asset belong together.
- *Move `*MilpVars` to `controller/milp_planner/types.rs`*: Viable, but splits the per-asset type contract across two files without benefit. `asset_port.rs` already handles per-asset types; `types.rs` should remain grid- and solver-level numeric types.
- *Keep `*MilpVars` in `assets/`*: Would leave `milp_interactions.rs` and `milp_planner/` still importing from `crate::assets::`, which contradicts the AB-02 fix intent.

---

## D5 — `MilpVarPool` population mechanism: typed slots vs. trait-only

**Decision**: Retain typed named slots in `MilpVarPool` (`bat: Option<BatteryMilpVars>`, `ev: Option<EvMilpVars>`, `heater: Option<HeaterMilpVars>`). Trait method `declare_vars_into_pool()` populates the appropriate slot by matching on `asset_kind()`. The `BatEvCoexistInteraction::applicable()` check (`pool.bat.is_some() && pool.ev.is_some()`) remains unchanged.

**Rationale**:
- The solver grid-balance constraint and the cross-asset interactions both access pool slots by name (e.g., `pool.bat.as_ref().map(|v| v.p_ch[t])…`). Replacing named fields with a `HashMap<AssetKind, Box<dyn AssetVarHandle>>` would require pervasive solver-code changes — out of Phase 3 scope.
- Each asset's `declare_vars_into_pool()` implementation knows its own kind and writes directly to the correct pool slot (e.g., `pool.bat = Some(self.declare_vars(…))` in `BatteryMilpContext`'s impl). No dispatch helper is needed in the solver — each trait impl owns its slot assignment.
- Typed slots allow existing `applicable()` / constraint code to remain unchanged (the Phase 3 regression baseline).

**Alternatives considered**:
- *Fully erased pool: `Vec<Box<dyn AssetVarHandle>>`*: Rejected — requires rewriting all constraint-builder code that uses typed pool fields; deferred to a later phase if ever needed.

---

## D6 — `AssetConfig::build_milp_context()` return type

**Decision**: `AssetConfig::build_milp_context()` in `assets/mod.rs` returns `Option<Box<dyn AssetMilpContext>>` instead of `Option<AnyMilpContext>`. `AnyMilpContext` is retained as a `pub(crate)` (or private) construction helper inside `assets/mod.rs` — the existing dispatch logic (`AnyMilpContext::Battery(BatteryMilpContext{…})`) is reused internally and then box-erased at the module boundary.

**Rationale**:
- Callers outside `assets/` (specifically `tasks/planning.rs` or `loops.rs`) currently receive `Option<AnyMilpContext>`. Changing the return to `Box<dyn AssetMilpContext>` at the boundary means callers need no `assets/` knowledge for context construction.
- Retaining `AnyMilpContext` internally avoids touching the construction logic — only the return type widens to a trait object.
- Consistent with FR-010 (Assumptions §B): "`AnyMilpContext` may be retained inside `assets/mod.rs` as an internal construction helper, but MUST NOT be imported by `controller/milp/` or `milp_interactions.rs`."

---

## D7 — n=48 regression profile

**Decision**: New YAML profile at `VEN/src/controller/milp_planner/tests/profiles/test48.yaml` (or embedded inline in `tests/planner.rs` as a `const` string). Specs: `plan_horizon_h: 24`, `plan_step_s: 1800` → n = 48. Includes battery (10 kWh, 5 kW), EV (plugged, must-run, 50 % SoC, 7.2 kW), heater (2 kW full / 1 kW mid), PV (6 kWp). Regression assertion: `SolveOutput::net_grid_kwh` stays within 5 % of the v2.x baseline captured during development.

**Rationale**:
- 24h horizon covers a full PV irradiance cycle (zero at night, peak at noon) and overnight storage discharge — the regime where battery + PV interaction matters most.
- n=48 (1 800 s steps) keeps solve time < 5 s on Pi4 (vs. n=288 production). This is confirmed by benchmarks: n=24 (2 h) takes < 0.5 s; n=48 scales ~linearly.
- The test is added to `tests/planner.rs` alongside the existing n=24 baseline — both run under `cargo test`.

**Alternatives considered**:
- *n=96 (12h, 900s steps)*: Richer time resolution, but solve time approaches 30 s on Pi4 and may flap in CI. Rejected.
- *n=48 as a separate YAML file in `VEN/profiles/`*: Would require shipping an unused runtime profile. Embedding in tests or keeping in `tests/profiles/` is cleaner for test isolation.

---

## D8 — `asset_kind()` in interactions vs. pool-based checks

**Decision**: `BatEvCoexistInteraction::applicable()` continues to use `pool.bat.is_some() && pool.ev.is_some()` (the pool-based check). The `asset_kind()` discriminant on the trait is primarily used in:
1. `declare_vars_into_pool()` dispatch (choosing which pool slot to populate).
2. Logging and diagnostics (structured log lines that name the asset type without importing it).
3. Future `AssetInteraction` implementations that need to discover co-present assets before the pool is built.

**Rationale**:
- `applicable()` fires after `MilpVarPool` is already built; the pool check is more direct than iterating the asset list by kind.
- Keeping `applicable()` pool-based means zero change to `BatEvCoexistInteraction` — the safest approach for the Phase 3 regression baseline.

---

## Summary of unknowns resolved

| Item | Status |
|------|--------|
| Trait location | `controller/milp_planner/asset_port.rs` |
| Scalar extraction from `inputs.rs` | `milp_params()` returning `AssetMilpParams` enum |
| LP var declaration in solvers | `declare_vars_into_pool()` on the trait |
| `*MilpVars` relocation | → `controller/milp_planner/asset_port.rs` (alongside `*MilpContext`) |
| `MilpVarPool` structure | Unchanged (typed slots retained); imports `*MilpVars` from `milp_planner::asset_port` |
| `AnyMilpContext` fate | Retained as `pub(crate)` internal helper in `assets/mod.rs` |
| n=48 profile location | `VEN/src/controller/milp_planner/tests/profiles/test48.yaml` (embedded or YAML) |
| `applicable()` in interactions | Unchanged (pool-based check) |
