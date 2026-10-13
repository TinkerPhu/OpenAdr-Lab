# Design

## Context

See proposal.md (Why) for the motivation. This section is the inventory the
`one-concept-one-function` rule requires: every place a per-kind concept lives today, and what it
becomes.

| Concept | Where it lives today | Becomes |
|---|---|---|
| Asset LP variables | `MilpVarPool { bat, ev, heater: Option<..>, shiftable: Vec<..> }` (`controller/milp_interactions.rs:111`); each impl writes its own slot (`assets/{battery,ev,heater}_milp.rs`, `assets/shiftable_load.rs`) | `MilpVarPool { grid, assets: Vec<Box<dyn DeclaredAsset>> }` (D1) |
| Asset term in the power balance | `model_skeleton.rs:244-278`: `bat_dis` on the left, `ev_kw + heat_kw + shift_kw + bat_ch` on the right, each read from a named slot | `DeclaredAsset::net_power_expr(t)`, summed in one loop (D2) |
| "Controllable power" | `milp_interactions.rs:257` `controllable_power_expr`: a second copy of the same per-slot sum | Same `net_power_expr(t)` sum: one concept, one function (D2) |
| Which phase an objective is in | `c_startup_eur == 0.0` convention inside `battery_milp`, `ev_milp`, `heater_milp` objectives; heater gets `c_startup_eur = 1.0` as a flag | `ObjectivePart::{Cost(&CostWeights), Friction}` (D3) |
| Friction costs per kind | `Phase2Weights { c_bat_*, c_ev_*, w_tier_penalty_eur }`, picked by `match ctx.asset_kind()` in `model_skeleton.rs:68` and `solver_phase2.rs:154`; test fixture `make_phase2_weights` (`tests/fixtures.rs:54`) | Each context carries its own `FrictionCosts`, set when it is built (D3) |
| Wear cost per kind | `Phase1Weights.c_bat_wear_eur_kwh`, picked by `match` at `model_skeleton.rs:213`; recomputed for `CostBreakdown.c_wear_eur` at `results.rs:374` | `CostWeights.c_storage_wear_eur_kwh`, applied by the asset that wears; the breakdown reads the storage legs (D3, D7) |
| Free vs pinned declaration | `declare_vars_into_pool` + `declare_pinned_vars_into_pool` (two trait methods) + `Declaration::declare_asset` | `declare(n, DeclareMode, vars)` (D4) |
| Phase-2 warm start | `solver_phase2.rs:20-130`: rebuilds battery, EV and heater values from flat `SolveOutput` fields, reading `inputs.assets.heat_initial_y` | `DeclaredAsset::warm_start(&own phase-1 AssetSolution)` (D4) |
| Winning mode decisions | `WinningModeDecisions { u_bat, z_ev_on, y_heat, z_heat_ready, y_shift: HashMap }` | Inside each `AssetSolution`'s detail (D5) |
| Solution readback | `model_skeleton.rs:323-429` unpacks each slot into `SolveOutput.p_bat_ch_kw`, `p_ev_kw`, `soc_ev`, `y_heat`, `e_heat_tank_kwh`, ... and builds an n+1 flat SoC curve for a missing EV | `DeclaredAsset::read(&dyn Solution) -> AssetSolution`; `SolveOutput.assets: Vec<AssetSolution>` (D5) |
| Planner-side copy of asset parameters | `AssetScalars` (13 EV/heater fields, plus a `ShiftableLoadMilpContext` copy of every shiftable load that `results.rs:222` and `session_flexibility.rs:156` read) + `asset_scalars()` + `AssetMilpParams`/`*Scalars` + `milp_params()` (`input_stages.rs:236-356`, `asset_milp_port.rs`) | Deleted. Each reader asks the asset (D7, D8) |
| Energy a firm EV obligation needs | Two definitions. `AssetScalars::ev_obligation_kwh` / `ev_firm_kwh` (`input_stages.rs:298-310`): `(target − soc_init) · battery_kwh`, read by `ev_diagnostics.rs:40` and `session_flexibility.rs:66`. `EvMilpContext::firm_required_kwh` (`assets/ev_milp_accessors.rs:107`): clamps the target to `soc_ceiling()` and adds the forecast consumption up to the deadline, read by the solve | W1 moves the first into the EV asset unchanged, next to the second (plans stay identical). Consolidating them changes the shortfall warning and the session-flexibility energy, so it is a needs-decision debt row (task 2.3), not part of W1 |
| Coupling interactions | `BatEvCoexistInteraction` unwraps `pool.bat`/`pool.ev` | Role accessors `discharge_leg()` / `on_indicator()`, every pair (D6) |
| Allocation per asset | `results.rs:179-276`: four hand-written blocks (EV, heater, shiftable, battery) in a fixed order | One loop over `AssetSolution`s, ordered by declared `surplus_claim_rank`, priced by declared `AllocationBasis` (D7) |
| Planned state per slot | `planned_state.rs` takes `Option<&BatteryParams/EvParams/HeaterParams>` | `AssetSolution.planned_state`, built by the asset with the existing `battery_future_state` / `ev_future_state_at` / `heater_future_state` (D7) |
| Plan site fields `bat_charge_kw`, `bat_discharge_kw`, `soc_trajectory_kwh` | `sol.p_bat_ch_kw` / `p_bat_dis_kw` / `e_bat_kwh` | Σ over assets' declared `StorageLegs` (D7) |
| Session flexibility | `session_flexibility.rs`: three near-identical blocks (EV, heater, shiftable) reading `ev_session`, `heater_target`, `EvParams`, `HeaterParams`, `inputs.assets.*` | One function over each context's declared `FlexibilityWindow` (D8) |
| EV warnings | `ev_diagnostics.rs` (firm shortfall, reads `inputs.assets.ev_*`); `input_stages::ev_budget_warning` → `MilpInputs.budget_warning` | `AssetMilpContext::input_warnings()` + `AssetSolution.warnings`, written in `assets/ev_*` (D7) |
| Asset id of a context | `asset_id()` returns `ids::ASSET_BATTERY` / `ASSET_EV` / `ASSET_HEATER` (`battery_milp.rs:178`, `ev_milp.rs:383`, `heater_milp.rs:413`) | The roster id the context was built for (D10) |
| Kind-singular lookups in `run_planner` | `battery_params`, `ev_params`, `heater_params` (`milp_planner/mod.rs`); `min_ev_charge_kw` from the first EV (`simulator/plan_context.rs:130`) | Deleted. `pv_params` / `base_load_params` stay for the site series (W3) |
| Context build parameters | `MilpParticipant::build_milp_context`: 19 parameters, 6 of them planner scalars | One `MilpBuildInputs` parameter object carrying `&PlannerParams` (D11) |

