## Context

`VEN/src/controller/milp_planner/mod.rs::run_planner` already branches on the
solver's `Result`: `Ok((sol, phase1_cost_eur, friction_eur))` goes to
`translate_to_plan` (`results.rs`), `Err(e)` builds a `DomainError::PlanInfeasible`,
logs it with `warn!`, and calls `fallback_plan(reason)` with just the stringified
reason — the typed error is discarded at that point. Both branches produce a `Plan`
(`VEN/src/entities/plan.rs`), so callers (`services/planning.rs`, the SSE emitter,
and the UI) currently cannot tell which branch produced the `Plan` they're holding
except by pattern-matching the warnings list for a specific message string, which
`PlanHeaderBar.tsx` doesn't do — it renders any warning identically.

`SolverPort` (`controller/solver_port.rs`) is documented as infallible by contract:
implementations must always return a usable `Plan`, even on internal solver failure.
This design does not change that contract — `solve_status` is data attached to the
`Plan` the port already unconditionally returns, not a new failure mode.

## Goals / Non-Goals

**Goals:**
- Make the two solve outcomes that exist today (`Optimal`, `Infeasible`) visible on
  the `Plan` entity, the `/plan/events` SSE stream, and the UI, without changing the
  `SolverPort` contract or any route signature.
- Expose the already-computed `objective_eur`/`friction_eur` fields through the SSE
  `PlanReady` variant and the UI types, closing the gap where the backend already
  serializes them but the UI type declarations don't know about them.

**Non-Goals:**
- No third `FallbackHeuristic` state. There is no heuristic-solve code path in this
  codebase today (verified — all "heuristic" hits are `AssetHeuristics`/BL-14
  learned-load-profile code, unrelated to solver fallback). Adding an enum variant
  nothing can produce would be speculative; when a real heuristic-solve path exists
  (candidate: BL-13 early firm-up heuristic), extending the enum is a small, isolated
  follow-up.
- No change to `SolverPort`'s infallibility contract, to `fallback_plan`'s trigger
  conditions, or to any solver algorithm/objective behavior (`two-phase-milp`,
  `planner-config` capabilities are untouched).
- No new route. `/plan` and `/plan/events` keep their existing shape, just with
  additional fields.

## Decisions

**D1 — `SolveStatus` lives in `entities/plan.rs`, not a new module.**
It's a plain data enum describing an existing entity's provenance, same ring as
`Plan`/`PlanWarning`/`PlanSummary` already in that file. A new module would be
premature separation for a two-variant enum with no behavior of its own.

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SolveStatus {
    Optimal,
    Infeasible,
}
```

Alternative considered: a `bool is_infeasible` field. Rejected — an enum documents
intent at call sites (`SolveStatus::Optimal` vs. `true`/`false`) and costs nothing
extra to serialize; this matches the project's existing preference for typed
vocabulary over booleans (e.g. `UserNotificationSeverity`, not a `is_critical: bool`).

**D2 — Set the field at the two existing branch points, not via a new type.**
`translate_to_plan` (`results.rs`, `Ok` branch) sets `solve_status: SolveStatus::Optimal`;
`fallback_plan` (`results.rs`, `Err` branch, called from `mod.rs` where
`DomainError::PlanInfeasible` is currently constructed and logged) sets
`solve_status: SolveStatus::Infeasible`. No new call sites, no new error type — this
is the exact discard point the proposal targets, closed at the source.

**D3 — SSE payload change, not a new event variant.**
`PlannerEvent::PlanReady` already carries `objective_eur`, `friction_eur` — those are
serialized today (`services/planning.rs:389-397` reads them off the just-solved
`Plan`) but the UI's TypeScript union type never declared them, so the UI has been
silently ignoring fields the wire already sends. Add `solve_status` alongside them
in the same variant, and add all three fields to the TS type. No new SSE event kind
needed — `PlanReady` already fires exactly when this information becomes available.

```json
// GET /plan/events — PlanReady variant, after this change
{
  "type": "plan_ready",
  "plan_id": "...",
  "objective": {...},
  "solver_ms": 842,
  "objective_eur": 3.42,
  "friction_eur": 0.15,
  "solve_status": "INFEASIBLE",
  "slot_count": 96,
  "trigger": "periodic"
}
```

**D4 — UI: a distinct chip, not a warnings-list entry.**
`PlanHeaderBar.tsx` renders `warnings[]` as a count badge + expandable list, keyed by
`severity` (`CRITICAL`/`WARNING`). Folding `solve_status: Infeasible` into that list
(as today's fallback path already does, via a synthetic Critical warning with the
reason string) is exactly the bug this change fixes — a resident/operator currently
can't distinguish "the plan hit a real solver failure" from "the plan has a
non-critical caveat" without reading warning text. A separate chip driven directly by
`plan.solve_status`, rendered before/alongside the warnings badge, makes the
distinction visually immediate. The `fallback_plan`-produced Critical warning stays
(it still carries the human-readable reason) — the new chip is additive, not a
replacement for that detail.

## Risks / Trade-offs

- **[Risk] `fallback_plan`'s existing Critical warning and the new `Infeasible` chip
  now say overlapping things (both signal "solver failed").** → Mitigation: this is
  intentional duplication across two UI elements with different jobs — the chip is
  the at-a-glance signal (matches the Dashboard summary line planned in WP-T8), the
  warning entry keeps the detailed reason string for anyone who expands it. Not a
  new inconsistency; today only the warning exists and the plan doc already scoped
  the chip as *additive*.
- **[Risk] Two-state enum reads as under-scoped against the original three-state
  wording in `docs/plans/ven-ui-transparency.md` WP-T2.** → Mitigation: documented
  explicitly in proposal.md's Non-Goals and here; the plan doc's wording predates
  the code investigation that found no heuristic-solve path exists. This is a
  scope-narrowing correction, not a scope cut of anything reachable.
- **[Risk] Serde rename (`SCREAMING_SNAKE_CASE`) must match the UI's expected string
  exactly (`"OPTIMAL"` / `"INFEASIBLE"`) or the TS union silently mismatches.** →
  Mitigation: covered by an integration-level test asserting the exact JSON string
  for both variants, not just that the Rust enum round-trips.

## Migration Plan

Additive field on an existing struct/SSE variant — no migration needed. Old UI
builds against a new backend simply ignore the new fields (already doing so for
`objective_eur`/`friction_eur` today); no version gating required. Deploy order is
irrelevant (backend-first or UI-first both work); no data migration, no config
change, no Docker image beyond the normal rebuild.

## Open Questions

None — see `docs/plans/ven-ui-transparency.md` §5 for the plan-level open questions,
both already resolved before this WP was scoped.
