# Tasks

Branch: `refactor/asset-declared-planner-model`. Each numbered group lands as one or more commits
that are green on their own.

"Planner tests green" means: `cargo check --tests` locally, run in WSL under `scripts/wsl_lock.sh`
with `-j 2` after a free-memory check; then the HiGHS-backed suite on Node2
(`DOCKER_HOST=Node2 bash run_all_tests.sh --rust`). Before that, take the Node2 lock, check out the
branch on the host and confirm it with `git log -1`. "Goldens hold" means `model_fingerprint`,
`inputs_golden` and `plans_golden` pass with no `UPDATE_*` variable set, unless a task says
otherwise.

## 1. Safety net (no production change)

- [x] 1.1 Add `tests/plans_golden.rs`: for every scenario in `model_fingerprint::scenarios()`, run
      `solve_milp_two_phase` + `translate_to_plan` and record the whole `Plan`. That is every slot
      field, allocations, `planned_kw_by_asset`, `planned_state_by_asset` (sorted keys), session
      flexibility, warnings, summary, cost breakdown, marginal costs and `soc_trajectory_kwh`,
      with floats printed `{:?}`. Add an `UPDATE_PLANS_GOLDEN=1` writer. Also add a fallback-plan
      scenario. Verify: golden recorded on unmodified code and the test passes twice in a row on
      Node2.
- [x] 1.2 Add test accessors in `controller/milp_planner/tests/mod.rs` over today's flat
      `SolveOutput`: `AssetSolutionsView` with `sol.battery()`, `sol.ev()`, `sol.heater()`,
      `sol.shiftable_kw()`, `sol.shiftable_starts()` (the first asset of each kind; the
      multi-instance tests of group 7 read `AssetSolution`s by id). Migrate every test read of
      the flat per-kind fields to them (188 reads today), `model_fingerprint`'s `solution_text`
      included, so it prints the same text from any `SolveOutput` shape. The `pool.bat/ev/heater`
      reads in the assets' own `*_milp.rs` unit tests change with the pool itself (task 4.2).
      Verify: planner tests green, goldens hold, and
      `grep -rnE "\.(p_bat_|p_ev_kw|soc_ev|y_heat|e_bat_kwh|e_heat_tank)" controller/milp_planner/tests`
      finds only `tests/mod.rs`.

## 2. Contexts carry their id and one build-parameter object (D10, D11)

- [x] 2.1 Test first: `build_asset_contexts` returns each context with the roster id it was built
      for (a battery built as `"bat-a"` reports `"bat-a"`). Confirm it fails. Then store the id on
      `BatteryMilpContext`, `EvMilpContext` and `HeaterMilpContext` and return it from
      `asset_id()`. The test fixtures that build contexts directly (`tests/fixtures.rs`:
      `battery_ctx`, `lossless_battery`, `absent_ev`) pass the id the profile uses today, so the
      goldens see the same ids. Verify: the new test plus planner tests green, goldens hold.
- [x] 2.2 Introduce `MilpBuildInputs` and change `MilpParticipant::build_milp_context` to take it.
      Planner scalars are read from `&PlannerParams`. The EV's minimum charge power is its own
      `min_charge_kw`, which deletes `min_ev_charge_kw` in `simulator/plan_context.rs`. Remove the
      `#[allow(clippy::too_many_arguments)]` it no longer needs. Verify: `cargo clippy
      --all-targets --all-features -- -D warnings` clean, planner tests green, goldens hold.
- [x] 2.3 File the two firm-obligation energy rules (design inventory) in
      `docs/reference/TECHNICAL_DEBTS.md` as needs-decision (R-140; the heater's `lambda_sw`, the
      same first-of-kind shape, is R-141), with Severity, Kind and Cost, and
      `Affected files` naming `VEN/src/assets/ev_milp_accessors.rs` and
      `VEN/src/controller/milp_planner/input_stages.rs` (task 6.5 moves the second; update the
      row then). Verify: the row exists and `audit_debt_gate.py` accepts it.

## 3. Explicit objective parts and asset-owned costs (D3)

- [ ] 3.1 Test first, per asset: `objective(ObjectivePart::Friction)` contains the startup/ramp
      (battery, EV) or tier/switching (heater) terms and no economic term, and
      `objective(ObjectivePart::Cost(..))` the reverse. Write the tests in each asset's `*_milp.rs`
      test module and confirm they fail to compile. Then add `ObjectivePart`, `CostWeights` and
      `FrictionCosts` (set at build from `PlannerParams`; heater tier penalty from
      `w_tier_penalty_eur`). Map each existing `c_startup_eur == 0.0` branch 1:1.
- [ ] 3.2 Replace the `match ctx.asset_kind()` blocks in `model_skeleton.rs::cost_expr` /
      `declare_asset` and `solver_phase2.rs::solve_phase2` with one call per asset. Delete
      `Phase2Weights` and its test fixture `make_phase2_weights` (its callers set the friction on
      the context instead), and rename `Phase1Weights.c_bat_wear_eur_kwh` to `c_storage_wear_eur_kwh`
      (move it into `CostWeights`). The profile key `c_bat_wear_eur_kwh` is unchanged. Verify:
      planner tests green, goldens hold (sorted and solution digests identical).

