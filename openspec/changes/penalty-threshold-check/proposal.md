## Why

The planner has no notion of demand-charge / peak-penalty tariffs: it will
happily concentrate load into the single cheapest slot even if that slot's
peak import would trigger a real utility penalty. This is BL-09 in
`docs/BACKLOG.md` — the only open backlog item rated High gain (it is the
sole item with a recurring, quantifiable €/kW financial upside once a
penalty tariff is configured) and is tracked as WP6.3 in
`docs/plans/roadmap/phase-6-fidelity-and-cert.md`.

## What Changes

- Planner gains a **per-solve, per-window soft-penalty MILP term**: for each
  configured penalty rule, a slack variable bounds each measurement window's
  peak import at `threshold_kw`, penalized in the objective at
  `penalty_eur_per_kw` — same soft-constraint idiom already used for
  `s_imp_viol`/`s_exp_viol` capacity violations.
- New profile config: `planner.penalty_rules: Vec<PenaltyRuleParams>`
  (`rule_id`, `threshold_kw`, `measurement_window_s`, `penalty_eur_per_kw`),
  default empty (feature off unless configured).
- New `entities::planner_params::PenaltyRuleParams` type, threaded through
  `PlannerParams` → `MilpInputs` → both solver phases → plan results.
- `CostBreakdown` gains `c_peak_penalty_eur`; a `PlanWarning` is emitted
  whenever a window still exceeds threshold after solving (penalty accepted
  because reallocation was infeasible/more expensive).
- Profile validation: `measurement_window_s` must be a positive multiple of
  `plan_step_s`; malformed rules reject via the existing
  `Profile::validate()` mechanism (same path as every other profile
  invariant — see design.md's Correction note for why this superseded the
  original `DomainError::ProfileInvalid` idea).
- VEN UI (Planner tab): `PlanDecisionMatrix` gains a "Peak demand" row
  visualizing each slot's import against the active threshold(s); the
  "penalty accepted" case needs no new UI — it already surfaces via the
  existing `Plan.warnings` → `PlanHeaderBar` rendering.
- No VTN UI change — this is a VEN-local planning capability.
- **Explicitly out of scope**: the stateful, persisted billing-period
  `PenaltyRule`/`PenaltyThreshold` sketch in `entities/design_vocabulary.rs`
  (rolling averages, `breached_this_period` surviving restarts, non-peak
  `PenaltyCondition` variants). That is a separate, heavier feature to
  propose later if real multi-day billing tracking is ever needed.

## Capabilities

### New Capabilities
- `planner-penalty-threshold`: planner-side configuration, MILP formulation,
  and plan output (cost breakdown + warnings) for peak-demand penalty
  avoidance.

### Modified Capabilities
(none — no existing `openspec/specs/` capabilities exist yet in this repo to modify)

## Impact

- **Backend (Rust)**: `entities/planner_params.rs`, `entities/plan.rs`
  (`CostBreakdown`), `profile/validate.rs` (new invariants, existing
  mechanism — see design.md Correction note), `profile/schema.rs`, `main.rs`,
  `services/planning.rs` (test constructors),
  `controller/milp_planner/{inputs.rs,types.rs,solver_phase1.rs,
  solver_phase2.rs,solver_duals.rs,results.rs}`, new
  `controller/milp_planner/penalty.rs`.
- **Frontend (VEN UI)**: `components/planner/PlanDecisionMatrix.tsx`,
  `api/types.ts`.
- **Tests**: new unit tests in `controller/milp_planner/tests/penalty.rs`,
  new BDD scenario in `tests/features/ven_planner.feature` + new fixture
  profile `VEN/profiles/penalty_test.yaml` + new step def in
  `tests/features/steps/planner_steps.py`.
- **Docs**: `docs/BACKLOG.md` (remove BL-09), `docs/architecture/
  VEN_ARCHITECTURE.md` §2.3 (add penalty-threshold content — currently has
  none despite BL-09's citation), `docs/plans/roadmap/
  phase-6-fidelity-and-cert.md` (mark WP6.3 done).
