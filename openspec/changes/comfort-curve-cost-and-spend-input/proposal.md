# Proposal

## Why

GB-53 (`docs/BACKLOG.md`). A comfort curve is a list of marginal €/kWh bids, correct for the
planner but opaque to a resident: nothing says what a curve can cost, or what the plan actually
spends on it, and nobody thinks "0.083 €/kWh up to 60 %" — they think "€2 to reach 60 %". The
planner already holds both numbers (the priced bands, and `AssetAllocation.cost_eur`); neither
reaches the UI (`ui-transparency`).

## What Changes

Two independently shippable parts, **Part A first**:

- **Part A — cost beside the curve.** The comfort-curve card shows, next to the chart, the most the
  curve can cost for the charge ahead (price-only area under the curve, from the current charge to
  the vehicle's limit) and what the current plan spends on that asset. A preview route computes the
  maximum for the rows being edited, so it is visible before Save.
- **Part B — cumulative-spend input.** The card edits an EV curve as cumulative points ("60 % for
  €2.00, 80 % for €2.50"). The VEN differentiates them into the marginal bids it already stores and
  plans on; the stored form, the planner and the MILP do not change. The same card shows any stored
  curve back as cumulative spend. A curve that is not concave in spend is refused with a message in
  spend vocabulary.
- Not changed: `budget_eur` / `max_total_cost_eur` (hard per-session cost ceilings in the MILP) stay
  separate. A curve's own area already bounds what it can buy, so it needs no second cap.

## Capabilities

### New Capabilities
- `comfort-curve-cost`: the maximum cost of a comfort curve and the planned spend, exposed beside the curve.
- `comfort-curve-spend-input`: entering and reading a comfort curve as cumulative spend.

### Modified Capabilities
<!-- none: openspec/specs/ is empty by project rule; the existing comfort-curve behaviour is
     documented in docs/ and pinned by tests/features/ven_comfort_curve.feature -->

## Impact

- VEN Rust: `assets/asset_trait.rs` (+ `ev.rs`), `assets/ev_comfort.rs`, `entities/comfort.rs`,
  `controller/sim_read_port.rs`, `routes/hems/comfort.rs`, `controller/milp_planner/session_flexibility.rs`
  (shared planned-cost function), `ui_types.rs` (generated TS types).
- VEN UI: `components/devices/ComfortCurveCard.tsx`, `api/hooks.ts`, `api/client.ts`.
- Tests: Rust unit tests, `ComfortCurveCard.test.tsx`, two `@use_case` scenarios in
  `tests/features/ven_comfort_curve.feature` (+ `steps/comfort_steps.py`).
- Docs: `docs/architecture/ven_milp_planner.md` §10, `docs/use-cases/COMFORT-PERSONAS-USE-CASE-MANUAL.md`;
  remove GB-53 from `docs/BACKLOG.md`; journal and KEY_LEARNINGS entries.
- No new dependency, no planner change, no VTN or OpenADR wire change (VEN-local HTTP API only).
