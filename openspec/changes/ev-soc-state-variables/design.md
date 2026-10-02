# Design

## Context

See `proposal.md` — Why. This resolves R-93 and R-92
(`docs/reference/TECHNICAL_DEBTS.md`), and is the prerequisite for
`ev-session-queue-foundation`.

**Where the concept lives today** (`one-concept-one-function` inventory):

| Concept | Where it lives today | What happens to it |
| --- | --- | --- |
| EV charging power per slot | `EvMilpVars.p_ev` — `controller/milp_planner/asset_port.rs:140` | Unchanged |
| EV energy obligation | `EvMilpContext.t_dead_step` + `.e_required_kwh` — `asset_port.rs:92,101` | Becomes `obligations: Vec<EvObligation>` |
| Same pair, planner-input layer | `MilpInputs.t_ev_dead_step` + `.e_ev_required_kwh` — `milp_planner/types.rs:169,177`, filled at `inputs.rs:404,407` | Becomes the same obligation list — **two copies of one concept collapse into one** |
| Energy summed to the deadline | `EvMilpContext::energy_expr` — `assets/ev_milp.rs:96` | Deleted; the obligation binds SoC instead |
| What the window can deliver | `EvMilpContext::reachable_energy_kwh` — `ev_milp.rs:109` | Deleted; replaced by the slack variable |
| Firm floor, capped | `ev_milp.rs:170` `min(e_required_kwh, reachable_energy_kwh)` | Replaced by SoC bound + slack |
| Post-solve SoC curve | `asset_port::ev_soc_trajectory:324` | **Deleted** — read back from solved vars |
| Exogenous trip drops | `EvMilpContext.soc_drops: Option<ExogenousSocDrops>` fed by `ev_schedule::soc_drop_frac_per_slot` | Input to the balance constraint instead of a post-solve correction |
| Shortfall diagnostic | `milp_planner/ev_diagnostics.rs::firm_shortfall:26` | Reads the solved slack |
| Energy needed, for envelopes | `milp_planner/envelopes.rs:65` reads `e_ev_required_kwh` | Reads the obligation list |
| **The pattern to copy** | `BatteryMilpVars.e_bat` (`n+1` vars) + balance constraint — `assets/battery_milp.rs:30,76` | **Reused as the shape, not re-invented** |

**The battery is the precedent.** `battery_milp.rs` declares `e_bat` over `0..=n`,
pins index 0 to `e_init_kwh` by equal min/max bounds, bounds the rest to
`[e_min_kwh, e_max_kwh]`, and chains them with one equality per slot
(`e_bat[t+1] == e_bat[t] + dt*eff_ch*p_ch[t] - dt/eff_dis*p_dis[t]`). The EV gets
the same construction. `naming-transparency` note: the battery names its state in
energy (`e_bat`, kWh); the EV's user-facing concept is state of charge, and
`EvMilpContext.soc_init` and the UI both say "soc", so the EV's variable is
`soc_ev[t]` as a fraction, with the pack size converting power to SoC inside the
balance constraint.

**Constraint — one EV mode arm is not energy-shaped.** `EvMilpContext::from_state`
(`assets/ev_session_context.rs`) has arms where `e_required_kwh` is 0 and all value
comes from priced bands (`segments` × `e_seg`) or per-slot rewards
(`Opportunistic`, `AsapFree`, `MaxCost`, `ByDeadlineFree`). Those arms must keep
working unchanged: this change alters how a *firm obligation* is expressed, not how
energy is valued. The band accounting equality `ev_energy == bought`
(`ev_milp.rs:154`) is what ties priced bands to actual power and must survive.

## Goals / Non-Goals

**Goals:**

- One SoC authority for the EV inside the planner: the solved variables.
- One representation of a charging obligation, shared by `EvMilpContext` and
  `MilpInputs` (today two copies of the same scalar pair).
- R-92's "list of obligations" resolution, with the list holding ≤ 1 entry here.
- Post-return recharge becomes expressible.
- Byte-identical behaviour for every existing single-deadline scenario.

**Non-Goals:**

- No change to how energy is *valued* (comfort bands, per-slot rewards, budgets).
- No charging-efficiency factor on the EV (the battery has `eff_ch`; the EV model
  has never had one and adding it would change plan quantities).
- No session queue — that is the next change. The obligation list is built with at
  most one entry here.
- No change to `ev_schedule`'s trip generation or per-slot drop computation; this
  consumes them.
