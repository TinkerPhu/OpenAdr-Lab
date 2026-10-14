# Tasks

Branch: `refactor/asset-declared-plan-output`. "Rust green" means `cargo check --tests` and the
touched modules' tests locally in WSL (under `scripts/wsl_lock.sh`, `-j 2`, after a free-memory
check), then the HiGHS-backed suite on Node2 under its lock with the branch checked out and
confirmed by `git log -1`.

## 1. The plan names the assets it planned

- [x] 1.1 Test first: `results.rs` unit tests that a solved plan and a fallback plan built from
      the same three contexts carry their ids in input order, and that an idle asset is named
      without an allocation; `entities/plan.rs` tests for `Plan::planned_assets()` (list first,
      then ids only the slot maps name; empty-list JSON loads).
- [x] 1.2 Add `Plan.planned_asset_ids` (`#[serde(default)]`) and `Plan::planned_assets()`; write
      the list in `translate_to_plan` and `fallback_plan`; add the field to every hand-built
      `Plan` literal.
- [x] 1.3 `services/forecast.rs::build_asset_forecasts` enumerates `plan.planned_assets()`; a unit
      test pins that a planned idle asset with no state gets a zero `Optimization` forecast.

## 2. Delete the battery fields

- [x] 2.1 Move the planner tests off `bat_charge_kw`, `bat_discharge_kw` and `soc_trajectory_kwh`
      (`tests/planner.rs`, `tests/heater.rs`, `tests/multi_instance.rs`) to the battery's own
      `planned_kw_by_asset`, allocation, planned state or `AssetSolution.storage`.
- [x] 2.2 Delete the three fields from `entities/plan.rs`, `results.rs` and every literal; reduce
      `site_storage` to the throughput series the wear figure needs.
- [x] 2.3 Regenerate `tests/golden/plans.txt`; check it against the old golden with the
      transformation in design.md; `model_fingerprints.txt` and `milp_inputs.txt` hold unchanged.

## 3. UI

- [x] 3.1 Regenerate the ts-rs bindings (`Plan.ts`, `PlanTimeSlot.ts`); update
      `__tests__/fixtures/plan.ts`.
- [x] 3.2 Test first in `PlanDecisionMatrix.test.tsx`: an idle planned asset gets a row; a plan
      without `planned_asset_ids` still shows its allocated assets; then rows come from
      `planned_asset_ids` ∪ allocation ids, and the `"battery"`, `"pv"`, `"base_load"` literals go.
- [x] 3.3 `npm test`, `npm run lint` and `npm run build` in `VEN/ui`.

## 4. Enforcement

- [ ] 4.1 `scripts/audit_ven_architecture.py` rule 16: add `entities/plan.rs`, delete the
      `PLAN_WIRE_FIELDS` exemption, update the rule text; `scripts/test_audit_ven_architecture.py`
      plants `bat_charge_kw` in a copy of `entities/plan.rs` and expects a hit; `.claude/CLAUDE.md`
      invariant line.

## 5. Use case

- [ ] 5.1 BDD in `tests/features/ven_planner.feature`: on the test profile, the plan names the
      battery, the EV and the heater as planned, and every allocation belongs to one of them; run
      `python scripts/audit_step_definitions.py`.

## 6. Checks, docs, merge

- [ ] 6.1 fmt, clippy, `cargo audit`, file sizes, duplication, architecture audit, module graph,
      debt gate; all four suites on Node2.
- [ ] 6.2 Docs: `docs/architecture/ven_milp_planner.md` (plan assembly and the solution-to-plan
      table), `VEN_ARCHITECTURE.md` where it lists plan fields, `architectural_smells.md` #1, the
      Planner tab's Decision Matrix in the relevant use-case doc; journal and key learnings;
      `/wiki-sync` for the planner pages.
- [ ] 6.3 Delete this change; rebase, fast-forward merge, clean up Node2 and the worktree.
