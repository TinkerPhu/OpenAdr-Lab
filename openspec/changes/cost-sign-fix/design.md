## Context

`VEN/src/controller/milp_planner/results.rs::translate_to_plan` builds the `Plan`'s decision
matrix from the solved MILP variables. Each of the four asset-allocation blocks (EV, heater,
shiftable-load, battery-charging) independently computes `AssetAllocation.cost_eur` for its slot
as:

```
cost_eur = grid_power_kw * import_tariff_eur_kwh * dt_h
         - surplus_power_kw * export_tariff_eur_kwh * dt_h   // credit convention (old)
```

`VEN/src/controller/milp_planner/envelopes.rs::solved_session_cost()` (added during the BL-36
`FlexibilityEnvelope` rebuild) computes the same kind of quantity for the session-total estimate
as:

```
cost_eur = grid_power_kw * import_tariff_eur_kwh * dt_h
         + surplus_power_kw * export_tariff_eur_kwh * dt_h   // opportunity-cost convention (new)
```

The `+` convention treats surplus PV consumed by the asset as forfeited export revenue (the
asset "spends" what could have been sold to the grid), which is honest: it never makes
surplus-covered energy look *cheaper than free*. The `−` convention makes it look like a credit,
which understates cost and — because two different code paths now disagree — makes the Planner
tab (per-slot, from `translate_to_plan`) and the envelope panel (session total, from
`solved_session_cost`) show inconsistent signs for the same underlying data.

## Goals / Non-Goals

**Goals:**
- Make all four `AssetAllocation.cost_eur` blocks in `translate_to_plan` use the same
  opportunity-cost (`+`) convention as `solved_session_cost()`.
- Keep the decision matrix (`AssetAllocation.cost_eur`, per-slot/per-asset) and the envelope
  (`FlexibilityEnvelope.estimated_cost_eur`, session total) in sign agreement for the same
  PV-surplus scenario.

**Non-Goals:**
- No change to `solved_session_cost()` — it already uses the correct convention and is the
  reference.
- No change to `PlanSummary.total_cost_eur` / `CostBreakdown.c_energy_eur` (grid-level cost,
  computed independently as `net_import * import - net_export * export`) — those aggregate at
  the grid boundary, not per-asset, and are not part of this item's scope per BL-40's own note.
  If a future review finds they should also change, that is a separate backlog item.
- No change to MILP solver objective weights or constraints — `cost_eur` is a post-solve
  reporting computation, not an optimization input.
- No UI code changes — the Planner tab already renders whatever `cost_eur` the backend sends.

## Decisions

**Flip the sign in-place in each of the four blocks, rather than extracting a shared helper.**
`results.rs` computes `cost_eur` inline per asset block because each block also derives
`grid_power_kw`/`surplus_power_kw` differently from that asset's MILP variables. Introducing a
shared `fn allocation_cost_eur(grid_power_kw, surplus_power_kw, import_tariff, export_tariff,
dt_h) -> f64` helper is a reasonable follow-up but is a refactor beyond this fix's scope (BACKLOG
item is a "sign flip", not a restructuring); doing only the minimal sign change keeps the diff
easy to review against the existing tests block-by-block. Considered: extracting the helper now
since it would prevent future drift — rejected for this change to keep risk and diff size
minimal, but noted as a natural follow-up if a fifth allocation block is ever added.

**Verify sign agreement with a cross-check test, not just four isolated unit tests.**
Per BL-40's own Verify note, add one test that runs a PV-surplus scenario spanning multiple asset
types and asserts the decision matrix total and `solved_session_cost()`'s total agree in sign —
this is the regression guard for the actual bug (the two computations silently diverging again).

## Risks / Trade-offs

- [Risk] Existing tests assert the old (`−`) sign and will need updating, which could mask a
  wrong flip if updated mechanically without re-deriving the expected value → Mitigation: for
  each block, recompute the expected `cost_eur` by hand from the test fixture's known
  `surplus_power_kw`/`export_tariff_eur_kwh` rather than just negating the old assertion.
- [Risk] Downstream code might read `AssetAllocation.cost_eur` and sum it into something that
  currently (accidentally) relies on the old sign → Mitigation: grep all call sites of
  `AssetAllocation.cost_eur` / `.cost_eur` field access in `VEN/src/` and `VEN/ui/src/` before
  flipping, confirm none double-compensate for the old convention.

## Migration Plan

Single-PR change, no data migration (no persisted `cost_eur` values — computed fresh per plan
solve, and plans are not persisted across restarts per existing architecture). No rollback
concerns beyond reverting the commit.