Constraints that shape the approach:
- **Model identity is checkable.** `tests/model_fingerprint.rs` records every variable, objective,
  constraint and warm start of all three solves over the scenarios in its `scenarios()`, with a
  *sorted* and an *as-sent* digest per expression, plus the solution. `tests/inputs_golden.rs`
  does the same for `MilpInputs`. No golden covers the translated `Plan` yet.
- `good_lp::Solution` is object-safe (`eval` is `where Self: Sized`; `value` is not generic), so
  a declared asset can read back through `&dyn Solution`.
- Variable creation order is part of the model. Today it is: grid → penalty slacks → each asset
  in context order (each asset's internal order fixed by its `declare_vars_with`) → interactions.

## Goals / Non-Goals

**Goals:**
- No code in `controller/milp_planner/` or `controller/milp_interactions.rs` names an asset kind,
  a per-kind slot or a per-kind field. An audit rule makes that structural.
- Identical models and identical plans for every existing profile, proven by goldens recorded
  before the first production edit.
- The planner core plans several contexts of one kind (unit-tested with two batteries and two
  EVs).

**Non-Goals:**
- Lifting `validate_asset_roster` (W7), PV and base load as planner participants (W3), sessions,
  the arbiter, dispatch and UI per asset (W4–W6).
- Changing the `Plan` wire format (W2), including the site-level `bat_*` and `soc_trajectory_kwh`
  fields.
- An open set of asset kinds. The closed enums (`AssetState`, `AssetParams`, `AssetProfile`) stay.
  This change removes branching on them *outside* `assets/`, not the enums.
- Fixing the objective-dependent wear constant (see Risks); it would change plans.

## Decisions

### D1. The pool holds declared assets, not typed slots
`MilpVarPool { grid: GridMilpVars, assets: Vec<Box<dyn DeclaredAsset + 'a>> }`, in context order.
A `DeclaredAsset` owns its typed variable handles (`BatteryMilpVars` etc., unchanged) and borrows
its context. It answers everything the solve needs:
`asset_id`, `net_power_expr(t)`, `constraints(dt_h)`, `objective(part, dt_h)`,
`warm_start(&AssetSolution)`, `read(&dyn Solution) -> AssetSolution`, and the role accessors of D6.
*Alternative:* a `HashMap<id, AssetMilpVars>` with a closed enum of var structs. Rejected: every
consumer (balance, interactions, readback) would match on the enum again, which is the branching
this change removes. *Alternative:* keep vars in the context behind interior mutability. Rejected:
the same context is declared three times per cycle (phase 1, phase 2, pinned), so the vars belong to
one declaration, not to the context.

### D2. One signed net-power expression per asset
`net_power_expr(t)`: positive = import, the project-wide sign convention. Battery `p_ch − p_dis`,
EV `p_ev`, heater `p_step_kw · y_heat`, shiftable load `Σ power_kw · y_shift[j]` over the starts
covering `t` (the loop now in `model_skeleton.rs:245` moves into the shiftable load). The balance
row becomes `p_imp + p_pv_used − Σ_i net_i − p_exp == p_base`. `CtrlImportMalus` uses the same sum,
which deletes `controllable_power_expr`. The two were copies of one concept.

### D3. Objectives say which part they contribute; costs live with the asset
`ObjectivePart::Cost(&CostWeights)` is phase 1's economics (also phase 2's cap and the pinned pass).
`ObjectivePart::Friction` is phase 2's friction. Each asset maps its existing branches 1:1:
battery Cost = wear + terminal reward, Friction = startup + ramp; EV Cost = service reward,
Friction = startup + ramp; heater Cost = below-minimum penalty + terminal reward, Friction = tier
penalty + switching; shiftable load = nothing in either (its tie-break stays global).
`FrictionCosts { startup_eur, ramp_eur_kw, tier_penalty_eur }` is set on the context when it is
built, from the same `PlannerParams` fields `build_phase2_weights` reads today. `Phase2Weights` is
deleted. `CostWeights` carries the one objective-dependent asset term, `c_storage_wear_eur_kwh`
(today's `Phase1Weights.c_bat_wear_eur_kwh`, renamed because it is a price on storage throughput).
*Alternative:* keep `Phase2Weights` and let the asset pick its field. Rejected: that is the same
per-kind lookup, moved one step.

### D4. One declaration, two modes; warm start is the asset's
`AssetMilpContext::declare(n, DeclareMode, vars)` with `DeclareMode::Plan { friction: bool }`
(auxiliary startup/ramp variables only when `friction` is true and the asset's own cost is > 0,
which is today's `c_startup_eur > 0.0` test, unchanged) and `DeclareMode::Pinned(&AssetSolution)`
(the asset's own winning solution, looked up by id by the skeleton). The body is today's
`declare_vars_with`. `solver_phase2::build_phase2_warm_start` keeps the grid half and calls
`warm_start` per asset for the rest. The battery/EV/heater halves move verbatim into their asset
files, including the R-97 `u_bat` direction comment.

### D5. `SolveOutput` = grid series + one `AssetSolution` per asset
`AssetSolution { asset_id, net_power_kw: Vec<f64>, planned_state: Vec<HashMap<String,f64>>,
storage: Option<StorageLegs>, warnings: Vec<PlanWarning>, detail: AssetSolutionDetail }`.
`detail` is a closed enum (one variant per kind: today's `BatterySolOutput` + `u_bat`,
`EvSolOutput`, `HeaterSolOutput`, the shiftable `y_shift` row). Only the owning kind's module
matches on it, for its own pinned declaration and warm start, and an audit rule enforces that.
*Alternative:* `Box<dyn Any>` downcast. Rejected: not `Clone` (`SolveOutput` is), and a wrong
downcast fails at runtime instead of at compile time. *Alternative:* a named-series map
(`HashMap<&str, Vec<f64>>`). Rejected: stringly typed, the shape `state_values()` / `.val("...")`
was audited out of the codebase for.

### D6. Interactions couple roles
`discharge_leg() -> Option<DischargeLeg { p_dis: &[Variable], max_kw }>` (battery) and
`on_indicator() -> Option<OnIndicator { z_on: &[Variable], p_min_kw }>` (EV).
`BatEvCoexistInteraction` (kept name, doc updated: "discharge while a switched load charges on PV
surplus") declares one McCormick block per (discharger, on-indicator) pair with distinct ids, in
pool order. With one battery and one EV that is exactly today's block. `applicable` = at least one
pair. `InteractionVars::BatEvCoexist` holds one `x_coexist` vector per pair.

### D7. Plan assembly reads declared facts
- **Allocations:** one loop over `AssetSolution`s, sorted (stable) by
  `AssetMilpContext::surplus_claim_rank()`. The rank is declared per kind at the kind's definition
  (EV 0, heater 1, shiftable 2, battery 3), which reproduces today's block order. Equal ranks keep
  context order. The allocation rule is today's (`|net| > 0.01`), priced by the declared
  `AllocationBasis::{Load, Storage}`: Load = grid share at import + surplus share at export,
  `marginal_value` = import tariff; Storage = Load's rule while charging, export revenue while
  discharging, `marginal_value` = export tariff. These are today's two pricing blocks, each
  written once.
- **Planned state:** each asset fills `AssetSolution.planned_state` in `read` with its existing
  future-state helper. The heater context gains `temp_min_c` and `thermal_mass_kwh_per_c`, the EV
  uses its own `a_ev` mask. `planned_state.rs` is deleted.
- **Site storage fields:** `bat_charge_kw`, `bat_discharge_kw` and `soc_trajectory_kwh` are the
  sum over assets that declare `StorageLegs` (the battery; the EV does not, as today). The wear
  line of `CostBreakdown` is `c_storage_wear_eur_kwh · Σ (charge + discharge) · dt` over the same
  legs, in today's summation order. With no storage asset these are zeros of today's lengths.
- **Warnings:** the EV's firm-shortfall warning (`ev_diagnostics.rs`) moves into
  `assets/ev_*` and is emitted into `AssetSolution.warnings`. The budget warning becomes
  `input_warnings()` on the EV context; it is computable before the solve from the tariff the EV
  already receives in `inject_grid_slots`. `translate_to_plan` keeps today's warning order:
  stale-rate, CO2 stale-rate, input warnings (context order), capacity violation, penalty rules,
  solution warnings (context order).

### D8. One session-flexibility rule over declared windows
`AssetMilpContext::flexibility_window() -> Option<FlexibilityWindow { energy_needed_kwh,
power_min_kw, power_max_kw, window_start, window_end }>`. The EV declares its current session
(departure; energy = firm requirement, else priced-band total; power from its own min/max charge).
The heater declares its target (`ready_by`; energy = `e_target_kwh`; power 0 to
`p_step · n_stages`). A shiftable load declares its own window and energy. `build_session_flexibility`
becomes one loop. The window start is `max(window_start, now)`, which is `now` for EV and heater,
so the three variants reduce to one formula. The EV and heater contexts store the session/target
window they were built from. Consequently `run_planner` and `SolveRequest` no longer need
`ev_session`, `heater_target` or `shiftable_loads`, and those fields are removed (their only
planner use was this function). The fallback plan uses the same windows.

### D9. Identity is proven by goldens recorded first
Task 1 adds `golden/plans.txt`: the full `Plan` (every slot field, allocations, planned state,
session flexibility, warnings, summary, breakdown, marginal costs) for every fingerprint scenario,
floats printed with `{:?}` (round-trip exact). It is recorded on unmodified code and committed
before any production change. Then:
- `plans.txt` must stay byte-identical.
- In `model_fingerprints.txt`, every *sorted* digest and every solution digest must stay identical.
  An *as-sent* digest may change only where D2 regroups the terms of a row. That is accepted only
  in a commit that changes nothing else in the golden, and the commit message names each changed
  line. If a solution digest changes, the refactor is wrong (or exposed a tie). Investigate and do
  not regenerate.
- `milp_inputs.txt` is regenerated once, in the commit that removes `MilpInputs.assets`,
  `budget_warning` and `w_tier_penalty_eur`. The diff must contain only removed lines.

### D10. Contexts carry their roster id; duplicate ids are a contract violation
Each context stores the `asset_id` passed to `build_milp_context` (today only the shiftable load
does) and returns it. The `run_planner` debug assertion checks unique ids instead of "one per kind",
and `AssetKind` is deleted once `asset_kind()` has no caller.

### D11. A parameter object for context building
`MilpParticipant::build_milp_context(&self, inputs: &MilpBuildInputs)`, where `MilpBuildInputs`
carries the id, state, grid (`n`, `cum_s`, `now`), sessions, target, comfort rates,
`&PlannerParams`, `lambda_sw`, terminal value and anchor. The six planner scalars become reads
from `PlannerParams`. `ev_min_charge_kw` (today taken from the *first* EV's params by
`plan_context.rs:130`) is the EV's own `min_charge_kw`. `lambda_sw` stays an input: it is the same
"first heater's value" shape, but `Heater` is persisted sim state and has no switching-penalty field,
so giving it one needs a decision about old JSON (R-141). This is the parameter object R-71 names
for this signature.

### D12. The audit makes it structural
`forbid()` walks its bases with `Path.rglob("*.rs")`, which finds nothing when a base is a single
file, so a rule naming `controller/milp_interactions.rs` would pass vacuously. `rust_files` first
learns to accept a file as a base (with a test in the script's own test style).
`scripts/audit_ven_architecture.py` then gains `forbid` rules over production code in
`controller/milp_planner/` and `controller/milp_interactions.rs`: no `AssetKind`, no
`AssetSolutionDetail::`, no `ids::ASSET_`, and no identifier matching
`\b(bat|ev|heat|heater)_[a-z_]*` except the wire field names `bat_charge_kw` / `bat_discharge_kw`
on `PlanTimeSlot` (allowlisted by exact line pattern). A second rule: `AssetSolutionDetail::<Kind>`
appears only in that kind's own `assets/` files.

## Risks / Trade-offs

- [A regrouped balance row changes HiGHS's path on a tie] → D9: sorted digests and solution digests
  are the gate. A changed solution blocks the commit and is investigated, never regenerated.
- [Floating-point reassociation changes a plan figure in the last bit] → D7 keeps each sum in
  today's order. `plans.txt` uses exact `{:?}`, so any drift is visible rather than tolerated.
- [The EV context's session window differs from the `current_ev_session(now)` that
  `build_session_flexibility` reads today] → the context is built from the same `ev_sessions`
  slice. Task 6.2 adds a test that both pick the same session (the active one at `now`), and
  `plans.txt` covers the EV-session scenarios.
- [The heater's planned allocation under `MustNotRun`] today is suppressed by a mode check, not by
  power. → Verify `y_heat` is fixed to 0 under `MustNotRun` (task 5.3). If it is not, the heater's
  `net_power_kw` readback applies the mode, inside the asset.
- [Two firm-obligation energy rules (inventory)] → W1 keeps both, byte-identical, and co-locates
  them in the EV asset so the debt row points at one file.
- [~190 test references to flat fields churn the test suite] → by-id accessors in
  `tests/mod.rs` (`sol.asset("battery")`, `.storage()`, `.detail_ev()`) keep each test edit
  mechanical and reviewable. No test assertion changes value.
- [Wear constant] `build_phase1_weights` hard-codes `0.03` for `MinCost`/`MaxRevenue` and ignores
  the profile's `c_bat_wear_eur_kwh`. Out of scope (it changes plans). It is filed as a
  needs-decision debt row in task 9.4.
- [`AssetSolutionDetail` is one more closed enum] → it is matched only by its own kind, which
  the audit rule enforces. Adding a kind adds a variant next to the existing `AssetState`
  variant, and the compiler lists every place.

## Migration Plan

Internal refactor, no data or wire migration. Branch `refactor/asset-declared-planner-model`,
rebased and fast-forward merged to `main` after all pre-merge checks and the E2E suite on Node2.
Deploy to the Node1/Node2 VENs as usual. Rollback is a revert of the merge range; no persisted
state changes shape (`Plan` JSON is unchanged).

## Open Questions

- Whether `BatEvCoexistInteraction` is renamed (e.g. `DischargeWhileSwitchedLoadInteraction`)
  once it couples roles. That is naming only, decided at task 4.2 against the UI/config label
  `c_bat_ev_coexist_eur_kwh` (naming-transparency favours keeping the config word).