## 4. Declared assets in the pool; balance and interactions over them (D1, D2, D4, D6)

- [ ] 4.1 Add `DeclaredAsset` (in `controller/asset_milp_port.rs`) and
      `AssetMilpContext::declare(n, DeclareMode, vars)`. Implement it for battery, EV, heater and
      shiftable load by moving today's `declare_vars_with` bodies. Each impl holds its typed vars.
      Test first: each asset's `net_power_expr(t)` has the coefficients of today's balance terms
      (unit test per asset via `bound_range`-style probing). Verify: those tests green.
- [ ] 4.2 Switch `MilpVarPool` to `{ grid, assets: Vec<Box<dyn DeclaredAsset>> }`. Build the
      balance row as `p_imp + p_pv_used − Σ net_i − p_exp == p_base`. `CtrlImportMalus` sums
      `net_power_expr` and `controllable_power_expr` is deleted. Give `BatEvCoexistInteraction`
      role accessors (`discharge_leg`, `on_indicator`) with one McCormick block per pair, and
      decide the open naming question here. Delete `declare_vars_into_pool` /
      `declare_pinned_vars_into_pool`. Verify: planner tests green. Goldens: sorted and solution
      digests identical; an as-sent-only change is regenerated in its own commit whose message
      lists the changed lines (design D9).

## 5. Per-asset readback, warm start and pinned declaration (D4, D5)

- [ ] 5.1 Add `AssetSolution` / `AssetSolutionDetail` and `DeclaredAsset::read(&dyn Solution)`.
      Each asset moves its `read_solution` into `read` and fills `net_power_kw`. `SolveOutput`
      becomes grid series + `assets`. Re-point the 1.2 test accessors to the new shape, and leave
      every test body untouched. Verify: planner tests green, goldens hold.
- [ ] 5.2 Move the battery, EV and heater halves of `build_phase2_warm_start` into
      `DeclaredAsset::warm_start(&AssetSolution)` verbatim (keep the R-97 `u_bat` comment). The
      heater reads `initial_y` from its own context. Verify: the warm-start record in
      `model_fingerprints.txt` is identical.
- [ ] 5.3 `DeclareMode::Pinned(&AssetSolution)`: each asset pins its own decisions from its own
      detail. Delete `WinningModeDecisions` and `pinned_binary`'s per-kind callers outside the
      assets. While here, verify (unit test) that a `MustNotRun` heater's `y_heat` is fixed to 0;
      if it is not, apply the mode inside the heater's `read` (design Risks). Verify: marginal-cost
      records identical, planner tests green.

## 6. Result side reads declared facts (D7, D8)

- [ ] 6.1 Allocations: add `surplus_claim_rank()` and `allocation_basis()` to `AssetMilpContext`
      (EV 0/Load, heater 1/Load, shiftable 2/Load, battery 3/Storage). Replace the four blocks in
      `translate_to_plan` with one loop over a stable sort of `AssetSolution`s. Verify:
      `plans.txt` byte-identical.
- [ ] 6.2 Planned state and storage legs: each asset fills `planned_state` and (battery only)
      `storage` in `read`. The heater context gains `temp_min_c` / `thermal_mass_kwh_per_c`.
      Delete `planned_state.rs`. Plan `bat_charge_kw`, `bat_discharge_kw`, `soc_trajectory_kwh` and
      the wear line of `CostBreakdown` sum the storage legs in today's order. Verify: `plans.txt`
      byte-identical.
- [ ] 6.3 Warnings: move `ev_diagnostics.rs` into `assets/` as the EV's solution warnings, and
      `ev_budget_warning` into the EV context's `input_warnings()`. Remove
      `MilpInputs.budget_warning`. Keep the warning order of design D7. Verify: `plans.txt`
      byte-identical (it includes the budget and firm-shortfall scenarios; add a scenario to 1.1's
      list first if either is missing).
- [ ] 6.4 Session flexibility: test first. A unit test of the one generic rule over a
      `FlexibilityWindow` reproduces today's EV, heater and shiftable entries. Another test shows
      that the EV context's declared window comes from the same session `current_ev_session(now)`
      returns. Then add `flexibility_window()` and replace the three blocks with one loop (fallback
      plan too). Verify: `plans.txt` byte-identical, including the fallback scenario.