- No V2G / discharge. `p_ev` stays non-negative.

## Decisions

### Decision 1 — `soc_ev[t]` over `0..=n`, pinned at index 0, following the battery

`EvMilpVars` gains `soc_ev: Vec<Variable>` of length `n+1`. Index 0 is pinned to
the live SoC by equal min/max bounds (`variable().min(soc_init).max(soc_init)`),
exactly as the battery pins `e_bat[0]` to `e_init_kwh`. Indices `1..=n` are bounded
`[floor_frac, 1.0]`.

*Why a fraction rather than kWh*: `EvMilpContext.soc_init` is already a fraction,
the UI says "soc", and the plan's existing EV trajectory is a fraction — making the
variable a fraction means the readback is the plan's value with no conversion, and
no second unit for one quantity. The pack size appears once, inside the balance
constraint. Per the `naming` rule the variable carrying a percentage-style fraction
keeps the project's existing `soc` wording rather than gaining a `_pct` suffix,
since it is 0..1 and every existing `soc` field in this model is too.

*Why not reuse `ExogenousSocDrops.floor_frac` as the variable's lower bound only*:
it is used as the bound **and** in the balance constraint's floor behaviour — see
Decision 3, where the distinction matters.

### Decision 2 — The balance constraint is an equality, with the drop as a constant

For each slot `t`:

    soc_ev[t+1] == soc_ev[t] + (dt_h[t] / battery_kwh) * p_ev[t] - drop_frac[t]

`drop_frac[t]` is a constant from `ExogenousSocDrops.drop_frac_per_slot` (zero in
most slots), i.e. the same numbers `soc_drop_frac_per_slot` already produces and
that `ev_soc_trajectory` applies post-solve today. No new source, no second rule.

*Why an equality and not `<=`*: SoC is a physical state, not a budget; an
inequality would let the solver discard charge to dodge an upper bound, which would
show as a plan whose own SoC curve disagrees with its power schedule — exactly the
plan/actual divergence class this project treats as a bug.

### Decision 3 — The floor is a variable bound, and the drop is *clamped* by a slack on the drop, not by weakening the equality

The live tick floors a drop (`apply_return_drop`: `(soc - drop).max(floor)`), and
`ev_soc_trajectory` mirrors that. A bare equality plus a `>= floor_frac` bound
would make a large drop **infeasible** instead of floored, which would turn a
harmless deep trip into a failed site solve.

So the drop term carries a non-negative slack `drop_unmet[t]`, used only in slots
where `drop_frac[t] > 0`:

    soc_ev[t+1] == soc_ev[t] + (dt_h[t] / battery_kwh) * p_ev[t] - drop_frac[t] + drop_unmet[t]
    drop_unmet[t] <= drop_frac[t]

and `drop_unmet[t]` is penalised in the objective at a rate high enough that the
solver never buys it for free, but finite so it is preferred over infeasibility.
`soc_ev[t+1] >= floor_frac` then floors the result exactly as the tick does: the
slack can only absorb the part of the drop that would have gone below the floor.

