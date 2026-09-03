# Design: Reconcile Battery Round-Trip-Efficiency Models

## Context

See proposal.md for the full mismatch: `battery.rs` puts all round-trip loss on
charge (asymmetric); `battery_milp.rs` splits it as `sqrt(rte)` on both charge
and discharge (symmetric). Both are internally consistent and produce the same
total-cycle efficiency — they disagree only on the SoC trajectory *within* a
cycle.

**Worked example, to make the divergence concrete rather than asserted:** take
`round_trip_efficiency = 0.81` (so `sqrt(rte) = 0.9`) and charge with 10 kWh of
AC import, with no discharge yet.
- Simulator (`battery.rs`): stored energy = `10 * 0.81 = 8.1 kWh` (loss applied
  once, on the charge leg).
- Planner (`battery_milp.rs`): stored energy = `10 * sqrt(0.81) = 10 * 0.9 =
  9.0 kWh` (loss applied once, at half-strength, same charge leg).

That's an 8.1 vs. 9.0 kWh disagreement — about 11% relative — after a single
charge leg with no discharge involved at all yet. Both numbers are "correct"
under their own model's convention (verified algebraically: a full charge of P
kWh followed by a full discharge of everything just stored yields `P * rte`
kWh delivered back out under *either* model — `sqrt(rte)*sqrt(rte) = rte =
rte*1.0`), but the *simulator's real SoC* and the *planner's believed SoC*
diverge by a real, non-trivial amount the moment charge and discharge aren't
perfectly paired within one MILP planning cycle — which, under a 5-minute
rolling replan, is close to always.

## Goals / Non-Goals

**Goals:** pick one model, make both files agree, close the partial-cycle test
blind spot that let this ship.

**Non-Goals:** modeling temperature-dependent efficiency, calendar aging, or
any other battery-physics refinement beyond resolving this one disagreement.

## Decision needed (not yet made — for the user)

Two candidate resolutions, not yet chosen:

- **D-A: Make the simulator symmetric (match the planner).** Change
  `battery.rs::step_inner`/`forecast` to apply `sqrt(round_trip_efficiency)` on
  both charge and discharge, matching `battery_milp.rs`. Rationale for this
  direction: `sqrt`-split is the more standard textbook convention for
  round-trip efficiency when only one combined efficiency figure is available
  (no separately-measured charge/discharge efficiencies), and is already the
  MILP's existing, presumably deliberately-chosen convention.
- **D-B: Make the planner asymmetric (match the simulator).** Change
  `battery_milp.rs::build_milp_context`'s `eff_ch`/`eff_dis` derivation
  (currently both set to `self.round_trip_efficiency.sqrt()`, lines 227/
  235-236) to `eff_ch: self.round_trip_efficiency, eff_dis: 1.0`, matching
  `battery.rs`. Rationale for this direction: the simulator represents the
  "real" physical battery being simulated; if that's the ground-truth model
  the rest of the system was designed around, the planner's internal model
  should track it, not the other way around.

**Correction to an earlier draft of this document:** it previously justified
D-A partly on being "the smaller, more contained edit" versus D-B needing to
"rework the MILP's LP variable structure." That's not accurate — the
SoC-evolution constraint (`battery_milp.rs:76-78`,
`e[t+1] == e[t] + dt*eff_ch*p_ch[t] - dt*(1/eff_dis)*p_dis[t]`) already takes
independent `eff_ch`/`eff_dis` scalars; no constraint restructuring is needed
for either direction. D-B is exactly as small an edit as D-A — a two-line
change to `build_milp_context`'s derivation, not a different value plugged
into the same two fields. **Implementation cost is not a differentiator
between D-A and D-B; the choice rests entirely on which physical convention
is preferred.**

**Recommendation:** lean towards D-A (symmetric split) since it's the more
standard textbook convention for a single combined efficiency figure, but this
is a physics modeling call, not a refactor — confirm with the user before
implementing either direction.

## Verification

Test-first, per this repo's convention: write a **partial-cycle** test first
(e.g. charge 50% of available headroom, then discharge 50% of what was just
added, assert the resulting SoC) that fails under the current mismatch, then
implement the chosen direction until it passes, for both `battery.rs` and
`battery_milp.rs`'s existing test suites.