- [ ] 6.5 Move `AssetScalars::ev_obligation_kwh` / `ev_firm_kwh` into the EV asset unchanged, next
      to `firm_required_kwh` (do not merge them; task 2.3's row). Then delete what has no caller
      left: `AssetScalars` / `asset_scalars`, `AssetMilpParams`,
      `*Scalars`, `milp_params()`, `MilpLoadMode` (if unused), `AssetKind` / `asset_kind()`,
      `MilpInputs.assets` / `w_tier_penalty_eur`, `run_planner`'s `battery/ev/heater_params`, and
      `SolveRequest`'s and `run_planner`'s `ev_session`, `heater_target` and `shiftable_loads`
      (check `services/planning` callers). Replace the duplicate-kind debug assertion with a
      duplicate-id one. Regenerate `milp_inputs.txt` once. Verify: its diff is removed lines only,
      `plans.txt` byte-identical, planner tests green.

## 7. Several assets of one kind, at planner level (spec requirement 2)

- [ ] 7.1 Test: two battery contexts (`"bat-a"` 10 kWh at 0.2 SoC, `"bat-b"` 5 kWh at 0.9 SoC)
      under a two-zone tariff. Each id gets its own allocation and planned SoC, and each planned
      energy stays within its own bounds. Verify: green on Node2.
- [ ] 7.2 Test: two EV contexts with different departures and firm targets. Each charges toward its
      own target, and an unreachable target on one names that EV's session in the warning. Verify:
      green on Node2.
- [ ] 7.3 Test: the coexistence interaction with two batteries and one EV declares two pairs; with
      no discharger it declares nothing (spec requirement 3). Verify: green.

## 8. Make it structural (D12)

- [ ] 8.1 Teach `rust_files` in `scripts/audit_ven_architecture.py` to accept a single file as a
      base (today `rglob` on a file yields nothing, so a rule on `milp_interactions.rs` would pass
      vacuously). Then add the `forbid` rules. First run them against a scratch copy that
      reintroduces `pool.bat` in `milp_interactions.rs` *and* in `milp_planner/` and confirm both
      fail; then confirm they pass on the branch. Verify: `python scripts/audit_ven_architecture.py`
      passes.
- [ ] 8.2 Document the new rules in `.claude/CLAUDE.md`'s `ven-architecture` invariant list (one
      line each). Verify: the line exists and names the script.

## 9. Pre-merge checks, docs and registers

- [ ] 9.1 `cargo fmt --check`, `cargo clippy --all-targets --all-features -- -D warnings`,
      `cargo audit`, `python scripts/audit_file_sizes.py`, `python scripts/audit_duplication.py`
      (expect fewer clusters: session flexibility and allocation blocks; run `--update` only after
      it reports a reduction), `python scripts/audit_ven_architecture.py`,
      `python scripts/gen_module_graph.py` then `python scripts/audit_module_graph.py` (files move
      and disappear, so the generated import graph changes), `python scripts/audit_debt_gate.py`.
      Verify: all pass.
- [ ] 9.2 Fold into current-state docs: `docs/architecture/ven_milp_planner.md` gets a section on
      how assets contribute to the model (pool, net power, objective parts, roles, readback,
      declared result facts), pointing at `plans_golden.rs` and `model_fingerprint.rs` rather than
      restating them. Update `docs/architecture/VEN_ARCHITECTURE.md` (the planner/`SolveOutput`
      paragraph near line 1621), the hand-drawn diagram in
      `docs/architecture/module_dependency_graph.md` (its `AssetMilpContext` node still lists
      `build_variables()` / `extract_solution()`, which no longer exist) and `docs/architecture/ven_asset_interface_spec.md` (the asset's MILP surface).
      Verify: `grep -rn "MilpVarPool\|declare_vars_into_pool\|WinningModeDecisions" docs/architecture`
      shows only current names.
- [ ] 9.3 Update `docs/reference/architectural_smells.md` (#1, R-127). Keep the decision that a VEN
      has one asset per kind, but correct the facts: the MILP no longer has one slot per kind, and
      the boundary is now the profile check plus W2–W7.
- [ ] 9.4 `docs/reference/TECHNICAL_DEBTS.md`: file the `MinCost`/`MaxRevenue` hard-coded wear
      `0.03` as needs-decision (Severity, Kind, Cost, Why-open). Update R-118 (`translate_to_plan`
      is split) and R-71 (`build_milp_context` now takes a parameter object) with re-measured
      numbers, or delete them if done. Verify: `audit_debt_gate.py` passes.
- [ ] 9.5 `docs/history/project_journal.md` entry (what, why, learnings). Add any durable lesson
      to `docs/reference/KEY_LEARNINGS.md`. Run `/wiki-sync` for the planner pages.
- [ ] 9.6 Full suites on Node2: `DOCKER_HOST=Node2 bash run_all_tests.sh` (Rust, E2E BDD,
      resilience), under the Node2 lock with the branch verified on the host. Verify: all green.
      No BDD scenario changes, since no user-observable behaviour changes. Then rebase,
      fast-forward merge to `main`, and push.
- [ ] 9.7 After the merge: delete this change directory (its facts now live in the docs above),
      switch Node2 back to `main`, delete the merged branch locally and on the host, and remove any
      worktree used. The final report lists the issues filed, fixed and deferred.
