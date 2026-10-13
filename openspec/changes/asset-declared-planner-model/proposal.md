# Proposal

## Why

The asset-plugin refactors gave every MILP-capable asset its own `AssetMilpContext`. The planner
around those contexts still knows each kind by name. It has one typed variable slot per kind
(`MilpVarPool.bat/ev/heater`), and the power balance names those slots one by one. `SolveOutput`
carries a flat field per kind (`p_bat_ch_kw`, `p_ev_kw`, `soc_ev`, `y_heat`, ...), and
`AssetScalars` holds a copy of 14 EV and heater parameters that "a later context of a kind
replaces". Startup, ramp and wear costs are looked up with `match ctx.asset_kind()`. The phase-2
warm start rebuilds every kind's variables by hand, and the asset objectives detect which phase
they are in from `c_startup_eur == 0.0`. On the result side, `translate_to_plan` and
`fill_planned_state` branch per kind, and `build_session_flexibility` holds three near-identical
copies of one rule.

Every one of these is a second place that knows what an asset is. Under `one-concept-one-function`
and `asset-competence-assurance` those copies are where this project's bugs come from (R-98 was
exactly this: a hand-written second declaration drifted until the dual LP was infeasible). They
are also the only thing that stops a second asset of a kind from reaching the planner. This change
finishes the isolation. It is W1 of the asset-isolation analysis, plus the half of W2 that changes
no wire format.

## What Changes

- **Variable pool keyed by asset.** `MilpVarPool` keeps the grid variables and holds every
  declared asset as a `Box<dyn DeclaredAsset>` in declaration order, the same shape shiftable
  loads use today. The typed `bat`, `ev`, `heater` and `shiftable` slots are removed.
- **Assets declare their own part of the power balance.** Each declared asset returns its net
  power at slot `t` as an expression, signed positive for import. The balance row becomes
  `p_imp + p_pv_used − Σ net_i − p_exp == p_base`, built in one loop.
- **The objective says which part it is.** Asset objectives take an explicit
  `ObjectivePart::{Cost(&CostWeights), Friction}` instead of reading the phase from
  `c_startup_eur == 0.0`. Each context carries its own friction costs (startup, ramp, tier,
  switching), set when it is built from `PlannerParams`. The `match ctx.asset_kind()` blocks in
  `model_skeleton.rs` and `solver_phase2.rs` are deleted.
- **One declaration, three modes.** `declare(n, DeclareMode::{Plan { friction }, Pinned(&own winning solution)})`
  replaces `declare_vars_into_pool` and `declare_pinned_vars_into_pool`. The phase-2 warm start
  becomes each declared asset's own `warm_start(&phase1_solution)`.
- **Each asset reads back its own solution.** `SolveOutput` keeps only grid-level series, plus
  `assets: Vec<AssetSolution>`, each carrying the asset id, net power, the planned state per slot,
  its own mode decisions and an asset-typed detail. `WinningModeDecisions` and the flat per-kind
  fields are deleted.
- **Interactions work over roles, not named slots.** `BatEvCoexistInteraction` couples every
  asset that offers a discharge leg with every asset that offers an on-indicator, and
  `CtrlImportMalus` sums `net_power_expr` over all assets. Neither reads a named slot.
- **The result side reads what assets declare.** Allocations take their surplus-claim order and
  pricing basis from the asset. Planned state comes from `AssetSolution`. EV plan warnings
  (firm-obligation shortfall, budget shortfall) move into the EV asset. Session flexibility
  becomes one generic function over each asset's declared `FlexibilityWindow`, replacing three
  copies. `AssetScalars` and the per-kind `Option<&XParams>` lookups in `run_planner` are deleted.
- **Contexts report their real id.** `asset_id()` returns the roster id the context was built
  for, not an `ids::ASSET_*` constant.
- **The planner core accepts several contexts of one kind.** The `run_planner` debug assertion
  "duplicate AssetKind for a singleton kind" becomes "duplicate asset id". Profile validation
  still refuses a second asset of a kind; lifting that is W7 and needs a product decision.
- **The architecture audit enforces it.** `scripts/audit_ven_architecture.py` gains a rule:
  no `AssetKind::`, no per-kind pool slot and no per-kind `SolveOutput` field in
  `controller/milp_planner/` or `controller/milp_interactions.rs` production code.
- **Not in scope (and not BREAKING):** the `Plan` wire format. `PlanTimeSlot.bat_charge_kw`,
  `bat_discharge_kw`, `pv_forecast_kw`, `pv_used_kw` and `Plan.soc_trajectory_kwh` stay, filled
  from asset-declared storage legs, so the BFF, UI and BDD steps see byte-identical plans for
  every existing profile. PV and base load remain site series (W3), and sessions, the arbiter and
  the UI are untouched (W4–W6).

## Capabilities

### New Capabilities
- `milp-planner-asset-model`: how the MILP planner composes its model and its plan from
  asset-declared contributions. This covers single-instance plan identity, several assets of one
  kind at planner level, role-based interactions, and asset-declared result facts.

### Modified Capabilities
- None. `openspec/specs/` holds no capability for the planner today.

## Impact

- **Code (VEN only):** `controller/asset_milp_port.rs` (trait reshaped),
  `controller/milp_interactions.rs`, `controller/milp_planner/{model_skeleton, solver_phase1,
  solver_phase2, solver_duals, types, input_stages, inputs, results, planned_state,
  session_flexibility, ev_diagnostics, asset_port, mod}.rs`, `assets/{battery_milp, ev_milp,
  ev_milp_accessors, heater_milp, shiftable_load}.rs`, `simulator/plan_context.rs`
  (context building), `services/test_support/milp_pool.rs`.
- **Tests:** about 190 references to per-kind `SolveOutput` fields in
  `controller/milp_planner/tests/`, and the pool-slot reads in the assets' own unit tests, move to
  by-id accessors. `golden/model_fingerprints.txt` must
  not change. A new `golden/plans.txt` records the whole translated `Plan` for every fingerprint
  scenario *before* any production edit, and must not change either. It is also R-118's stated
  first step for splitting `translate_to_plan`. `golden/milp_inputs.txt` loses the fields that move out
  of `MilpInputs` (`assets`, `budget_warning`, `w_tier_penalty_eur`). That regeneration is
  deliberate and is reviewed field by field.
- **No change to:** the profile schema, the `Plan` JSON, the BFF, either UI, the simulator tick,
  the arbiter, or OpenADR behaviour.
- **Risk:** reordering terms inside a constraint can change HiGHS's path even when the model is
  mathematically the same. The fingerprint probe records both a *sorted* and an *as-sent* digest
  per expression; design.md D9 defines how each is treated.
- **Effort:** about 8–11 focused days, landed as a sequence of commits on one branch, each green
  on its own (see tasks.md).
