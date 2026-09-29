# Proposal

## Why

An EV charge request is valued today as one **all-or-nothing block**: the planner either
delivers the whole "core" (current SoC → `soc_target`) or nothing at all, decided by a single
binary `z_ev_core`. When the user's comfort bid does not cover that whole block at the
prevailing cost, the optimum is zero kWh — not a partial charge. That is the confirmed
mechanism behind GB-41, where four of nine fleet VENs charged **nothing for 24 h** while
solving OPTIMAL, reproduced offline in
`VEN/src/controller/milp_planner/tests/gb41_soft_deadline_core.rs`.

The binary was built for a threshold need — "I need 60 % to reach the destination, otherwise I
take the train". That case does not justify it: a genuine threshold belongs in a **firm**
deadline (a hard constraint that cannot decline), and unlike a wash cycle, energy bought short
of a target is not wasted — it stays in the battery for the next trip. The discontinuity the
model asserts does not exist in the physics.

## What Changes

- **BREAKING (semantics):** the comfort curve's `fill` axis for an EV means **state of charge**
  (0 = empty, 1 = full), not task completion. Existing curves are reinterpreted on this axis.
- **BREAKING (semantics):** on a **soft** request, `soc_target` stops being a promise. It is
  where the user's bid typically drops. On a **firm** request it remains a guarantee.
- **BREAKING (valuation):** a declining curve now buys **materially less** than the same curve
  did before. The old model read one point (`value_at_fill(0.0)`) and applied it as a lump to the
  whole block, so a 0.35 → 0.05 ramp was worth 0.35 €/kWh everywhere. Read marginally, the same
  ramp averages about 0.20 €/kWh, which is below a realistic cost of tariff plus the 0.22 €/kWh
  controllable-import malus — it would buy almost nothing. This is the correct reading (a bid for
  the *next* kWh is what a comfort curve means), so the built-in default in
  `assets/ev.rs::default_comfort_rates` is re-drawn from 0.35 → 0.05 to **0.45 → 0.30** to keep
  the same practical willingness to charge, and any hand-drawn curve must be re-read the same
  way. `tests/modes.rs` pins the default against exactly this regression.
- Remove the EV's `z_ev_core` binary, `e_core_kwh`, and the separate "extra" energy block. One
  comfort curve prices every kWh from the current SoC to 100 %, piecewise, per kWh.
- Charging degrades gracefully: a bid that covers part of the energy buys that part, instead of
  buying nothing.
- Firm requests gain an explicit hard floor: deliver at least
  `(soc_target − soc_init) × battery_kwh` by the deadline.
- `validate_curve` rejects a curve whose bid **rises** with fill (with a message naming the
  point), which is what keeps the valuation concave and solvable with continuous variables
  only — no binaries reintroduced.
- `EV_CORE_ENERGY_UNMET` narrows to firm-request shortfall and carries the numbers (delivered
  vs required), ending its current double meaning.
- One binary fewer in a MILP that already brushes its per-phase solver timeout (R-97).

## Capabilities

### New Capabilities
- `ev-charge-valuation`: how an EV charge request's energy is valued and delivered — the
  comfort curve as a per-kWh bid over state of charge, the difference between a firm guarantee
  and a soft preference, and what the planner must do when a bid covers only part of the
  energy.

### Modified Capabilities
<!-- None: `openspec list --specs` reports no existing specs in this project. -->

## Impact

**Planner / solver**
- `VEN/src/assets/ev_milp.rs` — `declare_vars`, `constraints`, `objective`, `read_solution`,
  and the per-`UserRequestMode` branches of `from_state`.
- `VEN/src/assets/ev_comfort.rs` — `resolve_ev_comfort_reward` becomes the segment builder.
- `VEN/src/controller/milp_planner/asset_port.rs` — `EvMilpVars`, `EvMilpContext`,
  `EvSolOutput` (drops `z_ev_core`); `VEN/src/controller/asset_milp_port.rs::EvScalars`;
  `VEN/src/controller/milp_planner/inputs.rs`.
- `VEN/src/controller/milp_planner/solver_phase2.rs` and `solver_duals.rs` — one fewer binary
  to pin/fix between phases.
- `VEN/src/controller/milp_planner/ev_diagnostics.rs` — the warning's meaning.

**Validation / API**
- `VEN/src/services/comfort.rs::validate_curve` — new non-increasing-bid rule (a 400 where
  today a curve is accepted).

**UI**
- `VEN/ui/src/components/devices/ComfortCurveCard.tsx` (axis wording), `EvCard.tsx`,
  `VEN/ui/src/pages/PlanHistory.tsx` (warning label), `VEN/ui/src/api/types.ts`.

**Tests**
- `VEN/src/controller/milp_planner/tests/gb41_soft_deadline_core.rs` — the nine offline probes
  are the regression harness; two state the old all-or-nothing behaviour and must be rewritten
  to state the new.
- `tests/features/` — a new scenario for partial delivery under a low bid.

**Records**
- `docs/BACKLOG.md` GB-41 closes as resolved-by-removal; `docs/reference/TECHNICAL_DEBTS.md`
  R-95's warning-kind collision note drops.

**Not affected**
- The fleet runs entirely on `MustRun` (`usage_forecast`'s `engage_charge_planning`), so the
  soft path carries no production traffic today. Free/opportunistic (`reward_per_slot`) modes
  keep their flat per-slot reward: they are gated by PV surplus, not by price.