*Why not simply pre-clamp the drop constants*: the clamp depends on the SoC at the
moment of the drop, which is what the solver is deciding — the existing code comment
in `ev_schedule::daily_trip` says precisely this ("clamping depends on the SoC at
return, not on the trip alone"). Pre-clamping would need the answer before solving.

*Alternative considered*: allow `soc_ev` to go below the floor and clamp on
readback. Rejected — the plan's SoC curve would then not be the solved one, which
is the defect this change exists to remove.

### Decision 4 — An obligation is a SoC bound at its deadline step, with a shortfall slack

`EvObligation { deadline_step: usize, target_soc: f64, session_id: Option<Uuid> }`,
and per obligation one slack `shortfall_soc` with:

    soc_ev[deadline_step] + shortfall_soc_k >= target_soc_k
    shortfall_soc_k >= 0

penalised in the objective above any tariff or comfort bid, so the solver only buys
shortfall when the window physically cannot deliver.

This deletes `reachable_energy_kwh` and the `min(...)` pre-cap at `ev_milp.rs:170`.
R-93's entry names this as the point of the exercise: *"with a real SoC balance plus
slack the shortfall would be expressible in the model instead of pre-clamped."*
`ev_diagnostics::firm_shortfall` then reports `shortfall_soc_k * battery_kwh` as
kWh, which is the amount actually missed rather than a pre-solve estimate — and it
can name the obligation, which the scalar pair could not.

*Why a penalised slack over a hard bound*: a hard bound makes an unreachable target
an infeasible site solve. The existing behaviour (charge what the window allows,
report the gap) is the behaviour to preserve; slack is how the model expresses it.

*Penalty calibration*: strictly greater than the largest comfort bid and the
largest tariff spread over the horizon, so shortfall is never cheaper than charging
when charging is possible; derived from the existing cost scale rather than a magic
constant, and pinned by a test that a reachable target is never traded for slack.

### Decision 5 — `energy_expr` goes; the band-accounting equality stays, horizon-wide

`energy_expr` summed power to `t_dead_step` for the obligation. With the obligation
now a SoC bound, its only remaining caller is the band-accounting equality
`ev_energy == bought`, which must still hold so priced bands cannot be rewarded
without moving `p_ev`. That equality becomes whole-horizon (all `n` slots), which
is also what it has to be once more than one obligation exists.

*Consequence to verify*: in the single-obligation case, energy charged *after* the
deadline previously fell outside the equality. The arms where `e_extra_max_kwh` is
non-zero already allow beyond-target charging, so the test matrix in
`ev_milp.rs`/`tests/solver.rs` must confirm no arm changes its totals.

### Decision 6 — `ev_soc_trajectory` is deleted; readback reads the variables

`EvSolOutput` gains `soc_ev: Vec<f64>` read from the solved variables, as
`BatterySolOutput` already does for `e_bat` (`battery_milp.rs:135`). The function
`asset_port::ev_soc_trajectory` and its callers go away. Its unit tests
(`assets/ev.rs:887-955`) are rewritten against the solved path rather than deleted —
they encode real behaviours (monotonic charge, the floor, the drop timing) that must
still hold, so they become solver-level tests asserting the same properties.

This also discharges the second half of R-73, which notes `ev_soc_trajectory` as
the one consolidated EV integrator; with it deleted there is no integrator to keep
in sync at all.

### Decision 7 — `MilpInputs` and `EvMilpContext` share one obligation type

Today the same scalar pair exists twice: `EvMilpContext.t_dead_step`/`e_required_kwh`
and `MilpInputs.t_ev_dead_step`/`e_ev_required_kwh`, copied across at
`inputs.rs:404-407`. Both become `Vec<EvObligation>` of one shared type, so there is
one definition of "a charging obligation" in the planner. This is the R-92 fix as
its entry words it ("generalize the pair into a list … in `EvScalars`/`MilpInputs`").

## Risks / Trade-offs

- **Solve time**: `n+1` continuous variables, `n` equalities, plus slacks per EV, on
  a model whose phase 2 is already 11–13 s (R-97) → measure before merge against the
  R-97 battery+EV baseline and record the delta in the journal; the variables are
  continuous (no new binaries), which is the cheap kind.
- **A mis-calibrated shortfall penalty silently buys slack** instead of charging →
  pinned by a test that a reachable target is always met and its slack is zero, and
  by a test that an unreachable one reports exactly the missing kWh.
- **The drop-slack could be abused to dodge a drop** in slots where the floor is not
  actually binding → `drop_unmet[t] <= drop_frac[t]` plus the penalty bounds it; a
  test asserts a drop that keeps SoC above the floor is applied in full.
- **A behaviour change hiding in a non-firm mode arm** (Decision 5's horizon-wide
  equality) → the existing arm-by-arm test matrix must pass unchanged; any arm whose
  totals move is a finding to explain, not to re-baseline (per the project's
  test-failure rule).
- **R-21 heap flake** around the heaviest HiGHS tests may obscure a real regression →
  run the affected module in isolation when the full suite crashes, as that debt
  entry prescribes.
- **File-size caps**: `ev_milp.rs` grows (vars + balance + slacks) → split the
  obligation/slack construction into its own module beside it if
  `scripts/audit_file_sizes.py` flags the 500-line cap.

## Migration Plan

Internal-only: no wire format, no HTTP shape, no persisted state. `Plan`'s EV SoC
series keeps its existing field and meaning — it is now solved rather than
reconstructed, so the UI needs no change (and that is the test: the same plan JSON
shape, same series name).

Rollback is a revert. Nothing depends on this change until
`ev-session-queue-foundation` builds on it.

## Open Questions

- Should the drop-slack penalty and the shortfall penalty be one configured cost or
  two? Deferrable: both are derived from the cost scale, and collapsing them later
  changes no behaviour if each is already provably never bought when avoidable.
