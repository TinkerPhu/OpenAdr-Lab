## 1. Domain: `SolveStatus` on `Plan`

- [x] 1.1 Add `SolveStatus { Optimal, Infeasible }` enum to `VEN/src/entities/plan.rs`
      (`Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize`,
      `#[serde(rename_all = "SCREAMING_SNAKE_CASE")]`).
- [x] 1.2 Add `solve_status: SolveStatus` field to the `Plan` struct.
- [x] 1.3 Unit test `solve_status_serializes_as_screaming_snake_case`: round-trip both
      variants through `serde_json` and assert the exact strings `"OPTIMAL"` /
      `"INFEASIBLE"`.

## 2. Planner: set the field at both existing branch points

- [x] 2.1 In `VEN/src/controller/milp_planner/results.rs`, set
      `solve_status: SolveStatus::Optimal` in `translate_to_plan`'s `Plan{...}`
      construction (the `Ok` branch of `run_planner`).
- [x] 2.2 In the same file's `fallback_plan`, set
      `solve_status: SolveStatus::Infeasible` in its `Plan{...}` construction (called
      from `mod.rs`'s `Err` branch, where `DomainError::PlanInfeasible` is
      constructed/logged today — leave that logging and the Critical warning
      unchanged).
- [x] 2.3 Unit test `test_plan_carries_optimal_status_and_objective_value`: a
      feasible planning case (existing happy-path fixture/mock) produces
      `Plan.solve_status == Optimal` and a non-zero `objective_eur` matching
      `phase1_cost_eur`.
- [x] 2.4 Unit test `test_plan_carries_infeasible_status_on_unsolvable_constraints`:
      force an unsolvable case (via the mock solver in
      `services/test_support/mock_solver_port.rs` or an existing infeasibility
      fixture if one exists) and assert `Plan.solve_status == Infeasible` while the
      existing Critical warning with the reason string is still present.

## 3. SSE: `PlannerEvent::PlanReady` payload

- [x] 3.1 Add `solve_status: SolveStatus` to the `PlanReady` variant in
      `VEN/src/planner_events.rs` (fields `objective_eur`/`friction_eur` already
      exist there — no change needed to those two beyond confirming they're
      populated).
- [x] 3.2 In `VEN/src/services/planning.rs` (`adopt_if_warranted`, around line
      389-397), pass `plan.solve_status` through to the emitted `PlanReady` event
      alongside the existing `objective_eur`/`friction_eur` reads.
- [x] 3.3 Unit/integration test `test_plan_ready_event_solve_status_matches_plan`:
      adopting a `Plan` with a known `solve_status` emits a `PlanReady` event whose
      `solve_status` (and `objective_eur`/`friction_eur`) match exactly.

## 4. VEN Rust suite gate

- [x] 4.1 `wsl cargo fmt --check` and `wsl cargo clippy --all-targets --all-features -- -D warnings`
      clean.
- [x] 4.2 `wsl cargo test -p ven-app` green (domain + use-case + adapter-contract
      layers all touched by this change). (678/678 passed)
- [x] 4.3 `scripts/audit_file_sizes.py` — confirm `entities/plan.rs`,
      `controller/milp_planner/results.rs`/`mod.rs`, `planner_events.rs`,
      `services/planning.rs` stay under the 500-production-line VEN/src/ limit after
      the additions (all are small field/branch additions, not expected to breach).
      Audit passed.

## 5. UI: types + `PlanHeaderBar` chip

- [x] 5.1 In `VEN/ui/src/api/types.ts`, add `solve_status: 'OPTIMAL' | 'INFEASIBLE'`,
      `objective_eur: number`, `friction_eur: number` to the `Plan` type, and the
      same three fields to the `plan_ready` variant of the `PlannerEvent` union.
- [x] 5.2 In `VEN/ui/src/components/planner/PlanHeaderBar.tsx`, add a distinct
      infeasible-status chip (separate `data-testid`, e.g.
      `plan-infeasible-chip`) rendered when `plan.solve_status === 'INFEASIBLE'`,
      positioned alongside (not inside) the existing warnings-count badge. No
      chip rendered when `solve_status === 'OPTIMAL'`.
- [x] 5.3 Component test: infeasible plan renders the chip and the existing Critical
      warning remains visible when the warnings list is expanded; optimal plan
      renders no chip.

## 6. UI suite gate

- [x] 6.1 `cd VEN/ui && npm test` green. (362/362 passed)
- [x] 6.2 ESLint zero errors on changed files. (also: `npx tsc --noEmit` clean)

## 7. BDD (optional for this WP, add if a suitable scenario slot exists)

- [x] 7.1 Checked `tests/features/ven_planner.feature` — no natural hook without a
      heavy new fixture (the existing infeasibility test double,
      `InfeasibleBatCtx`, is unit-test-only and not exposed at the BDD/E2E layer).
      Deferred as GB-12 in `docs/BACKLOG.md` rather than forced into this WP.

## 8. Bookkeeping

- [x] 8.1 Marked WP-T2 as done in `docs/plans/ven-ui-transparency.md` §4/§7.
- [x] 8.2 Noted in `docs/history/project_journal.md`: the two-state (not three-state)
      `SolveStatus` decision and why, plus the 12-call-site struct-literal lesson.
