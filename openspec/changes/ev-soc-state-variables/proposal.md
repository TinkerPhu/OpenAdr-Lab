# Proposal

## Why

Resolves **R-93** (`docs/reference/TECHNICAL_DEBTS.md`, Severity 🟠 Medium-High)
and with it **R-92**, which that entry names as blocked by it: *"Pairs naturally
with R-92 — both are the same 'generalize the EV model' work."*

The EV is modelled in the MILP as **total energy delivered before one deadline**.
There is no per-slot state-of-charge variable and no SoC-balance constraint, so
the SoC curve — trip drops included — is reconstructed *after* the solve by
`asset_port::ev_soc_trajectory`. The solver therefore cannot reason about SoC
mid-horizon: it can avoid charging while the car is away, but it cannot plan the
recharge that a predicted *return* makes possible, and it cannot carry charge
across one departure toward a later deadline.

Every shipped behaviour is correct under the projection approach; what is missing
is planning capability. But the shape is now actively blocking: a queue of
charging sessions (the `ev-session-queue-foundation` change) needs each session's
target to bind at that session's own departure, with the SoC between sessions
chained by the model. Without SoC variables, that chaining has to be predicted
outside the solver — which would make a *third* place that computes this EV's
future SoC, alongside `ev_soc_trajectory` and `ev_schedule::soc_drop_frac_per_slot`.
That is the duplication `one-concept-one-function` exists to prevent, so the model
is generalised first and the queue is built on it.

The battery already has exactly the required shape: `e_bat` as `n+1` energy-state
variables with a per-slot balance constraint
(`VEN/src/assets/battery_milp.rs:30,76`). This change gives the EV the same
primitive rather than inventing a second one.

## What Changes

- The EV gains per-slot state-of-charge variables and a **SoC-balance
  constraint** chaining them through charging power and exogenous trip drops,
  following the battery's `e_bat` pattern.
- A charging obligation becomes a **bound on the SoC variable at its deadline
  step** instead of a cumulative-energy sum over the slots before it.
- **BREAKING** (internal planner API only): the scalar deadline pair
  (`t_dead_step`, `e_required_kwh` on `EvMilpContext`; `t_ev_dead_step`,
  `e_ev_required_kwh` on `MilpInputs`) becomes a **list of obligations**, each a
  (deadline step, target SoC) pair — resolving R-92's recorded fix. This change
  populates that list with at most one entry, exactly as today; the queue change
  is what fills it with more.
- The exogenous trip drop moves from a post-solve correction into the balance
  constraint, so the plan's own SoC curve is the solved one.
- `ev_soc_trajectory`'s **post-solve reconstruction is deleted**; the SoC
  trajectory is read back from the solved variables, as the battery's already is.
- The reachable-energy cap on a firm obligation is replaced by an explicit
  **shortfall slack variable** in the model, so an unreachable target is expressed
  by the model rather than pre-clamped by the caller, and the reported shortfall
  is the solved slack.
- Post-return recharge becomes plannable: a predicted return inside the horizon
  can be charged for.

## Capabilities

### New Capabilities
- `ev-soc-planning`: the planner reasons about the EV's state of charge over the
  horizon — what the projected SoC means, how trip drops and charging combine, and
  what an unmet charging target reports.

### Modified Capabilities
<!-- `openspec/specs/` is empty: implemented specs are folded into `docs/` and
     deleted (workflow rule 3). Nothing to delta against. -->

## Impact

Code:
- `VEN/src/assets/ev_milp.rs` — variables, balance constraint, obligation bounds,
  shortfall slack, solution readback.
- `VEN/src/controller/milp_planner/asset_port.rs` — `EvMilpContext` obligation
  list, `EvMilpVars` SoC variables, `EvSolOutput` solved trajectory;
  `ev_soc_trajectory` deleted.
- `VEN/src/controller/milp_planner/types.rs`, `inputs.rs` — `MilpInputs` EV
  obligation list.
- `VEN/src/controller/milp_planner/ev_diagnostics.rs` — shortfall read from slack.
- `VEN/src/controller/milp_planner/envelopes.rs` — reads `e_ev_required_kwh`.
- `VEN/src/assets/ev_session_context.rs`, `ev_usage_forecast.rs` — build
  obligations instead of the scalar pair.
- `VEN/src/assets/ev.rs` — the `ev_soc_trajectory` tests move to the solved path.

Performance: adds `n+1` continuous variables and `n` equality constraints per EV.
R-97 measured EV phase 2 at 11–13 s, so the delta must be measured before merge.

Dependencies: none added. No wire-format or HTTP change.
