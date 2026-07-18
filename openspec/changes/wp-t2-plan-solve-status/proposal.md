## Why

The MILP planner already knows internally whether a solve succeeded or fell through
to `fallback_plan` (`VEN/src/controller/milp_planner/mod.rs`), but that outcome is
discarded before it reaches the `Plan` returned to callers or the UI — a solver
failure looks identical to a minor plan warning in `PlanHeaderBar.tsx` today. This is
WP-T2 of `docs/plans/ven-ui-transparency.md`: the VEN UI does not transparently show
what the planner actually did, and this is the cheapest, most safety-relevant gap to
close first.

## What Changes

- Add a `solve_status: SolveStatus` field to the `Plan` entity (`VEN/src/entities/plan.rs`),
  with a two-state enum `Optimal | Infeasible` — reflecting the two solve outcomes
  that actually exist in the code today (see Non-goals).
- Set `solve_status: Optimal` in `translate_to_plan` (`results.rs`, the `Ok` branch of
  `run_planner`) and `solve_status: Infeasible` in `fallback_plan` (`results.rs`, the
  `Err` branch of `run_planner`, `mod.rs` around line 208), instead of discarding the
  typed `DomainError::PlanInfeasible` into a plain warning string.
- Add `solve_status` (and expose the already-computed but undeclared `objective_eur`,
  `friction_eur`) to the `PlanReady` variant of `PlannerEvent` (`VEN/src/planner_events.rs`).
- UI: add `solve_status`, `objective_eur`, `friction_eur` to the `Plan` and
  `PlannerEvent` TypeScript types (`VEN/ui/src/api/types.ts`); render a distinct
  infeasible-status chip in `PlanHeaderBar.tsx`, separate from the existing generic
  warnings-count badge.

## Capabilities

### New Capabilities
- `plan-solve-status`: the planner reports whether a `Plan` was produced by a
  successful MILP solve or by the infeasibility fallback, on both the REST `Plan`
  shape and the `/plan/events` SSE stream, and the UI renders that outcome as a
  distinct, legible status rather than folding it into generic warnings.

### Modified Capabilities
(none — `two-phase-milp` and `planner-config` govern solver *behavior*, not the
status/outcome reporting this change adds; no requirement in either changes)

## Impact

- **VEN** (Rust): `entities/plan.rs` (new field/enum), `controller/milp_planner/mod.rs`
  and `results.rs` (set the field at both solve outcomes), `planner_events.rs` and
  `services/planning.rs` (SSE payload). No route signature changes — same endpoints,
  richer payload.
- **VEN UI** (TypeScript/React): `api/types.ts` (type additions),
  `components/planner/PlanHeaderBar.tsx` (new status chip).
- **Non-goals**: this change does NOT introduce a third `FallbackHeuristic` state.
  The codebase has no separate heuristic-solve path today (searched — all
  "heuristic" hits are `AssetHeuristics`/BL-14 learned load profiles, unrelated to
  solver fallback); `fallback_plan` is synonymous with infeasibility, not a distinct
  heuristic substitute. A third state is deferred until a real heuristic-solve path
  exists (candidate: BL-13 early firm-up heuristic, Phase 6) — adding an unreachable
  enum variant now would be speculative code against a state nothing can produce.
- No VTN, BFF, or openleadr-rs changes. No OpenADR 3.1 spec constraint applies — this
  is purely VEN-internal observability, not wire-protocol behavior.
