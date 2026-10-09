# Planner solve-cost benchmarks (R-97 / GB-40)

Measured results for the two-phase MILP planner's solve cost. Companion to
`GB40_MIP_GAP_BENCHMARK.md`, which covers the MIP-gap sweep only.

**Why this file exists.** These tables were originally hand-copied into
`TECHNICAL_DEBTS.md` R-97, which turned a debt register into a measurement log.
Numbers belong where they can be regenerated and compared; the debt entry keeps
the *conclusions* and points here for the evidence.

## How results are stored, and what is deliberately not stored

Results fall into three classes, and only one of them needs disk:

| class | example | reproducible? | storage |
|---|---|---|---|
| **1. Deterministic benchmark** | `bench_phase1_vs_tank_slack` — switch counts identical across 3 runs | yes, from committed code | **recipe only**: bench name + commit SHA + the summary table below. Raw output: none. |
| **2. Non-reproducible measurement** | time-limited phase-2 solves (the same 60 s budget gave friction 2.0805 and 3.4555); any fleet observation | **no** — gone forever if not captured | must be written down. A few KB each. |
| **3. Derived artefact** | `VEN/target` (11 GB), docker build cache (~70 GB per host) | yes, mechanically | **reclaim, never archive** |

The practical consequence: this project has no results-volume problem. Every
measurement in this document is a few hundred bytes; the whole session that
produced them is well under 100 KB. The disk pressure is entirely class 3, and the
answer there is a retention policy, not compression.

A deterministic benchmark's table is therefore the *compressed form* of its raw
output — regenerating it costs one command and is more trustworthy than a stale
archive. A non-reproducible measurement has no such fallback, which is exactly why
time-limited solver results must be recorded at the time they are taken.

## Phase 1 at production settings: no cliff, and the production grid is the best 48 h grid

**Corrected 2026-10-01.** An earlier version of this section reported a "60 s cliff" at 288
slots/48 h and concluded that phase-1 cost scales with horizon duration. That was measured at
`mip_gap_target` **0.02** (the code default) while every heater VEN in the fleet runs **0.06**. At
the production gap the cliff does not exist.

`bench_phase1_count_vs_duration`, slack-poor heater (200 L / 15 K), fully priced, gap the only other
variable:

| grid | slots | hours | gap 0.02 | gap **0.06** (production) |
|---|---|---|---|---|
| 96x300s | 96 | 8 | 0.45 s GapLimit | 0.37 s GapLimit |
| 288x300s | 288 | 24 | 2.14 s GapLimit | 1.44 s GapLimit |
| 192x900s | 192 | 48 | 25.78 s GapLimit | 9.77 s GapLimit |
| 96x300+96x600+96x900 (**production**) | 288 | 48 | **58.28 s TimeLimit** | **3.89 s GapLimit** |

Two things follow:

- **There is no horizon cliff at production settings.** 3.89 s, GapLimit, on the full 48 h grid.
- **The production 3-zone grid is the fastest 48 h option measured** — better than uniform 192x900s
  (9.77 s) despite having 50 % more slots. Zone structure matters more than slot count, and the
  existing choice is a good one.

So phase-1 optimisation is not where the time goes. The 48 h span, which is a requirement, costs
~4 s at the gap the fleet actually runs.

**Far-zone coarsening does not help** (`bench_phase1_vs_zones`, all 48 h, gap 0.06, slack-poor
heater):

| grid | slots | phase 1 | switches 8 h | first-8 h EUR | objective |
|---|---|---|---|---|---|
| 96x300+96x600+96x900 (**production**) | 288 | 5.33 s | 28 | 1.5088 | -7.0071 |
| 96x300+48x1200+24x3600 | 168 | 6.11 s | 28 | 1.6652 | -6.1022 |
| 96x300+24x2400+12x7200 | 132 | 7.80 s | 28 | **+37.3964** | — |
| 48x300+44x1200+28x3600 | 120 | 1.81 s | 18 | 1.4486 | -6.9275 |

Coarsening *only* the far zones made it **slower** (5.33 -> 6.11 -> 7.80 s). The earlier
"192 slots beat 288" reading was a uniform-900 s grid, i.e. coarsening everything, not the far
horizon — a misreading of that experiment. The one variant that got faster halved the **near** zone
(96x300s -> 48x300s), so fine slots are the expensive ones, and that trades away executed
resolution: switches in the first 8 h fell 28 -> 18 because the window can represent fewer.

**Hard limit found:** 7200 s (2 h) steps break the model — objective +37.40 EUR against ~-7 EUR
elsewhere. Do not coarsen beyond ~3600 s.

## Phase 1: difficulty is monotone in the tank's thermal slack

`bench_phase1_vs_tank_slack`. Volume and band varied; grid, tariffs, gap (0.06) and
assets held fixed. Three repeats. Switch counts are identical across runs — these
solves finish, so they are deterministic.

| volume L | band K | slack kWh | phase 1 (3 runs) | switches |
|---|---|---|---|---|
| 200 | 15 (**ven-3**) | 3.5 | 1.29 / 1.22 / 2.00 s | 53 |
| 200 | 40 | 9.3 | 0.66 / 0.70 / 1.21 s | 14 |
| 500 | 15 | 8.7 | 0.61 / 0.61 / 0.67 s | 12 |
| 1000 | 15 | 17.4 | 0.44 / 0.42 / 0.50 s | 16 |
| 2000 | 15 | 34.9 | 0.22 / 0.18 / 0.19 s | 4 |
| 2000 | 40 (**ven-2**) | 93.0 | 0.19 / 0.21 / 0.24 s | 22 |

Switch count does **not** predict difficulty (2000 L/40 K: 22 switches, fastest).
Slack does.

## Phase 2: value, budget, and the epsilon threshold

`bench_phase2_epsilon_sweep`, diurnal tariff, 60 s phase-2 budget. Slot counts out
of 288.

| mip_gap | epsilon | heat slots moved | friction |
|---|---|---|---|
| 0.02 | 0.17 | 51 | 2.2916 |
| 0.02 | 0.50 | 0 | 4.9113 |
| 0.02 | 1.00 | 47 | 1.9949 |
| 0.02 | 5.00 | 74 | 1.2471 |
| 0.06 | 0.17 | 0 | 4.6201 |
| 0.06 | 0.50 | 59 | 1.8315 |
| 0.06 | 1.00 (**ven-2**) | 68 | 2.0805 |
| 0.06 | 5.00 | 87 | 1.4559 |

**This table is class 2, not class 1** — phase 2 always hits its time limit, so these
numbers are not reproducible and a re-run will differ. It also contains a proof that
phase 2 does not solve its own model: raising epsilon strictly enlarges the feasible
set, so friction cannot rise, yet it goes 2.2916 -> 4.9113 (gap 0.02, epsilon
0.17 -> 0.50) and 1.8315 -> 2.0805 (gap 0.06, 0.50 -> 1.00).

Budget (`bench_phase2_budget_with_real_prices`, gap 0.06, epsilon 1.00): friction
4.6201 at 1-2 s (no change), 3.4555 at 5/10/20 s, 2.0805 at 60 s — **one run**. A
second run gave 3.4555 at 60 s as well, which is why the plateau must not be treated
as a stable property.

Executed window (`bench_phase2_budget_executed_window`): 5 s and 60 s give an
identical near-term schedule — 2 heater switches at 25 min and at 1 h under both —
and phase 2 halves switching against phase 1 alone (58 -> 32 over 48 h, 28 -> 11
over 8 h).

## Live fleet observations (class 2 — not reproducible)

`planner: phase timings` log, Node1, 2026-09-30.

| VEN | phase 1 | phase 2 | epsilon | friction |
|---|---|---|---|---|
| ven-2 | 227-309 ms | 13-15 s, sometimes GapLimit | 1.00 | ~1.33 |
| ven-3 | 11 131-45 566 ms | ~15 s, always TimeLimit | 0.17 | 4.30-5.60 |

Totals from `plan_history`, median over each regime:

| VEN | 60 s phase-2 budget | 5 s | 15 s |
|---|---|---|---|
| ven-2 | 64.0 s, TL 57 % | 6.9 s, TL 94 % | 13.4 s, TL 33 % |
| ven-3 | 72.2 s, TL 90 % | 17.1 s, TL 100 % | 55.9 s, TL 100 % |

The bench's hardest phase-1 row is 1.3-2.0 s against ven-3's live 11-46 s, so the
bench under-represents production by roughly an order of magnitude. Direction is
confirmed; absolute scale is not the bench's to give.


## Solve cost by fleet asset-mix class — binary interaction is real

`bench_asset_mix_solve_cost`, gap 0.06, epsilon 1.00, 288 slots, ven-3's tank where a heater is
present, ven-5's battery (11 kWh, 5.5 kW, eta 0.92, min_soc 0.10). Both phases timed separately.

| class | VENs | phase 1 | phase 2 | p1 status | p2 status |
|---|---|---|---|---|---|
| **heater + battery** | ven-5, ven-14, ven-17 | **57.81 s** | **57.79 s** | **TimeLimit** | **Err** |
| heater only | ven-2, 3, 10, 12, 15, 18, 20 | 3.83 s | 57.75 s | GapLimit | TimeLimit |
| battery only | ven-1, 4, 6, 13, 16, 19 | 0.79 s | 34.39 s | GapLimit | GapLimit |
| neither | ven-7, ven-8, ven-11 | 0.04 s | 0.06 s | Optimal | Optimal |

Heater+battery is **15x worse than heater alone** and 73x battery alone, and nowhere near additive
(3.83 + 0.79 = 4.6 s against an actual 57.81 s). This is the first measurement in this project that
supports GB-40's binary-interaction theory, and it explains why ven-5 was GB-40's worst case at
120 s: it is one of only three VENs carrying both assets.

### Production confirmation

The bench predicted phase 2 *fails* on that class. `grep 'Phase 2 failed'` over 3 h on Node2
(~36 replan cycles):

| VEN | class | failures in 3 h |
|---|---|---|
| ven-5 | heater + battery | **29** |
| ven-14 | heater + battery | **33** |
| ven-17 | heater + battery | **32** |
| ven-11 | neither | 0 |

So those three fail phase 2 on essentially every cycle, fall back to phase 1, and discard the whole
phase-2 budget. The logged cause is `NoSolutionFound` with **`epsilon: 0.02`** — the default. The cap
`cost <= c_star + 0.02` is tight enough that HiGHS finds *no* solution at all on a heater+battery
instance, despite being handed a feasible warm start (phase 1's own schedule satisfies the cap by
construction).

### CORRECTED after the warm-start fix (2026-10-01)

Everything in the table above, and the per-class advice that used to follow it, was measured
**through an infeasible phase-2 warm start** (the `u_bat` direction-selector bug, fixed in
`b8430232`). Re-measured on correct code, `bench_min_epsilon_by_class`, 30 s phase-2 budget:

| epsilon | heater only: slots moved / friction | heater + battery: slots moved / friction |
|---|---|---|
| 0.02 | 0 / 4.6201 | 0 / 6.5530 |
| 0.10 | 0 / 4.6201 | 0 / 6.5259 |
| 0.20 | 0 / 4.6201 | 0 / 6.4971 |
| 0.30 | 0 / 4.6201 | 0 / 6.4691 |
| 0.50 | 0 / 4.6201 | 0 / 6.4176 |
| 0.75 | 0 / 4.6201 | **90 / 2.9686** (switches 74 -> 29) |
| 1.00 | **76 / 3.4555** (switches 58 -> 32) | 0 / 6.3237 |
| 2.00 | 0 / 4.6201 | 0 / 6.1964 |

Three things change:

1. **Phase 2 now succeeds on heater+battery at every epsilon** — it previously returned `Err`
   (`NoSolutionFound`) at all eight. The earlier recommendation to set `phase2_epsilon_eur = 0.0`
   on ven-5/ven-14/ven-17 is **retracted**: it was predicated on phase 2 being unfixably broken
   there, and disabling it would now discard real smoothing.
2. **"Heater slots moved" was too narrow a metric.** On heater+battery, friction improves
   monotonically with epsilon (6.5530 -> 6.1964) in rows where *zero* heater slots moved, so phase
   2 is smoothing the **battery and grid** schedules there. Earlier conclusions drawn from heater
   slot counts alone missed that entirely.
3. **The erratic behaviour is real, not a consequence of the bug.** Exactly one epsilon in eight
   finds the large heater improvement, and it is a *different* value per class (1.00 for heater
   only, 0.75 for heater+battery). Raising epsilon strictly enlarges the feasible set, so a value
   that works at 0.75 cannot legitimately fail at 1.00 — phase 2 is still landing on suboptimal
   incumbents, and the choice is incumbent luck rather than a threshold.

**Consequence for tuning:** there is no reliable per-VEN epsilon to recommend from a single sweep.
The monotone part (battery/grid friction falling as epsilon rises) is the dependable gain; the
large heater-schedule improvement is a lottery. Any epsilon recommendation needs repeated runs per
candidate value, which is the same discipline this file already requires for budget measurements.

### Superseded: what follows, per VEN class

`solver_phase2.rs` skips phase 2 outright when `phase2_epsilon_eur == 0.0`, so the settings below
are the whole mechanism — no code change needed.

| VENs | today | consequence | proposed |
|---|---|---|---|
| ven-5, ven-14, ven-17 | epsilon 0.02 (default) | phase 2 **fails every cycle**, 15 s discarded | **0.0** — disable. Reclaims ~15 s/cycle with **zero** behavioural change, since the plan already comes from phase 1 |
| ven-10, 12, 15, 18, 20 | epsilon 0.02 (default) | phase 2 runs, finds nothing (below the starvation threshold), 15 s wasted | **~1.00** — buys the smoothing the 15 s is already being spent on (68 of 288 heater slots, switching 58 -> 32) |
| ven-3 | epsilon 0.17 | same starvation | **~1.00** |
| ven-2 | epsilon 1.00 | working as intended | unchanged |

Both remedies are profile-only. The first is strictly a saving; the second buys value for time
already being spent. Neither changes asset behaviour or the forecast horizon.


## Phase 2 is deterministic, not a lottery (2026-10-01)

`bench_epsilon_repeatability`: hold epsilon at the value that worked for each class, solve phase 1
once (it does not depend on epsilon) so every repeat starts from an identical warm start, and repeat
phase 2 five times.

| class | epsilon | runs improving | result each run |
|---|---|---|---|
| heater only | 1.00 | **5/5** | 76 slots moved, switches 58 -> 32, friction 3.4555 |
| heater + battery | 0.75 | **5/5** | 90 slots moved, switches 74 -> 29, friction 2.9686 |

Bit-for-bit identical every time. **Two earlier claims in this file are retracted:**

- *"Phase 2's improvement is a lottery over incumbents"* — no. A given configuration reproduces
  exactly.
- *"Phase 2 at a fixed budget is not reproducible"* (the friction 2.0805 vs 3.4555 pair) — that
  compared two **different bench functions** assumed to build identical instances. Under a
  controlled five-repeat test the result is deterministic, so those two instances must have
  differed in some way not accounted for. The non-reproducibility was in the comparison, not the
  solver.

What survives is more precise: phase 2 is **deterministic but chaotically sensitive to epsilon**.
0.75 improves and 1.00 does not, reproducibly, on heater+battery. Changing epsilon changes the LP
relaxation, hence the branching order, hence which incumbent the search lands on. That is still
proof of suboptimal incumbents — a strictly larger feasible set cannot yield a worse optimum — but
it is predictable for a fixed configuration rather than random.

**Practical consequence:** per-VEN epsilon tuning *is* viable, because a measured choice holds. What
cannot be done is reasoning about the value, or extrapolating one VEN's working value to another —
each needs measuring. The open question is whether a value that works on one instance keeps working
as live state changes every replan cycle; that is what `bench_epsilon_across_instances` tests.


## Does phase 2's smoothing reach the relay? Mostly no (2026-10-01)

`bench_epsilon_across_instances` first: no fixed epsilon works across instances.

| instance | e=0.3 | e=0.5 | e=0.75 | e=1.0 | e=1.5 | e=2.0 |
|---|---|---|---|---|---|---|
| cool tank, full power | =58 | =58 | =58 | **32** | =58 | =58 |
| near T_min, off | =58 | =58 | =58 | **26** | **34** | **32** |
| mid-band, mid stage | =54 | **26** | **24** | =54 | =54 | =54 |
| warm tank, off | =44 | =44 | =44 | =44 | =44 | =44 |

(Heater switches after phase 2 against after phase 1; `=N` means phase 2 changed nothing.) No column
improves every row — e=1.0 helps the first two instances, e=0.5/0.75 the third, and the fourth is
untouched at every value. Since live state changes every 300 s, **per-VEN epsilon tuning is not
viable in production**: the working value moves with the tank, even though each individual solve is
deterministic.

`bench_phase2_smoothing_reaches_relay`, six instances at the **production 15 s budget**:

| instance | 25 min | 1 h | 4 h | 8 h | 48 h |
|---|---|---|---|---|---|
| cool tank, full — phase 1 / 2 | 0 / **2** | 0 / **2** | 10 / 5 | 28 / 11 | 58 / 32 |
| near T_min, off — phase 1 / 2 | 2 / **1** | 4 / **2** | 14 / 7 | 32 / 8 | 58 / 26 |
| mid-band, mid — phase 1 / 2 | 0 / 0 | 0 / 0 | 6 / 2 | 24 / 4 | 54 / 24 |
| warm tank, off — phase 1 / 2 | 0 / 0 | 0 / 0 | 0 / 0 | 12 / 12 | 44 / 44 |
| cool + battery — phase 1 / 2 | 0 / 0 | 0 / 0 | 12 / 12 | 28 / 28 | 74 / 74 |
| mid-band + battery — phase 1 / 2 | 0 / 0 | 0 / 0 | 4 / 4 | 20 / 20 | 66 / 66 |

Two results:

1. **At the 15 s production budget, heater+battery gets no smoothing at all.** Those same instances
   improved at a 30 s budget (74 -> 29 at epsilon 0.75), so ven-5/ven-14/ven-17 still get nothing —
   now for a budget reason rather than the warm-start bug.
2. **The executed window is a wash.** With a 300 s replan interval, only the first slots ever run.
   Phase 2 changes the first 25 minutes in 2 of 6 instances — once **worse** (0 -> 2) and once better
   (2 -> 1) — and leaves four unchanged. All the substantial improvement (58 -> 32, 58 -> 26,
   54 -> 24) sits in hours 4-48, which the next cycle replaces.

**What this implies, as a recommendation rather than a change.** Phase 2 costs ~15 s per cycle on
every VEN. At the production budget it improves the horizon-wide schedule on about half of
instances, none of that reaching the hardware, and it occasionally adds a switch to the window that
does. `phase2_epsilon_eur = 0.0` would reclaim that time fleet-wide with no measured executed-
behaviour cost.

This is deliberately **not** applied. Disabling a whole phase changes what the plan *looks* like —
horizon-wide chatter becomes visible in the UI and in `plan_history` — and that is a change in the
character of VEN behaviour, which is the user's call and not an optimisation to make unattended.
The evidence is also from one bench profile: before acting it should be reproduced on the real
heater profiles and checked against anything downstream that consumes plan smoothness (the arbiter's
limit-enforcement pass, the gate switch penalty).


## A free 2.6-9.5x on the fleet's slowest VENs: gap 0.30 for heater+battery (2026-10-01)

After the warm-start fix, production timings put ven-5's phase 1 at 36-60 s, hitting the 60 s
ceiling on most cycles, while phase 2 is bounded at 15 s. Phase 1 on the heater+battery class is
therefore the dominant remaining cost in the fleet, and the bench agrees (57.8 s against 3.83 s for
heater only).

`bench_heater_battery_gap_sweep` — gap is the only variable:

| mip_gap | phase 1 | objective | status |
|---|---|---|---|
| 0.06 (current) | 57.99 s | -11.8370 | TimeLimit |
| 0.10 | 57.74 s | -12.6554 | TimeLimit |
| 0.15 | 58.81 s | -11.8347 | TimeLimit |
| 0.20 | 57.44 s | -11.8235 | TimeLimit |
| **0.30** | **12.54 s** | -11.0826 | **GapLimit** |

0.30 is the first gap that gets phase 1 off the ceiling. (0.10 landing a *better* objective than
0.06 is more incumbent luck — both TimeLimit, so both suboptimal.)

`bench_gap_executed_cost` then prices what actually runs, across four tank start conditions:

| instance | phase 1 @0.06 | phase 1 @0.30 | speedup | cost 25 min | cost 1 h | cost 8 h |
|---|---|---|---|---|---|---|
| cool, full | 57.40 s | **11.09 s** | 5.2x | identical | identical | identical |
| near T_min, off | 57.22 s | **6.05 s** | 9.5x | identical | identical | identical |
| mid-band, mid | 56.07 s | **21.81 s** | 2.6x | identical | identical | 0.7 % cheaper |
| warm, off | 27.96 s | **6.22 s** | 4.5x | identical | identical | 2.2 % cheaper |

**The executed window is unchanged in every instance** — first-25 min and first-1 h cost identical,
first-8 h identical or slightly cheaper. The ~6 % penalty on the 48 h objective is paid entirely in
slots the next replan (300 s) discards.

This is why the conclusion differs from GB-40's. That sweep went to 0.22 and rejected looser gaps on
**phase-1 objective** grounds, reporting +4-9 % mean cost at 16-22 %. It never priced the executed
window, and the executed window is what the meter sees. The horizon objective is the wrong yardstick
for a receding-horizon controller — the same mistake this file records for the 48 h horizon question
and for phase 2's smoothing.

**Applied as a canary on ven-5 only.** ven-14 and ven-17 are the same asset mix and stay at 0.06 as
the control, so the production difference is attributable. 0.30 is well beyond GB-40's measured
range, and four bench instances are not a fleet, so the canary earns the change rather than
assuming it.


## CORRECTED: it was the smallest tank, not the asset mix (2026-10-01)

The canary's own controls refute the conclusion that produced it. ven-5, ven-14 and ven-17 share the
heater+battery mix and the same host; only ven-5 carries gap 0.30. Production phase-1 medians:

| VEN | volume | band | slack kWh | battery | mip_gap | phase 1 median | status |
|---|---|---|---|---|---|---|---|
| **ven-5** | **150 L** | 20 K | **3.49** | 11 kWh | 0.30 | **18.8 s** | GapLimit |
| ven-17 | 250 L | 20 K | 5.81 | 12 kWh | 0.06 | **4.2 s** | GapLimit |
| ven-14 | 300 L | 20 K | 6.97 | 7 kWh | 0.06 | **4.2 s** | GapLimit |

The untouched controls at 0.06 are **4.5x faster than the canary at 0.30**. So "heater+battery is
catastrophically slow" is **wrong** — two VENs with that exact mix solve in ~4 s. ven-5 is slow
because it has the **smallest tank in the fleet**, half ven-14's.

This is the tank-slack result confirmed in production, and more cleanly than the bench managed:
three VENs, identical asset mix, same host, same 20 K band, differing only in volume — and phase-1
time tracks slack (3.49 kWh -> 18.8 s; 5.81 and 6.97 kWh -> 4.2 s).

**Why the bench misled.** It used 200 L across a 15 K band = 3.49 kWh of slack, which is *exactly*
ven-5's 150 L across 20 K. Every "heater+battery" bench row was therefore modelling ven-5
specifically, not the class, and the 57.8 s it reported is ven-5's number rather than the mix's.

**What the bench measured that production does not contradict:** at *constant* slack (3.49 kWh),
adding a battery took bench phase 1 from 3.83 s (heater only) to 57.8 s. Production cannot separate
that cleanly — ven-3 is heater-only with the same 3.49 kWh slack and also runs 11-46 s — so the
battery's marginal contribution on top of low slack is **not established**. Slack is the driver that
is established, on both bench and fleet.

**Canary status: justified, but narrower than claimed.** ven-5 went from 36-60 s TimeLimit to
6.8-38.7 s GapLimit (median 18.8 s), and the gap change caused it — the warm-start fix had already
been deployed and ven-5 was still at 36-60 s TimeLimit after it. Total solve 75 s -> 23 s. But the
reason is ven-5's tank, not its asset mix, so it should **not** be extended to ven-14 and ven-17,
which are already at 4 s and would pay the ~6 % horizon-objective cost for nothing.

**The better lever for ven-5 is physical.** The tank-slack sweep showed widening a band from 15 K to
40 K roughly halving phase 1 and cutting switching 53 -> 14. ven-5 runs 45-65 C on 150 L; if that
installation can take a wider band or a larger tank, it addresses the cause rather than loosening
the optimality tolerance. That is a hardware question, recorded here rather than acted on.

## Fleet-wide phase survey: it is the count of co-scheduled storage assets (2026-10-01)

Measured in production on all 20 VENs' own containers, 4 consecutive periodic solves each,
commit `a51f7a13`. Rebuilding the asset map **from the profiles themselves** rather than from
earlier notes was the point of the exercise, and it found the notes wrong: ven-19 was recorded
as battery-only but carries battery + EV + PV, ven-1/13/16 as battery-only but all carry an EV,
and ven-7 as assetless but carries an EV. Several earlier class comparisons rested on that map.

| VEN | assets | phase 1 median | phase 2 median | phase 2 status |
|---|---|---|---|---|
| ven-11 | EV | **0.07 s** | 0.23 s | GapLimit |
| ven-9 | EV + `penalty_rules` | **0.18 s** | 0.80 s | GapLimit |
| ven-4 | battery, PV | **0.58 s** | 4.6 s | GapLimit |
| ven-6 | battery, PV | **0.57 s** | 6.8 s | GapLimit / Optimal |
| ven-1 | battery, EV, PV | **0.78 s** | **15.6 s** | **TimeLimit** |
| ven-16 | battery, EV | **1.3 s** | **15.2 s** | **TimeLimit** |
| ven-13 | battery, EV | **2.4 s** | **15.1 s** | **TimeLimit** |
| ven-19 | battery, EV, PV | **12.6 s** | **15.2 s** | **TimeLimit** |
| ven-17 | battery, heater, PV | 4.2 s | — | — |
| ven-14 | battery, heater | 4.2 s | — | — |
| ven-5 | battery, EV, heater, PV | 18.8 s | — | — |

**Phase 1 scales with how many storage assets must be co-scheduled**, not with which ones:

| storage assets | phase 1 |
|---|---|
| EV alone | 0.07 s |
| battery alone | ~0.6 s |
| battery + EV | 0.8-12.6 s |
| battery + heater | 4.2 s |
| battery + EV + heater | 18.8 s |

Each storage asset adds a corridor the solver must thread, and they interact only through the
shared import/export balance — so the difficulty compounds rather than adds. This **subsumes**
the tank-slack result rather than replacing it: slack is a real driver *within* the heater
class (monotone across 3 bench repeats, and across ven-5/14/17 in production), but across
classes the asset count dominates. ven-5 being the fleet's **only** three-storage-asset VEN is
the structural half of why it is slowest; its 150 L tank is the rest.

It also settles the question the previous section left open. "The battery's marginal
contribution on top of low slack is not established" — it now is, from the other direction:
ven-19 (battery + EV, **no heater**) runs 12.6 s, three times ven-14/17's heater+battery 4.2 s.
Heaters are not what makes this model hard.

### Phase 2 fails to converge on exactly one class: battery + EV

All four battery+EV VENs hit the 15 s `phase2_solver_timeout_s` on every sampled solve. Every
other class converges — EV-only in under a second, battery-only in 4-7 s. The pairing is what
phase 2 cannot close, and it is insensitive to phase 1's own difficulty: ven-1 solves phase 1 in
0.78 s and still burns the full phase-2 budget.

That makes this the fleet's **dominant** solver cost: 4 VENs x 15 s every 5 minutes, spent
reaching a time limit rather than an answer. Read together with "Does phase 2's smoothing reach
the relay? Mostly no" above — which found the first-25-minutes dispatch changed in 2 of 6
instances, all real gains landing in hours 4-48 — this is 60 s/cycle of fleet compute for an
effect that mostly does not reach hardware. The executed-window check has **not** been run on
the battery+EV class specifically, so this is the next measurement, not yet a conclusion.

### `penalty_rules` are not a solve-time risk (closes the ven-9 open item)

ven-9 is the only VEN carrying `penalty_rules`, and ven-11 is its exact control: both are
EV + base_load on the same host and grid, differing only in the rule. The 48 h horizon at
1800 s windows gives 96 windows, so the rule adds **96 continuous slack variables and 288
constraints — and no binaries** (`penalty.rs::declare_penalty_vars` adds `variable().min(0.0)`).

Measured cost: phase 1 0.07 -> 0.18 s, phase 2 0.23 -> 0.80 s, roughly 2.5-3x both phases on a
base so small the total stays near 1 s. The factor is real and structurally explained — the
window constraints couple every import slot within each 30 min bucket, which is LP work repeated
at every branch-and-bound node — but there is no combinatorial blow-up, because nothing branches.

**Separate observation, not a solver issue.** ven-9's threshold is 0.3 kW while its base load is
a flat 0.5 kW, and it has neither PV nor battery, so `p_imp >= 0.5` in every slot and the slack
is pinned at `>= 0.2 kW` in all 96 windows: ~19.2 EUR of the reported ~50 EUR objective is a
constant the plan can never avoid. The rule still steers the part that matters (11 kW of EV
charging on top of it), but its floor means `objective_eur` for ven-9 is not comparable with any
other VEN's, and phase 2's epsilon — an **absolute** 0.02 EUR cap — is a 0.04 % tolerance against
a `c_star` that size. Worth knowing before reading ven-9 cost numbers or tuning its epsilon.

### Refuted here

- **Battery corridor width (capacity/power) does not predict difficulty.** The fleet is
  near-uniformly 2.0 h of storage (ven-1 10/5, ven-6 9/4.5, ven-13 10/5, ven-16 8/4, ven-5
  11/5.5, ven-14 7/3.5, ven-17 12/6); the only exceptions are ven-4 at 2.7 h and ven-19 at
  2.29 h. ven-4 is the fastest battery VEN and ven-19 the slowest, so the ratio orders them
  backwards. The thermal-slack analogy does **not** carry over to the electrochemical corridor.
- **PV is not the discriminator.** ven-1 carries battery + EV + PV and solves phase 1 in 0.78 s,
  against ven-19's 12.6 s with the same three. Export/curtailment decisions are not the cost.

### Open

**ven-19 is a 10x outlier inside its own class** (12.6 s vs 0.78-2.4 s for ven-1/13/16) and
nothing structural explains it yet. Its distinguishing features are the fleet's largest battery
(16 kWh / 7 kW) and the only `round_trip_efficiency` of 0.93 rather than 0.92. The efficiency is
the more suspicious of the two: round-trip loss sets the price spread at which arbitrage breaks
even, and an efficiency that puts break-even near the actual tariff spread leaves many
near-optimal schedules and a weak LP bound — which would also explain the chaotic epsilon
sensitivity recorded above. Untested; a bench sweeping only `round_trip_efficiency` on one
battery+EV instance would settle it.

## RETRACTED: production wall-clock cannot compare VENs — 17 solvers, 4 cores (2026-10-01)

**The quantitative half of the fleet survey above is invalid.** Re-measuring the same VENs
40 minutes later, at the same commit, with no code or profile change:

| VEN | first sample | second sample |
|---|---|---|
| ven-14 (battery, heater) | 4.2 s | **21-28 s** |
| ven-5 (battery, EV, heater) | 18.8 s | **36-49 s** |
| ven-15 (heater, PV) | — | 8.8 s **and 60.1 s (TimeLimit)** |

A 5-6x swing on one unchanged VEN is larger than every between-VEN difference the survey table
rests on, so that table measures host load, not model difficulty.

**Why.** Node2 has **4 cores and runs 17 VENs**, each solving every 300 s. Solver wall-time
summed across the fleet is 1,222 s per 30 min — only ~17 % of 4 x 1800 core-seconds, so the host
is *not* saturated on average. The problem is **alignment, not volume**: the solves arrive in
bursts. Six completed inside a 90 s window at 20:26 UTC, including the two heaviest (ven-5,
ven-17), against 4 cores.

**And the bursts are partly self-inflicted.** Recreating eight containers in a tight loop (the
container-name cleanup, 20:14 UTC) restarted eight VENs within seconds of each other, so their
fixed 300 s cadences are now in phase and will stay in phase. Every subsequent cycle collides.
The slow second-sample numbers above are all from that restarted set.

**A real deployment issue, found by accident.** The replan cadence has no stagger and no jitter,
and the interval is measured from solve *completion*, so a VEN whose solve takes 34 s runs on a
~334 s period. ven-17's completions: 20:09:34, 20:15:03, 20:21:19, 20:26:58 — intervals of 329,
376 and 339 s against a nominal 300. Phases therefore drift at different rates per VEN, wander
into each other, and stick together for a while once aligned. On a 4-core host with 17 VENs that
converts directly into wall-clock solve time, TimeLimit terminations, and the appearance of
model difficulty. Filed as a backlog item; the fix is per-VEN replan jitter.

### What survives from the section above

- **The asset-map correction.** ven-1/7/13/16/19 were misclassified in earlier notes; read from
  the profiles, not from timings. Several older class comparisons rest on the wrong map.
- **`penalty_rules` add no binaries** — `declare_penalty_vars` adds `variable().min(0.0)` only,
  96 continuous slacks and 288 constraints at 48 h / 1800 s windows. Structural, from the code.
  The *measured* 2.5-3x is as confounded as everything else here and is withdrawn.
- **ven-9's penalty floor.** 0.3 kW threshold under a flat 0.5 kW base load with no PV or
  battery pins `>= 0.2 kW` of slack in all 96 windows: ~19.2 EUR of its ~50 EUR objective is
  unavoidable. Arithmetic, not measurement.
- **Battery capacity/power ratio cannot drive the within-class spread**, and this one holds
  whatever the cause of the spread: ven-1, ven-13 and ven-16 have *identical* 2.0 h ratios
  (10/5, 10/5, 8/4) and do not solve alike, so a quantity that is the same cannot explain a
  difference that is real. The thermal-slack analogy does not carry over.

### Withdrawn, pending bench confirmation

- "Phase 1 scales with the count of co-scheduled storage assets." Suggestive, and it would
  subsume the tank-slack result neatly, but every number supporting it is a contended wall-clock
  reading. Needs one-at-a-time bench instances per mix.
- "Phase 2 fails to converge on exactly one class, battery+EV." The second sample contradicts
  it outright — ven-5, ven-14 and ven-15 are heater VENs and all hit the phase-2 TimeLimit.
- ven-19 as a "10x outlier". Possibly just where its phase landed.

### Method rule for everything after this

**Compare models on the bench, never in production.** The bench runs one solve at a time on a
quiet machine, which is why `bench_phase1_vs_tank_slack` could show a monotone relationship
across three repeats while production could not. Production logs remain good for what they
uniquely show — did the solve succeed, what status did it reach, is the fleet keeping up — and
for *within*-VEN before/after on the same host at the same hour, which is what the gap-0.30
canary measured. They cannot rank two different VENs.

## Battery+EV on the bench: phase 1 is trivial, phase 2 is genuinely expensive (2026-10-01)

`bench_battery_ev_phase2_executed_window`, one solve at a time on a quiet machine — the
instrument the retraction above says to use. ven-19's battery (16 kWh / 7 kW) plus an 11 kW EV,
no heater, 288 slots, gap 0.06, epsilon 0.02 (the fleet default). Phase 2 budget 15 s.

| round-trip eff | phase 1 | phase 2 | phase 2 status | EV 25 min (p1 → p2) | battery net 25 min (p1 → p2) | battery net 48 h (p1 → p2) |
|---|---|---|---|---|---|---|
| 0.92 (fleet) | **0.28 s** | **13.4 s** | TimeLimit | 5.500 → 5.500 | −2.369 → −2.172 | 1.486 → 1.927 |
| 0.93 (ven-19) | **0.20 s** | **11.5 s** | GapLimit | 5.500 → 5.500 | −2.519 → −2.317 | 1.336 → 1.781 |
| 0.96 | **0.19 s** | **10.7 s** | GapLimit | 5.500 → 5.500 | −2.492 → −2.436 | 1.328 → 1.326 |

**Phase 1 on this class is 0.19-0.28 s, not 0.8-12.6 s.** The production readings were inflated
3-45x by contention. This is the retraction's own claim measured directly, and it is worse than
the retraction assumed: for battery+EV, production phase-1 time is almost entirely queueing.

**ven-19's efficiency hypothesis is refuted.** Phase 1 does not climb with `round_trip_efficiency`
— it falls slightly (0.28 → 0.20 → 0.19 s) — so the arbitrage-break-even/weak-bound story is
wrong for phase 1. With capacity/power already refuted, **ven-19 has no model-side explanation
left**, which is consistent with its 12.6 s having been contention and nothing else.

**But the phase-2 claim survives, and this is the useful result.** 10.7-13.4 s on a machine with
nothing competing is not queueing — phase 2 on battery+EV really does consume most of its 15 s
budget, and at the fleet's actual 0.92 it hits the TimeLimit outright. Efficiency does matter
here, just in the other phase and mildly: 0.92 is the hardest of the three (13.4 s, TimeLimit)
and higher efficiency is easier (10.7 s, GapLimit). A ~25 % effect, not a 10x one.

**What the 11-13 s buys at the relay.** EV dispatch in the first 25 minutes is **identical** in
both phases at every efficiency (5.500 kWh, to four digits). Battery net energy moves by
0.06-0.20 kWh against ~2.4 kWh dispatched — a 2-8 % change in the executed window. Real, but
small, and nothing at all on the EV. The larger divergence is again out in the horizon
(1.486 → 1.927 kWh over 48 h at 0.92), matching the heater-class finding that phase 2's work
lands in hours 4-48.

`friction_eur` is **negative** (−0.345) on all six solves, reconfirming that phase 2's friction
is not a switching metric — it carries the PV-use tiebreak reward (see "What is actually inside
phase 2's friction?" above) and can go below zero.

### Where this leaves the fleet's solver cost

Phase 2 is now the only measured, uncontended cost worth attacking: ~11-13 s per cycle on the
four battery+EV VENs, for a 2-8 % change in executed battery energy and none in EV dispatch.
Phase 1 on that class is free. The outstanding decision is therefore the one already recorded
and deliberately not taken — whether to cut `phase2_solver_timeout_s` (or epsilon) for
battery+EV — and it now has a measured price tag on both sides rather than only on the cost side.
Note this is a *different* argument from the retracted "phase 2 fails only on battery+EV": the
heater VENs hit the phase-2 TimeLimit too, and whether that is contention or real is still
unmeasured on the bench for that class.

## Post-GB-54: phase 2's problem is the heater class, not battery+EV (2026-10-02)

Re-measured 7 h after GB-54 reached production, 6 consecutive solves per VEN. This is the
measurement the retraction above said had to come before any phase-2 budget decision, and it
inverts the answer.

| VEN | assets | phase 1 median | phase 1 worst | phase 2 TimeLimit |
|---|---|---|---|---|
| ven-13 | battery, EV | 0.57 s | 0.9 s | 2 / 6 |
| ven-16 | battery, EV | 0.51 s | 1.0 s | 1 / 6 |
| ven-19 | battery, EV, PV | 1.6 s | 2.4 s | **0 / 6** |
| ven-4 | battery, PV | 1.4 s | 2.6 s | 0 / 6 |
| ven-6 | battery, PV | 0.55 s | 1.7 s | 0 / 6 |
| **ven-20** | **heater (450 L)** | **0.14 s** | 0.18 s | **0 / 6** |
| ven-12 | EV, heater | 1.8 s | 2.6 s | 4 / 6 |
| ven-10 | heater (space, 18-23 C) | 1.8 s | 2.4 s | 6 / 6 |
| ven-15 | heater, PV (200 L) | 2.4 s | 3.2 s | 6 / 6 |
| ven-14 | battery, heater (300 L) | 4.2 s | 10.0 s | 6 / 6 |
| ven-17 | battery, heater, PV (250 L) | 6.0 s | **60.1 s** | 6 / 6 |
| ven-5 | battery, EV, heater (150 L) | 5.0 s | **60.1 s** | 6 / 6 |

**GB-54 did most of the work on battery+EV.** Before it, all four battery+EV VENs hit the phase-2
TimeLimit on *every* sample; now it is 3 of 18. Phase 1 fell with it — ven-19 from a 12.6 s median
to 1.6 s (8x), ven-13 2.4 s to 0.57 s, ven-16 1.3 s to 0.51 s. The remaining gap to the bench's
0.2 s is ordinary residual load, not a different regime.

**So options B and C were indeed fixes for a problem that was about to disappear.** Cutting
`phase2_solver_timeout_s` or zeroing epsilon on battery+EV is no longer warranted: phase 2
converges there. Holding that decision for one measurement was the right call, and it is the
second time in this investigation that acting on a contended reading would have produced a
permanent change to fix a transient cause.

**The phase-2 problem is now entirely the heater class**: 34 of 42 samples at TimeLimit across
ven-5/10/12/14/15/17, against 3 of 18 everywhere else. This is the exact inverse of the claim
retracted above ("phase 2 fails to converge on exactly one class, battery+EV"), which was built on
contended data. The heater class is also where phase 1 still occasionally exhausts its own 60 s
budget — ven-17 twice and ven-5 once in these 12 samples.

### ven-20 is the control the fleet has been missing

ven-20 is a heater VEN that solves phase 1 in **0.14 s** and never hits the phase-2 TimeLimit,
while every other heater VEN does both. Its tank:

| VEN | volume | band | thermal slack |
|---|---|---|---|
| **ven-20** | **450 L** | **40-75 C (35 K)** | **18.31 kWh** |
| ven-14 | 300 L | 45-65 C (20 K) | 6.98 kWh |
| ven-17 | 250 L | 45-65 C (20 K) | 5.81 kWh |
| ven-15 | 200 L | 42-62 C (20 K) | 4.65 kWh |
| ven-5 | 150 L | 45-65 C (20 K) | 3.49 kWh |

ven-20 carries **2.6x the slack of the next-largest tank** and is 20-50x faster. That is the
tank-slack result reproduced on uncontended production data, which is what the retraction asked
for — the bench had shown it monotone across three repeats, and the earlier production attempt
could not be trusted.

**It does not isolate slack from asset count**, and should not be read as doing so: ven-20 is also
the only heater VEN with no battery, PV or EV beside it. Slack and asset count both point the same
way here, and this measurement cannot separate them. What it does establish is that a heater VEN
*can* be trivially fast, so "heaters are inherently hard" is not the explanation.

ven-10 is excluded from that table deliberately — it is a **space** heater (18-23 C, no
`volume_l`), a different asset shape whose thermal mass is not a tank volume, so its 5 K band is
not comparable to a cylinder's.

### What this makes the next question

Not "should phase 2's budget be cut" — it should not — but **why the heater model cannot close its
phase-2 gap in 15 s when every other class now can**, and whether ven-5 and ven-17 exhausting a
60 s *phase 1* budget is the same cause. The lever recorded earlier for ven-5 (a larger tank or a
wider band, a hardware change rather than a tolerance change) now has ven-20 as direct evidence
that it would work.

## ven-17's slow solves are not contention — the heater model is genuinely stuck (2026-10-02)

A prediction made when GB-54 was verified: ven-9, ven-17 and ven-3 hash to offsets 144/149/154 s,
so they would still collide, and ven-17 — one of the two heaviest VENs — would stay slow for that
reason. **Wrong on both counts.**

Parsed 2788 solves across 5 h and 17 VENs on Node2, then asked how many other VENs were solving
**at the instant each solve started**:

| VEN | solves | busy at start, slow half | busy at start, fast half |
|---|---|---|---|
| **ven-17** | 60 | **0.00** | **0.00** |
| **ven-5** | 60 | **0.00** | **0.00** |
| **ven-14** | 60 | **0.00** | **0.00** |
| ven-15 | 60 | 0.43 | 0.03 |
| ven-13 | 60 | 1.10 | 0.87 |
| ven-19 | 60 | 1.00 | 0.70 |
| ven-20 | 61 | 1.37 | 1.10 |

ven-17 began **every one of its 60 solves with the host otherwise idle**, and still ran a 24.5 s
median and a 77 s maximum. GB-54 is giving it a clean start every cycle; the time is the model.
ven-3 was never relevant — it runs on Node1, a different host, so its offset cannot contend at all.

**A near-miss worth recording, because the first cut of this analysis said the opposite.**
Counting *overlap across each solve's duration* showed ven-17's slow solves averaging 1.52
concurrent VENs against 0.00 for its fast ones — an apparently crisp confirmation. It is an
artefact: a 77 s solve spans 77 s in which to overlap somebody, a 3 s solve spans 3 s, so overlap
count is mechanically coupled to the duration it is supposed to explain. The slowest solves all
overlapped ven-10, ven-4, ven-19 and ven-13 — whose offsets (172, 187, 195, 205 s) all fall
*after* ven-17's 149 s, i.e. they started during ven-17's long solve rather than causing it.
Measuring concurrency at the start instant removes the coupling and reverses the conclusion.

### What this changes

**Phase-1 TimeLimit on ven-5 and ven-17 is a quality risk, not just a CPU cost, and is worth
attention.** Unlike a phase-2 TimeLimit — which returns a warm-started incumbent the epsilon cap
forbids from being worse than phase 1 — a phase-1 TimeLimit returns an incumbent at an unknown
gap, so the plan may be materially suboptimal and nothing reports by how much (R-65: the achieved
gap is not observable through `good_lp`).

**And the gap knob does not fix it.** ven-5 already runs `mip_gap_target` 0.30, five times the
fleet's 0.06, and still reaches 60 s on some instances. So this is the heater formulation itself,
which is GB-40's original territory and already carries a remediation plan there — dwell-time
constraints and a continuous power variable bounded by the tier binaries — not a tolerance to be
loosened further.

**Phase-2 TimeLimit on the heater class remains not worth chasing**, for the reasons recorded
above: bounded by the epsilon cap, ~0.3 cores on a host at ~17 %, and buying an effect that
reaches the executed window in 2 of 6 instances, once better and once worse.

ven-20 remains the evidence that the physical lever works: 450 L across a 35 K band, 18.31 kWh of
slack, 0.14 s phase 1, and it never hits either limit.

## Phase-2 startup penalty: a per-asset-mix setting, not a fleet default (2026-10-03)

**Trigger.** ven-1's live plan (2026-10-03T10:40Z, battery + EV + PV) had one-slot battery
dropouts at 14:10 and a 0.28 / 2.74 kW make-up blip at 16:45-16:55 local. At 14:10 the EV took
its 1.4 kW minimum while 3.4 kW was exported, which is strictly worse than charging the
battery. It sat inside phase 1's 2 % gap, and phase 2 had stopped at its 15 s TimeLimit
before it could merge the runs. In production, ven-1's phase 2 hit TimeLimit in 48 of 104
cycles that morning.

**`bench_ven1_spikes`** (class 1, `tests/phase2_spikes.rs`) replays that plan's per-slot PV,
base load and tariffs. First 8 h, battery starts / summed |Δnet|:

| Variant | Phase 2 | Battery starts | Ramp |
|---|---|---|---|
| production: startup 0.01, 15 s | TimeLimit 15.0 s | 2 | 15.4 kW |
| budget 60 s / 180 s | Optimal 30.6 / 35.9 s | 1 | 7.0 kW |
| **startup ×10 (0.10), 15 s** | TimeLimit 14.8 s | **1** | **7.3 kW** |
| ramp ×10, 15 s | TimeLimit | 1 | 7.3 kW, but the EV run is cut short |
| phase-2 gap 0.20 | TimeLimit | 2 | 15.4 kW |
| phase-1 gap 0.005 / 0.001 | (phase 1 only) | 2 | 26.6 kW, identical plan |

Tightening phase 1 does not help: its plan stays the same at every gap. The leftovers are
phase 2's to remove, and a 0.10 startup weight gets phase 2 there inside the existing budget.
Phase 2's cost cap is unchanged, so the plan cannot get dearer.

**Live after setting 0.10 on ven-1** (restart 11:28Z): phase 2 finished within budget on every
cycle (Optimal/GapLimit, 6-13 s), and every sampled plan had one battery run and zero one-slot
blips. This is confounded with today's instance getting easier: phase 1 also fell to < 1 s.

**`bench_fleet_startup_penalty`** (class 2, every real `ven-N.yaml`, ven-1's tariffs, each
profile's own PV/state, 15 s budget) compared 0.01 with 0.10:

- **No change in 17 of 20 profiles.** The first-8 h shape and the first-25-min dispatch are
  identical. That includes ven-1 and ven-19 on this synthetic instance, so the ven-1 gain is
  instance-dependent. It appears when battery and EV share a PV surplus near a run boundary.
- **ven-12 (EV + heater) got worse.** Phase 2 went from GapLimit 9.5 s to TimeLimit 15.1 s,
  and the first 25 min of EV energy moved from 0.831 to 0.766 kWh.
- Battery-only (ven-4/6) and battery + EV without PV (ven-16) already merge their runs at
  0.01 (ven-4: 5 → 1, ven-16: 5 → 1).
- On every heater VEN, phase 2 time is spent on the relay, which these weights do not reach.

**Decision:** 0.10 for battery + EV + PV without a heater (ven-1, ven-19), default elsewhere.
The guidance table lives in `VEN/profiles/README.md` "Planner smoothing by asset mix".

**Side observation, not acted on:** `friction_eur` reads 1 049 / 9 054 / 2 067 on
ven-7/18/19 here. The EV's R-93 integrity slacks (`DROP_UNMET_PENALTY_EUR`,
`SHORTFALL_PENALTY_EUR`) sit in the EV objective in both phases. They are therefore part of
phase 2's "friction" as well as the cap. That does not change decisions, since the cap holds
them too, but it makes `friction_eur` useless as a smoothing metric whenever a trip forces a
shortfall.

---

## Appendix A — investigation log moved from `TECHNICAL_DEBTS.md` R-97 (2026-09-27 to 2026-10-06)

Moved here unchanged on 2026-10-09 so the register keeps conclusions only. It is a log: later
entries correct earlier ones (several are marked CORRECTED or RETRACTED in their own text), and the
sections above supersede it where they disagree. Two statements in it are out of date: the phase-2
budget default is 15 s (it says 5 s in one place), and phase 2 is effective (an early entry asks
why it is inert).

### R-97 — MILP solve time sits close to its own timeout

**Live evidence, 2026-10-06: heater + battery sites pulse, and phase 2 cannot merge it cheaply.**
ven-5 and ven-17 run the heater in single 5-minute slots through the PV hours, and the battery
mirrors each pulse (it charges from the surplus except under the pulse). `bench_ven5_heater_fragmentation`
replays ven-5's plan of 07:25Z exactly (`ven5_heater_frag_data.rs`). Unlike ven-1's EV, which was an
equal-cost tie fixed by a tie-breaker (b632c6dc), this one isn't a tie: a heater tie-breaker of
1e-4 or 1e-3, and a phase-1 gap of 0.02 instead of 0.30, left it at about 30 heater runs, 26 of them
single slots. Phase 2 returns the same plan at 15 s and 60 s for cost allowances of 0.02 to 0.5 EUR.
Only 2 EUR with 60 s halves it (13 runs), at +0.7 EUR of grid cost over 48 h. So the remaining
question is a policy one (what a heater switch is worth in EUR) plus phase 2's search, not a
missing tie-break.

**A claimed regression from `ev-comfort-piecewise-core`, retracted the same day (2026-09-29).**
Kept because the method is the lesson, not the conclusion.

Post-deploy sampling showed ven-11 (EV + base load, no heater) at a 6.4 s median against 114 ms
the day before, and ven-2 at 45-54 % TIME_LIMIT against 5.6 %. That was reported as a 56x
regression caused by the change. **It was not established, and the attribution was wrong.**

What refuted it:

- **The step precedes the deploy.** Plotting consecutive solves instead of hourly medians, ven-2
  jumps 15.4 s -> 66.0 s between 08:13:28Z and 08:18:44Z, and ven-11 402 ms -> 21984 ms between
  08:09:54Z and 08:14:54Z. Both hosts stepped within five minutes of each other at ~08:15Z. The
  feature reached Node1 at 09:17Z and Node2 at ~09:20Z. A cause cannot follow its effect.
- **It does not reproduce offline.** Seven band shapes on the production 288-slot grid solve in
  0.04-0.09 s (`bench_ev_band_solve_cost`), including the fleet's actual shape — no bands at all,
  one synthetic zero-reward guarantee band, which is the *fastest* of the seven. Through the full
  two-phase planner with `usage_forecast` and charge planning, 0.08-0.22 s
  (`bench_ev_session_solve_cost`). Nothing in the band model costs seconds.
- **Other VENs moved the other way in the same window.** ven-7 went 28.3 s -> 5.0 s across 08:15Z
  while ven-19 and ven-15 stayed flat — all pre-deploy. That is host and fleet conditions, not a
  code change.

**The methodological failures, which are why this entry is kept.**

1. *A one-sample baseline.* The change's own baseline table held one `solver_ms` per VEN. ven-2's
   entry was 11.9 s; its real distribution over the preceding 1910 solves was median 15.7 s, p90
   31.5 s, tail to the ceiling. A single draw cannot distinguish a 4x regression from an ordinary
   sample — in either direction.
2. *Time-of-day matching is not load control.* Comparing the same clock window on consecutive days
   looks controlled and is not, when the second day is full of E2E runs, image builds and fleet
   redeploys on those same hosts — activity caused by the very work being measured.
3. *Hourly medians hid the boundary.* They placed the jump "somewhere in the 08-09Z hour", which
   was compatible with the deploy. Consecutive-solve sequences placed it at 08:15Z, which is not.
4. *A confident fix for an unverified cause.* Band merging was shipped as the remedy and changed
   nothing (6406 -> 6752 ms). That should have been read immediately as the diagnosis being wrong
   rather than the fix being insufficient. The merge is kept on its own merits — fewer variables,
   provably identical valuation, pinned by `bands_at_the_same_bid_are_one_band`.

**What did cause the step: an EV usage-forecast transition.** Both VENs stepped as their EV
changed availability state, not as code was deployed.

- **ven-11** (assets: `base_load` + `ev` only — nothing else can explain it): `GET /ev-usage-sim`
  gives `leave_at 2026-09-29T08:09:39Z` with a **65.1 % expected SoC drop**. The step is the very
  next replan, 08:14:54Z.
- **ven-2** (evening departure / morning return pattern, `return_at ~07:59`): its EV **returned**
  at ~07:59Z with a 17.4 % drop; the step follows three cycles later at 08:18:44Z.

The mechanism is the same in both directions: once a car is away and returning with a real SoC
drop, the planner must place a substantial charge *after a predicted return* instead of charging a
car that is present — a materially harder problem, and one whose difficulty varies day to day with
the randomised drop (`soc_drop_pct_mean`/`stddev`). That is why the effect appears on one morning
and not the previous one.

This capability is `ev-usage-forecast`'s, made reachable for an away-now car by `96f259e9`
(deployed 2026-09-27) — which is also when ven-11's daily median first stepped, 136 ms on the 26th
to 563 ms on the 27th, two days before `ev-comfort-piecewise-core` existed on any host.

**Confidence:** strong for ven-11 (one cycle, single-asset site, exact time match), good for ven-2
(same mechanism class, three cycles). Neither is proof of a *quantitative* cost model for
away-window planning, which is what the next measurement should establish.

**The EV half, measured properly (2026-09-29).** `bench_ev_session_solve_cost` now repeats each
variant five times and reports min/median/max, because single samples on a laptop swing 3-6x —
enough to invent or erase the whole effect. With that, the away-window step is solid and repeatable
on an EV-only site at 288 slots through the real two-phase planner:

| variant | min | median |
|---|---|---|
| no EV session | 0.16 s | 0.17 s |
| firm EV session | 0.26 s | 0.31 s |
| forecast, car **home** | 0.19 s | 0.21 s |
| forecast, car **away** (20/65/90 % drop) | 0.90-0.94 s | 0.95-1.09 s |
| forecast, just returned | 1.02 s | 1.36 s |

So ~5x for a car that is out, and **the SoC drop size is irrelevant** (20 %, 65 % and 90 % all land
together) — the cost is the away window itself, not the energy the trip needs. That kills the
"large drop means a large charge to place" reading.

**Re-measured after R-93 (2026-10-02), same bench, same machine.** Making the EV's SoC a solved
variable did not cost solve time — it reduced it, and the away-window penalty this entry was opened
to explain is largely gone:

| variant | before (min) | after R-93 (min) | status after |
|---|---|---|---|
| no EV session | 0.16 s | **0.05 s** | Optimal |
| firm EV session | 0.26 s | **0.11 s** | GapLimit |
| forecast, car **home** | 0.19 s | **0.18 s** | GapLimit |
| forecast, car **away** 20 % | 0.90 s | **0.74 s** | Optimal |
| forecast, car **away** 65 % | ~0.90 s | **0.15 s** | GapLimit |
| forecast, car **away** 90 % | ~0.94 s | **0.14 s** | GapLimit |
| forecast, just returned | 1.02 s | **0.20 s** | GapLimit |

These are 5-repeat minima, the estimator this entry already argued for, and "noise only ever adds
time" makes a *lower* minimum the trustworthy direction — so the drop is real rather than a quiet
laptop.

**One caveat, stated rather than buried:** the "after" column reports `GapLimit` on five of seven
variants, and the earlier table did not record status at all, so part of the speed-up may be the
solver reaching its 2 % `mip_gap_target` sooner on the new formulation rather than doing less work
to the same quality. A fair like-for-like needs the gap sweep
(`bench_mip_gap_quality_sweep`) re-run against the SoC-variable model; until then read the table as
"no regression, probably a real improvement", not as a certified 5x.

Why it would plausibly be faster: the obligation is now a bound on one `soc_ev` variable instead of
a cumulative-energy sum over a deadline-masked slot range plus a floor that had to be pre-capped at
a reachability estimate, and the penalised shortfall slack removes a tight equality that sat next to
infeasibility. Fewer vacuous rows per away slot, and a relaxation the solver can bound earlier.

**Attempt 1 — tighten the away slots — is not demonstrated.** Away slots declare `p_ev` over the
full `[0, p_max]` range and force it to zero only through `p_ev[t] <= 0 * z_ev_on[t]`, and their
`z_ev_on` is a *binary* fixed at 0. Bounding that power to zero directly, declaring those `z`
continuous, and skipping the two now-vacuous rows per away slot gives: away-20 % 1.25 -> 0.94 s,
away-65 % 1.10 -> 0.91 s, but away-90 % 0.81 -> 0.90 s and just-returned 0.54 -> 1.02 s. Two better,
two worse, i.e. inside cross-process variance. **Not shipped** — model size looks like the wrong
lever here, exactly as it was for the heater (see GB-40's refuted single-integer encoding).

**Next measurement, before any further attempt:** time the two phases separately, as
`bench_heater_variants` already does for the heater. GB-40 established that phase 2 never binds and
burns its full budget; if the EV's 5x also lands in phase 2 then the away window is a phase-2
problem and every model-size idea is aimed at the wrong half.

**Phase split settles it: the EV away window is not worth optimising (2026-09-29).**
`bench_ev_phase_split` times the two phases separately, five repeats, minimum reported.

EV-only site, 288 slots:

| variant | phase 1 | phase 2 | statuses |
|---|---|---|---|
| no forecast | 0.044 s | 0.036 s | Optimal / Optimal |
| forecast, car home | 0.035 s | 0.079 s | Optimal / Optimal |
| forecast, car **away** | 0.039 s | **0.833 s** | Optimal / GapLimit |
| forecast, away, 90 % drop | 0.033 s | 0.681 s | Optimal / GapLimit |
| forecast, just returned | 0.037 s | 0.490 s | GapLimit / Optimal |

**Phase 1 is flat.** The entire away-window cost is phase 2 — ~20x — which is why the phase-1
tightening above was inconclusive: it aimed at the half that spends 40 ms.

Same site **with a heater**, the shape that actually times out:

| variant | phase 1 | phase 2 | statuses |
|---|---|---|---|
| heater + EV, no forecast | 57.5 s | 57.4 s | TimeLimit / TimeLimit |
| heater + EV, car home | 57.3 s | 57.2 s | TimeLimit / TimeLimit |
| heater + EV, car away | 56.9 s | 56.3 s | TimeLimit / TimeLimit |

**Both phases are already pinned at the timeout with or without the EV, and with no usage forecast
at all.** There is no headroom for the EV to consume, so removing the away-window cost cannot move
any VEN that times out; and the VENs where the 20x is visible (EV-only, e.g. ven-11) run at
0.8 s against a 60 s-per-phase budget, solving OPTIMAL. **Conclusion: do not optimise the EV away
window.** It has no operational payoff at either end of the fleet. 100 % of the real timeout is the
heater (GB-40).

**The cheap option this exposes, which is not EV-specific.** Phase 2 minimises friction subject to
`phase1_cap_expr <= c_star + epsilon`, so *by construction* everything phase 2 can change is
bounded by `phase2_epsilon_eur`. On a heater VEN it spends its entire 57 s budget and still returns
TimeLimit — i.e. it cannot prove the refinement it is buying, while the most that refinement can be
worth is one epsilon. A separately configurable, much shorter phase-2 timeout would therefore cut
heater-VEN solve time by roughly half at a cost bounded by epsilon, without touching the
formulation. That is a measurement worth doing (`planner.mip_gap_target` already makes the gap configurable per
profile; the phase budgets are not).

**CORRECTED: phase 2 is *effective*; the earlier "inert" finding was a test-fixture artefact
(2026-09-30).** The conclusion below about the 5 s budget stands, but the reasoning that produced it
was wrong and is replaced here.

*What was claimed:* that phase 2 returns its warm start unchanged on every instance — 0 of 288 slots
moved, friction identical from a 1 s to a 60 s budget — and therefore burns half the planner's
budget for nothing.

*Why it was wrong:* every one of those runs used `make_tariffs(...)`, a **flat** tariff. Flat prices
give phase 1 no reason to fragment the heater schedule, so there was no chatter for phase 2 to
remove. The inertness was a property of the fixture, not the planner.

*What phase 2 actually does,* measured with a diurnal tariff at ven-2's own production settings
(`mip_gap 0.06`, `phase2_epsilon_eur 1.00`, `bench_phase2_epsilon_sweep`): it moves 68 of 288 heater
slots and 31 import slots, cutting friction from 4.62 to 2.08 EUR. Against phase 1 alone it roughly
**halves heater switching** — 58 switches over 48 h down to 32, and 28 down to 11 over the first 8 h.
Phase 2 earns its place.

*The starvation threshold is real, just below production.* At `mip_gap 0.06`: epsilon 0.17 moves
nothing, epsilon 0.50 moves 59 heater slots, 1.00 moves 68, 5.00 moves 87. ven-2's 1.00 sits
comfortably above the threshold. A profile left at a tight epsilon would get no smoothing at all.

*Budget vs value* (`bench_phase2_budget_with_real_prices`, same settings): friction 4.6201 at 1-2 s
(no change), **3.4555 at 5, 10 and 20 s** (an exact plateau — 76 heater slots), 2.0805 at 60 s. So
there is no useful middle setting: 10 s and 20 s buy nothing over 5 s, and the remaining gain
appears only somewhere past 20 s.

*Why 5 s is nonetheless right* (`bench_phase2_budget_executed_window`): with `replan_interval_s` at
300 s only a plan's first slots ever reach the relay, and there 5 s and 60 s are **identical** — 2
switches at 25 min and 2 at 1 h under both. Over 8 h the 5 s solution has *fewer* switches (11 vs
15). The horizon-wide friction difference lives in slots that are replaced before they run.

**Phase 2 returns provably suboptimal incumbents, and its response to configuration is not
monotone (2026-09-30).** This is the sharpest thing the sweeps show, and it comes out of the data
without needing a mechanism.

Phase 2 minimises friction subject to `cost <= c_star + epsilon`. Raising epsilon strictly enlarges
the feasible set, so the true optimum at a larger epsilon **cannot be worse**. The sweep violates
that in both gap rows:

| mip_gap | epsilon | heat slots moved | friction |
|---|---|---|---|
| 0.02 | 0.17 | 51 | 2.2916 |
| 0.02 | **0.50** | **0** | **4.9113** |
| 0.02 | 1.00 | 47 | 1.9949 |
| 0.02 | 5.00 | 74 | 1.2471 |
| 0.06 | 0.17 | 0 | 4.6201 |
| 0.06 | **0.50** | 59 | **1.8315** |
| 0.06 | **1.00** | 68 | **2.0805** |
| 0.06 | 5.00 | 87 | 1.4559 |

Friction *rises* from 2.29 to 4.91 as the cap loosens from 0.17 to 0.50 (gap 0.02), and from 1.83 to
2.08 loosening 0.50 to 1.00 (gap 0.06). Both are impossible for optimal solutions, so phase 2 is
landing on substantially suboptimal incumbents — consistent with TimeLimit at 58-59 s in all eight
cells. The cells showing zero movement are therefore **search failures, not structural inertness**.

Three consequences:

1. **`phase2_epsilon_eur` cannot be tuned by measurement on one instance.** The response is not
   monotone, so a sweep can rank a tighter cap above a looser one purely by incumbent luck.
2. **Phase 2's freedom is inversely coupled to phase 1's quality.** The cap is anchored on
   `c_star` = phase 1's objective, so a sloppier phase 1 gives phase 2 more absolute room. Visible
   at epsilon 0.17: a timed-out phase 1 (gap 0.02) lets phase 2 move 51 heater slots, a converged
   one (gap 0.06) lets it move none. Tuning `mip_gap_target` therefore silently retunes how much
   smoothing the plan gets — an odd property for a lexicographic two-phase design.
3. **Phase 2 is a heater-only pass in practice.** The EV column is 0 in all eight cells.

**The budget is a staircase, not a curve** (`bench_phase2_budget_with_real_prices`): nothing below
~5 s, an exact plateau at 5/10/20 s (friction 3.4555, 76 heater slots), then a better basin past
20 s (2.0805, and only 68 slots moved — fewer edits, better result). Classic incumbent-update
behaviour.

**Default raised 5 s -> 15 s as a result.** The plateau means 15 s delivers the same plan quality as
5 s, while the threshold behaviour means 5 s is fragile: it is a **wall-clock** budget, not a work
budget, and production hosts run 85-89 % busy, so 5 s there buys less solver work than 5 s on an
idle laptop and would intermittently land in the found-nothing regime. 15 s keeps the plateau with
2-3x margin, still far below the 60 s it replaced (ven-2 total was 64 s before any of this).

**Heater MILP difficulty is set by the tank's thermal slack — a property of the installation, not
the formulation (2026-09-30).** This is the first mechanism proposed today that survived its control,
and it bears directly on GB-40.

Live ven-2 and ven-3 run the same assets, the same 288-slot 48 h grid and the same
`mip_gap_target` (0.06 — confirmed in both profiles, so the tolerance is controlled), yet phase 1
takes **227-309 ms** on ven-2 and **11-46 s** on ven-3. The profiles differ in tank physics:
ven-2 has 2000 L across a 40 K band, ven-3 has 200 L across 15 K — about 27x the usable slack.

`bench_phase1_vs_tank_slack` varies volume and band with grid, tariffs, gap and assets held fixed,
three repeats:

| volume L | band K | slack kWh | phase 1 (3 runs) | switches |
|---|---|---|---|---|
| 200 | 15 (**ven-3**) | 3.5 | 1.29 / 1.22 / 2.00 s | 53 |
| 200 | 40 | 9.3 | 0.66 / 0.70 / 1.21 s | 14 |
| 500 | 15 | 8.7 | 0.61 / 0.61 / 0.67 s | 12 |
| 1000 | 15 | 17.4 | 0.44 / 0.42 / 0.50 s | 16 |
| 2000 | 15 | 34.9 | 0.22 / 0.18 / 0.19 s | 4 |
| 2000 | 40 (**ven-2**) | 93.0 | 0.19 / 0.21 / 0.24 s | 22 |

Monotone in slack in every run, ~6-8x end to end, and the endpoints are exactly the two live VENs.
These solves finish (GapLimit) so they are deterministic — switch counts are identical across runs,
unlike the time-limited phase-2 measurements elsewhere in this entry.

**Switch count does *not* predict difficulty** (2000 L/40 K has 22 switches and is fastest; 500 L/15 K
has 12 and is slower), so "number of forced thermostat cycles" is not the mechanism. Slack itself is
the predictor. The likely reason is that low slack narrows the feasible tank-trajectory corridor
until integrality binds hard — standard MILP behaviour — but that is **not verified**.

**Why this matters for GB-40.** GB-40 has pursued the formulation for weeks on the theory that the
stage binaries carry the power level and weaken the relaxation, and three reformulations failed:
the single-integer encoding bought nothing, tier-bounded continuous power was unsound (~25 % cheaper
unphysical answers), dwell-time constraints were worse. GB-40 also records variance it cannot
explain — "not every heater VEN is slow (ven-2 18.2 s, ven-20 29.0 s)". Thermal slack explains that
variance, and explains why the reformulations failed: when slack is tight the discreteness they tried
to relax away is load-bearing.

**Levers this opens, none of them a reformulation:**
1. **Widen a needlessly narrow thermostat band** where the installation permits. Measured: 200 L
   going 15 K -> 40 K roughly halves phase 1 (1.29 -> 0.66 s). A physical/config change, and it
   also reduces switching (53 -> 14).
2. **Treat slack-poor VENs differently** — a looser `mip_gap_target` or coarser far zones for them
   specifically, rather than fleet-wide settings that are wasted on slack-rich sites.
3. Far-zone coarsening (measured 2.6x earlier in this entry) now has a mechanism behind it.

**Caveat on magnitude:** the bench's hardest row is 1.3-2.0 s while production ven-3 is 11-46 s, so
the bench under-represents ven-3 by roughly an order of magnitude — its base load, PV, EV state and
real tariffs compound on top. The *direction* is confirmed; the absolute scale is not this bench's
to give.

**ROOT CAUSE of ineffective smoothing: 9 of 11 heater VENs run an epsilon below the threshold at
which phase 2 can do anything (2026-09-30).** Found by the new `planner: phase timings` log, within
minutes of deploying it.

Live ven-2 vs ven-3, same host, same code:

| | ven-2 | ven-3 |
|---|---|---|
| phase 1 | **227-309 ms** | **11 131-45 566 ms** |
| phase 2 | 13-15 s, sometimes GapLimit | ~15 s, always TimeLimit |
| `phase2_epsilon_eur` | **1.00** | **0.17** |
| friction | ~1.33 | 4.30-5.60 |

Auditing every profile: `default_phase2_epsilon()` is **0.02**, and the sweep above shows epsilon
0.17 already yields **zero** heater slot changes at gap 0.06. Only **ven-2** sets a working value
(1.00); ven-3 sets 0.17; every other heater VEN — ven-5, ven-10, ven-12, ven-14, ven-15, ven-17,
ven-18, ven-20 — runs the 0.02 default. **They get no smoothing at all, and now pay a 15 s phase 2
for it.**

There is a mechanical floor behind this, not just a tuning preference: removing a heater switch
requires moving energy in time, which costs something. If epsilon is below that cost, no improving
move is feasible and phase 2 provably finds nothing however long it runs. ven-2's own profile
comment states the scale — effective switching cost `3.0 x 10/60 = 0.50 EUR/switch`, epsilon set to
2x that. The 0.02 default is **25x below a single switch**.

**Validation gap:** `validate.rs` already rejects an epsilon that is too *large* relative to
switching cost, but has no lower bound — nothing warns that phase 2 has been configured into
uselessness. A check that epsilon is at least the effective cost of one switch (when a heater is
present) would have caught this on every one of those nine profiles. Making it a hard error would
stop nine VENs from booting, so it should warn, or ship together with the profile fix.

**Remedies, for a decision rather than an unattended change:**
1. Raise `phase2_epsilon_eur` to ~2x effective switching cost on heater VENs (ven-2's 1.00 is the
   worked example) — phase 2 then halves switching, 58 -> 32 over the horizon.
2. Or set `phase2_epsilon_eur: 0.0` on them, which disables phase 2 outright and reclaims the 15 s.
   Honest, and strictly better than paying for a pass that cannot act.
Doing neither is the only option with no argument for it.

**Also corrects a claim made earlier today:** "phase 1 is negligible in production (~0.25 s)" was
drawn from ven-2 alone and is **wrong for ven-3**, whose phase 1 runs 11-46 s. Phase-1 cost is
strongly VEN-dependent, so parking phase-1 optimisation fleet-wide was premature — it is negligible
on ven-2 and dominant on ven-3.

**Phase 2's result at a fixed budget is NOT reproducible — every budget comparison here is
suspect (2026-09-30).** Two runs of `bench_what_is_in_phase2_friction` / `bench_phase2_budget_with_real_prices`
on the same instance, same code, same 60 s budget returned friction **2.0805** and **3.4555**. A
time-limited search explores however many nodes the host grants in that window, so its incumbent
depends on machine load. GB-40's finding that "HiGHS is deterministic here" applies to solves that
*finish*; it does not extend to time-limited ones.

Consequences for everything measured on this axis:

- The 5/10/20 s "plateau" and the better basin "past 20 s" may both be single-run artefacts. In the
  second run **5 s and 60 s were identical** (friction 3.4555, 32 switches), which argues 5 s is
  sufficient.
- `phase2_solver_timeout_s: 15` remains defensible as **load margin** — phase 2 achieves nothing
  below ~5 s, and a wall-clock budget pinned at that threshold on an 85-89 %-busy host would
  intermittently deliver no smoothing. The separate claim that a longer budget *captures more value*
  does not survive.
- Any future budget or epsilon decision needs repeated runs, not one sweep. This is the same
  one-sample trap that produced the retracted regression earlier in this file, in a new place.

**What is robust across both runs:** phase 2 halves heater switching, 58 -> 32 over the horizon.
Its value is real; only its budget-sensitivity was noise.

**A refuted hypothesis, recorded so it is not re-proposed.** `PV_USE_TIEBREAK_EUR_PER_KWH` (0.005
EUR/kWh) is documented as a bias "small enough that any real constraint still dominates" — which is
calibrated against phase 1's objective (tens of EUR of energy cost), while phase 2's objective is
friction-only (a few EUR). The concern was that the same constant becomes first-order in phase 2 and
that `friction_eur` is really a switching/PV blend. **It is not:** PV utilisation is identical
(91.87 kWh) across phase 1, the 5 s and the 60 s solutions — it is saturated, nothing is traded —
and the term is worth only -0.4594 EUR, not the ~2 EUR estimated from assuming continuous 6 kW
output. The estimate, not the constant, was wrong.

**Rule for any future phase-2 measurement: use a varying tariff.** A flat fixture makes phase 2 look
inert and will reproduce this wrong conclusion.

**Fix shipped:** `planner.phase2_solver_timeout_s`, separate from `solver_timeout_s`, defaulting to
**5 s** (validated non-zero; `phase2_epsilon_eur = 0.0` remains the documented way to disable phase
2 entirely). This is safe by construction as well as by measurement: phase 2's cap
`phase1_cost <= c_star + phase2_epsilon_eur` is a hard constraint in its own model, so any
incumbent it returns — however early — already respects the cost bound. Truncating it can only cost
smoothing, and measurably costs none. 5 s leaves ample headroom for the sites where phase 2 does
finish (EV-only: 0.08-0.83 s, Optimal).

**Result:** `bench_heater_solve_cost` total for the heater site falls from the **108.55 s** recorded
under GB-40 to **61.41 s** — phase 1's budget plus a bounded phase 2. The whole Rust suite also
halved, 117 s to 63 s.

**What this does *not* fix:** phase 1 still hits TimeLimit at ~57 s on every heater instance. That
is GB-40's standing problem and the remaining half. The two known levers there are already measured
in GB-40: a looser `mip_gap_target` (10 % put phase 1 on GapLimit for nine of ten instances at
+2.05 % mean cost) and the refuted reformulations. Phase 2 is no longer part of that problem.

**Open question worth answering:** *why* is phase 2 inert? Either phase 1's schedule is already
friction-optimal within the 0.17 EUR epsilon — plausible, since rescheduling a heater stage costs
more than epsilon allows — or the search cannot find the improvement. Raising `phase2_epsilon_eur`
and re-running `bench_phase2_changes_across_instances` distinguishes the two. If epsilon is the
binding constraint then phase 2 is not broken, it is starved, and the friction it is meant to remove
is simply unaffordable under the current cap.

**Production verification of the phase-2 budget, and two side effects (2026-09-30).** Measured from
`plan_history` before (29th 13:00-16:00Z) and after (30th 05:45Z+) the deploy:

| VEN | before median | after median | TIME_LIMIT before -> after |
|---|---|---|---|
| ven-2 (heater) | 64.0 s | **10.3 s** | 57 % -> 100 % |
| ven-3 (heater) | 72.2 s | **8.5 s** | 90 % -> 100 % |
| ven-1 (no heater) | 4.2 s | 4.6 s | 0 % -> **27 %** |

A 6-8x win on exactly the VENs that were hurting, with tight distributions (ven-2 min 9.8 s,
p90 10.4 s, n=11). It also corrects the model this work was reasoning from: live ven-2's phase 1 is
only ~5 s, so **phase 2 was consuming nearly all of its 64 s**, and the benchmark instance
(phase 1 at 57 s) is considerably harder than any real fleet VEN.

Two side effects, neither predicted:

1. **`TIME_LIMIT` is now the normal state for a heater VEN, which breaks it as a health signal.**
   Phase 2 always hits its 5 s cap, so `solve_status` reports TIME_LIMIT on ~100 % of cycles by
   design. GB-38 and GB-40 both use the TIME_LIMIT *rate* as the fleet's headline symptom, and that
   metric is now uninformative: it can no longer distinguish "phase 1 could not solve this site"
   (a real problem) from "phase 2 stopped at its intended budget" (normal). The status should
   carry the two phases separately — e.g. record phase-1 and phase-2 status independently on
   `Plan`, rather than one field that collapses them. Until then, treat TIME_LIMIT on a heater VEN
   as uninformative rather than as GB-40 evidence.
2. **Non-heater VENs lose some smoothing in their tail.** ven-1 went from 0 % to 27 % TIME_LIMIT:
   phase 2 used to converge there within 60 s and now the slowest quarter of its cycles are cut at
   5 s. The cost impact is bounded by `phase2_epsilon_eur` by construction, so this is lost
   friction smoothing rather than lost cost-optimality — but it is a real behaviour change the
   bench did not predict (an EV-only bench site finishes phase 2 in 0.08-0.83 s; production ven-1
   evidently carries more). If that smoothing turns out to matter, 10-15 s would still cut the
   heater VENs by ~4x while leaving non-heater sites room to converge.

**Phase-1 optimisation is parked; the 48 h horizon stays (2026-09-30).** Decided with the user.
Two reasons:

- **The 48 h span is a requirement, not an accident.** A receding-horizon controller needs lookahead
  well past the window it optimises, or end-of-horizon effects distort the near term — battery
  drained at the boundary, tank left cold, no preparation for the next morning's departure. The
  profiles say so explicitly (`plan_horizon_h: 48 # 2 solar windows for the 15.5 h-fill tank`). The
  executed-window comparison below is consistent with keeping it: 24 h and 48 h produce identical
  near-term behaviour, so the long horizon is not costing anything in decisions.
- **Phase 1 is only ~5 s on a live VEN.** ven-2's pre-deploy total was 64 s, of which phase 2 was
  ~57 s. The 57 s phase-1 figure that drove this investigation came from the *benchmark* instance
  (mip_gap 0.02, emergency-full heater), which is harder than any real fleet VEN — ven-2 runs
  mip_gap 0.06. Phase-1 work would therefore shave seconds off a solve that is no longer the
  problem.

If a real VEN ever does sit at the benchmark's difficulty, the measured options are recorded above:
coarsening the far zones keeps the 48 h span and bought 2.6x (192 vs 288 slots at the same span);
`mip_gap_target` 0.06 -> 0.10 has GB-40's 10-instance cost measurement behind it (+2.05 % mean); and
relaxing heater integrality in *far zones only* is the strongest untested idea but must be weighed
against GB-40's Arm 1, where decoupling heater power from the stage integer produced ~25 % cheaper
unphysical answers — confined to slots that never execute it may be acceptable, but it biases
lookahead optimistically, which can distort near-term deferral.

**The open item is phase 2's effectiveness, not its cost.** Its cost is now bounded at 5 s. What is
unexplained is why it changes nothing, and therefore whether even 5 s is worth spending or
`phase2_epsilon_eur = 0.0` (disable) is the honest setting.

**Methodological flaw found in this work's own bench:** it ran mip_gap 0.02 / epsilon 0.17 while
ven-2 runs 0.06 / 1.00. At 0.02 the bench's phase 1 *times out*, so its phase 2 inherits a poor
incumbent and a `c_star` derived from it — not the situation on a live VEN, whose phase 1 finishes.
Every phase-2 conclusion here was drawn on that unrepresentative configuration and is being re-run
across both gaps.

**Root cause of phase 1's time: horizon DURATION, not model size (2026-09-30).** Four controls,
all on the heater+EV site, phase 1 only, `bench_phase1_vs_horizon` /
`bench_phase1_flat_vs_priced_far_horizon` / `bench_phase1_count_vs_duration`:

| grid | slots | hours | phase 1 | status |
|---|---|---|---|---|
| 96 x 300 s | 96 | 8 | 0.39 s | GapLimit |
| **288 x 300 s** | **288** | **24** | **2.14 s** | GapLimit |
| **192 x 900 s** | **192** | **48** | **23.15 s** | GapLimit |
| 96x300 + 96x600 + 96x900 (**production**) | 288 | 48 | **60.03 s** | **TimeLimit** |

288 slots solve in 2 s over 24 h; 192 slots take 23 s over 48 h. **The cost scales with the span of
time modelled, not the number of integer decisions.** Production sits at 48 h and times out.

Three mechanisms were proposed and refuted along the way, each by its own control — worth recording
so they are not re-proposed:
- *EV comfort-band count* — merging equal-bid bands changed nothing (6406 -> 6752 ms).
- *Away-slot variables* — tightening them was inside cross-process variance, and the phase split
  later showed phase 1 spends 40 ms on the EV regardless.
- *Flat far-horizon pricing* (the GB-42 interaction) — 288 slots time out at 60 s whether the far
  half is flat-held or fully priced (60.03 s both ways). Pricing does improve plan *quality*
  (-5.08 -> -7.04 EUR) but not solve time.

Unverified hypothesis for *why* duration dominates: the tank cycles roughly every 100 min, so 48 h
holds about twice as many near-interchangeable thermostat cycles to coordinate as 24 h, and
interchangeable patterns are what stop branch-and-bound pruning. Not tested.

**It is waste, not harm — an earlier claim here is retracted.** `bench_does_the_far_horizon_harm_execution`
compares the part of the plan that actually runs before the next cycle replaces it:

| horizon | phase 1 | first-8 h cost | EV kWh (8 h) | heater switches (8 h) |
|---|---|---|---|---|
| 24 h | 2.05 s | 1.4994 EUR | 22.00 | 28 |
| 48 h (production) | 60.03 s | 1.4875 EUR | 22.00 | 28 |

Identical EV energy and switch count, and the 48 h plan's executed cost is marginally *lower*. So
the long horizon costs ~58 s per cycle and changes nothing the VEN carries out.

An interim claim that the 48 h horizon produced *worse* plans was wrong, and the reasoning behind it
was invalid: it argued a 48 h optimum could replicate a 24 h plan and then idle, making -7.04 vs
-9.98 EUR proof of suboptimality. The second day carries its own unavoidable base load and tank
losses, so the two objectives span different periods and cannot be compared that way.

**Candidate fix, not yet validated:** `plan_horizon_h` 48 -> 24 would buy ~28x on phase 1 for no
measured change in executed behaviour — the first change that addresses the fleet's actual timeout
rather than a neighbouring cost. Before proposing it as a default: repeat the executed-window
comparison across all ten `HEATER_VARIANTS` (one instance is not a result), and establish what the
far horizon is currently relied on for — EV deadlines beyond 24 h, VTN capacity obligations, and the
far-horizon zone's documented role in `VEN_ARCHITECTURE.md`.

**Still missing, and the reason this took a wrong turn first:** `plan_history` records
`solver_ms` but nothing about the *inputs*. A slow solve cannot be replayed. The targeted fix is
an input digest recorded when a solve exceeds a threshold — slot count, distinct tariff levels,
EV availability pattern and required energy, binary count — so the next occurrence is
reproducible offline instead of inferred from correlations.

**Where:** `VEN/src/controller/milp_planner/` (two-phase solve, `solver_timeout_s` default 60 s
per phase).

Solves of **20.8 s and 63.6 s** were recorded during E2E on 2026-09-27 on a host under load —
against a 60 s per-phase timeout, i.e. at and past the ceiling that produced GB-38 (three VENs
hitting TIME_LIMIT on essentially every solve for 24 h and never charging their EVs). GB-38 is
marked resolved for that specific run, but nothing has reduced the underlying solve cost, so
the same failure is one busy host away.

**To resolve:** measure where the time goes before tuning anything — heater tier binaries and
EV semi-continuous constraints are the usual suspects — then decide between a cheaper
formulation, a coarser far-horizon zone, or an honest raise of the timeout with a visible
TIME_LIMIT surface. Related: R-93 (a per-slot EV SoC variable would *add* variables, so it
must be costed against this).

---

## Appendix B — GB-40's backlog entry, moved from `docs/BACKLOG.md` (2026-08-24 to 2026-09-30)

GB-40 and R-97 were the same subject and were merged into R-97 on 2026-10-09. The entry's text is
kept here unchanged. One statement in it is out of date: the MIP gap is a live per-profile setting
(`planner.mip_gap_target`), not reverted.

**Finding (2026-09-30).** **Thermal slack explains the per-VEN variance this entry could not (2026-09-30, R-97).** Live ven-2 and ven-3 share assets, grid, horizon and `mip_gap_target` 0.06, yet phase 1 takes 227-309 ms on ven-2 and 11-46 s on ven-3. ven-2 has a 2000 L tank across a 40 K band, ven-3 has 200 L across 15 K — ~27x the usable slack. `bench_phase1_vs_tank_slack` (three repeats, everything else held fixed) is monotone in slack, ~6-8x end to end, with the two live VENs as its endpoints. Switch count does **not** predict difficulty, so it is slack rather than cycle count; the likely mechanism (unverified) is that low slack narrows the feasible tank-trajectory corridor until integrality binds. This explains why all three reformulations below failed — with tight slack the discreteness they tried to relax away is load-bearing — and points at levers that are not reformulations: widen a needlessly narrow thermostat band where the installation allows (200 L at 15 K -> 40 K roughly halves phase 1 and cuts switching 53 -> 14), or treat slack-poor VENs specifically with a looser gap or coarser far zones instead of fleet-wide settings wasted on slack-rich sites.

**History and measurements.** A heater in a VEN's asset mix costs ~4.7× the MILP solve time of any other mix, and is the concrete driver behind GB-38's fleet-wide `TIME_LIMIT` symptom. Measured across all 20 VENs in one S-7 window (`experiments/results/20260824-0312-s7_stress/*-plan-history.json`): heater VENs mean **84.2 s** per solve (n=10) vs **18.0 s** without (n=10), and the eight slowest VENs in the fleet *all* carry a heater. Six of them sit at 105–121 s, i.e. pinned to the `solver_timeout_s: 60` two-phase ceiling, so they time out on essentially every cycle. This compounds: a `TIME_LIMIT` solve burns its **full** budget before giving up, so the slowest VENs are also the most CPU-expensive, which starves the rest and pushes more of them into timeout. Node2 (17 VENs, 4 cores) consequently runs 85–89% busy with a run queue of 4–5 — expected concurrent solves ≈ 17 × 51.7 s / 300 s ≈ 2.9 on 4 cores (measured 2026-08-25, `docs/history/fleet_run_journal.md`). **Confirmed fleet-wide on the 24h S-9 run (2026-08-26, 5111 solves): heater VENs `TIME_LIMIT` on 70% of solves (1643/2332) against 8% (226/2779) without — a ~9× split, the clearest signal in the whole fleet dataset.** That run also bounds the *consequence*: `TIME_LIMIT` degrades cost-optimality, it does not break function — ven-3 and ven-5 timed out on ~100% of solves and still charged their EVs to 76.6% and 79.5%, because a timed-out MILP returns a feasible incumbent. An interim claim that this superseded GB-38's root cause was wrong and has been retracted there. Not every heater VEN is slow (ven-2 18.2 s, ven-20 29.0 s are both heater-bearing), so it is the heater's integer relay/staging variables *interacting* with other assets' continuous variables that should be suspected, not the heater alone. **Measured in isolation (2026-08-25, `VEN/src/controller/milp_planner/tests/solve_cost.rs`)**: the same ven-3-shaped site on the same 288-slot grid, solved with and without the heater and nothing else changed, gives **0.19 s without / 108.55 s with — 561×**. The with-heater figure is essentially the two-phase `solver_timeout_s` ceiling, i.e. an *active* heater does not merely slow the solve, it **times it out**. The fleet's gentler 4.7× is an average that dilutes active heaters with idle ones (`MustNotRun` fixes every `z` to 0, leaving nothing to branch on), so 561× is the real cost of a heater that is actually running. Debug build, but the caveat is immaterial here: the no-heater case at 0.19 s shows Rust-side constraint building is negligible, so the 108.55 s is essentially all HiGHS branch-and-bound. **Diagnosis (code-grounded)**: the cause is not binary *count* — battery VENs declare comparable numbers (`u_bat` + `z_active` + `delta_active`) and solve in 18–50 s. It is that the heater's binaries carry the **power level itself**, not a mode. Battery/EV power (`p_ch`/`p_dis`) are continuous variables whose binary only picks a direction, so the LP relaxation is tight; the heater has *no* continuous power variable at all — `P_heat = p_mid·z_mid + p_full·z_full` (`heater_milp.rs` C2), so the only way the relaxation can express the intermediate power that tank-trajectory tracking almost always wants is a fractional `z`. Nearly every slot therefore relaxes fractional, and branch-and-bound must branch across all 2n heater binaries (n=288 on the standard `plan_zones` grid, identical for every heater VEN, so the grid is not the differentiator). Compounding it, **no min-up/min-down (dwell-time) constraints exist anywhere** — anti-chatter is only the soft `sw` switching penalty, which prices chatter but does nothing to tighten the relaxation or prune the tree. Likely also why ven-2/ven-20 are fast: a heater in `MustNotRun` has all `z` fixed to 0 (`heater_milp.rs` build), leaving nothing to branch on — worth confirming those two were simply idle in that window rather than structurally cheaper. **Single-integer stage encoding: REFUTED the degeneracy diagnosis (2026-08-28).** The heater's two tier binaries (`z_heat_mid`/`z_heat_full` + mutual exclusion) were replaced by one general integer `y ∈ [0, n_stages]` with `P = p_step_kw · y`, on the theory that the old encoding was *degenerate* — the LP could express one power several ways (4.5 kW as `(0.5, 0.5)` or `(0, 0.75)`), and that redundancy stalls branch-and-bound. The reformulation halved heater variables (576 binaries → 288 integers), deleted all 288 mutual-exclusion rows and halved the switching rows. **Solve time did not improve**: benchmark with-heater went 108.55 s → 116.60 s, both pinned at the two-phase `solver_timeout_s` ceiling (the without-heater case moved 0.19 s → 0.08 s on a warm cache, which is why the printed ratio rose to 1373× — noise, not signal). So the bottleneck is **neither variable count nor representation degeneracy**. What this usefully isolates: the original diagnosis bundled two claims — (a) *the binaries carry the power level, so the relaxation is weak*, and (b) *there are redundant fractional representations*. Fixing (b) alone bought nothing, so (a) is the live hypothesis and (b) is dead. A fractional `y = 1.5` still fakes 4.5 kW, exactly as fractional `z` did; removing the duplicate spellings of that fake did not make it any less available to the relaxation. The remaining GB-40 options are therefore re-ranked: **tier-bounded continuous power** (give the heater a real continuous `P` variable bounded by the stage integer, so the relaxation stops needing fractional integrality to express intermediate power) now looks like the only one that attacks (a) directly; dwell-time constraints and horizon truncation attack tree size rather than relaxation strength, which is the thing just shown not to be the binding constraint. The reformulation itself was kept for reasons independent of solve time — see the entry below. **Both re-ranked options were then A/B tested and both failed (2026-08-28, branch `experiment/heater-milp-tightening`, harness `bench_heater_variants` — five fixed start conditions, phases solved and timed separately so a single status cannot hide which phase timed out).** Baseline phase 1: `TimeLimit` on all five at 54–57 s, objectives 5.5849 / 5.6611 / 5.3549 / 5.4285 / 9.2006, totals ~109–112 s. **Arm 1 (tier-bounded continuous power)** is dramatically faster — phase 1 `Optimal` in 0.14–0.24 s, totals 2.0–12.4 s — but **unsound**: its objectives are 4.1297 / 4.1891 / 4.0993 / 4.0296 / 7.2566, ~25% *below* baseline on every instance. A valid reformulation of the same problem cannot find a cheaper optimum than a feasible incumbent of the original; a lower objective means it is solving an easier, unphysical problem — decoupling `P` from the stage integer lets the model draw any power while holding the stage flat, evading both the staging physics and C5's switching cost. The speed is the symptom, not the prize. This kills hypothesis (a) as a *fixable* weakness: the relaxation is weak precisely because the discreteness is real. **Arm 2 (min-up/min-down dwell constraints, k=3 slots)** gave no speedup at all — phase 1 still `TimeLimit` at 54–57 s on all five — with objectives *worse* (6.8738 / 24.0659 / 7.3568 / 6.9771 / 10.4853; instance 2's 24.07 is a 4× degradation, dwell forcing long uneconomic on-blocks) and phase 2 returning `Err` on four of five. So tightening via dwell also fails. **Standing diagnostic**: relaxing integrality outright makes the instance trivial (0.2 s vs a 54 s timeout), and two independent measurements bracket the relaxation gap at ~20–26%, so the entire difficulty lives in the discrete stage decisions and no reformulation tested so far removes it without changing the physics. Neither arm merits merging; the branch exists only as the record. **Reproduced on the 2026-09-08/09 S-9 re-run** (`docs/history/fleet_run_journal.md`, "S-9 re-run #4"): heater VENs `TIME_LIMIT` on 1512/2409 distinct plans (63%) vs 152/5399 (3%) without — the same ~9-10× split as 70%/8%. Within the heater group: ven-15/ven-3 100%, ven-5 89%, ven-18 87%, ven-10 80%, ven-17 66%, ven-12 58%, ven-14 48%, ven-20 15%, ven-2 8%. Time spent in the heater's emergency latch (GB-44, fixed 2026-09-12) does not explain the spread (ven-3/ven-15: 100% with 0% latch time). **TIME_LIMIT can break cap compliance, not just optimality (2026-09-12 smoke run):** ven-3's first plan after a 1.5 kW `capacity_limit` arrived (`RATE_CHANGE`, `TIME_LIMIT` at 120 s) scheduled its heater at 3.0 kW inside the cap (tank 58.8 °C, far above its 45 °C floor, so a planner choice, not the thermostat). Imported 3.0 kW against 1.5. A timed-out solve returns its incumbent, and since the cap is a penalised slack, not a hard constraint, that incumbent can violate it. This contradicts the earlier "degrades cost-optimality, not function" bound for the grid-compliance case. Single observation (4-min cap, shorter than the 5-min pass bar); the 2026-09-12 campaign's S-3/S-7/S-9 compliance per heater VEN should confirm or bound it. **Execution side closed by GB-47 (2026-09-15):** the arbiter's limit-enforcement pass (on by default) sheds a stage a timed-out incumbent put inside a hard limit, from the next tick — the S-7 re-run with it off reproduced ven-10/ven-12's failure, with it on both held their floor (`docs/history/fleet_run_journal.md`). What remains here is the planner side: a feasibility-first phase (or a repair step) so a timed-out incumbent respects the cap in the first place, instead of relying on execution to correct it. (A same-day note here claiming the split "does not reproduce" was a classification error — VENs were picked by grepping profiles for the word "heater", which matched comments — and has been replaced.)

**MIP gap: measured, then reverted (2026-08-27).** The gap was briefly made per-profile configurable to test whether loosening it relieves the timeouts; the code was reverted, but the measurement is kept so nobody repeats it. Swept at 2/5/10/20/35/50% on the benchmark, three repeats, phases timed separately: phase 1 holds `TimeLimit` at ~54 s through 10%, then flips to `GapLimit` at 20% (9.96 s), 35% (10.84 s) and 50% (7.38 s); total falls ~109 s → ~64 s. **Phase 1's achieved gap therefore lies between 10% and 20%** — a healthy MILP would close to 2%, so this quantifies the weak relaxation diagnosed above, and it is otherwise unobservable through `good_lp` (R-65). **Phase 2 never binds at any gap**, burning its full ~54 s in all six rows, so it is the larger remaining half and needs its own answer. The quality price was never established: the benchmark reported phase 2's objective, which tracks wall-clock work when both phases time out (two rows varied across repeats; 35% returned exactly the 2% objective; 50% appeared *better*), so an interim claim that 10% cost +8.72% in plan quality was noise. **The quality price has since been measured properly (2026-08-28)** — paired against the baseline series below, same five instances, `MIP_GAP_TARGET` the only difference, reading phase 1's own objective: 5.5849→5.6547 (+1.25%), 5.6611→5.8694 (+3.68%), 5.3549→5.4907 (+2.54%), 5.4285→5.7339 (+5.63%), 9.2006→9.8103 (+6.63%); **mean +3.9%**, while phase 1 goes from `TimeLimit` on all five (54–57 s) to `GapLimit` on all five (2.8–16.2 s). Two things follow. First, the realized loss is far below the tolerance: a 20% gap bounds the *worst case* distance to the true optimum, and branch-and-bound reaches a near-optimal incumbent early and then spends nearly all its time proving optimality — the gap buys out the proving, not the solution. Second, **it does not fix GB-40**, because phase 2 still burns ~55 s `TimeLimit` on all five; totals only fall ~110 s → 57–71 s. Caveat, in the safe direction only: gap-20 also had ~4× less search time, so +3.9% is an *upper* bound on the tolerance's own cost. This makes the 2026-08-27 revert look premature — it was decided on an assumed quality cost, and the measured one is small — but reinstating configurability should be argued on its own merits (R-27), not as a GB-40 fix. Note R-27 in `docs/reference/TECHNICAL_DEBTS.md` still asks for this constant to be exposed via config — that request predates and outlives this revert

**MIP gap: fine sweep finds 10% is the actual optimum, not 20% (2026-08-29).** The 2026-08-28 measurement above priced two points (2% and 20%) and reported +3.9% as *the* quality cost — accurate for that point, but never checked whether a better point existed between them. Swept 9 gaps (2/4/7/10/13/16/18/20/22%) across the same 5 fixed instances (`bench_mip_gap_quality_sweep`, 45 debug-build solves, 4012 s total), reading each instance's own phase-1 objective against its 2% baseline: mean cost is +0.00% (2%), +0.21% (4%), +0.43% (7%), **+0.65% (10%)**, then jumps to +5.55% (13%) and stays in a noisy +4–7% band through 22% with no further trend (16% +7.13%, 18% +6.70%, 20% +3.94%, 22% +6.53% — bouncing, not climbing, so branch-and-bound incumbent variance past 10%, not a cost that scales with gap size). The gap starts binding (`GapLimit` on phase 1) at **7%**, earlier than the previous sweep's "somewhere between 10% and 20%" bracket. **10% is therefore the target, not 20%**: same qualitative win (phase 1 off `TimeLimit`) at roughly a sixth of the quality cost. Caveat: even past the binding point, per-instance phase-1 time is not uniformly low (instance 4 took 46.81 s at 10%, instance 1 took 48.43 s at 18%) — "loose gap" lowers the average, it does not guarantee a fast solve on every instance. Phase 2 remains untouched at every gap tested (`TimeLimit`, ~54–57 s on all 45 solves), reconfirming it as the actual remaining bottleneck. This does not change the standing conclusion that the gap knob does not fix GB-40 — it only refines what the knob should be set to if used

**MIP gap: extended to 10 instances — the instability is real dispersion, not noise, and "10% ≈ free" doesn't survive harder instances (2026-08-29).** The many conclusions on this topic had been unstable (20%→10% as "the" optimum, a noisy non-monotonic band above 13%), so this re-run doubled the instance count precisely to tell real signal from an artifact of too few samples. **Reproducibility is confirmed first**: the original 5 instances are the first five entries of the now-10-entry `HEATER_VARIANTS`, and every one of their phase-1 objectives reproduced bit-for-bit against the prior run at every gap checked (15/15 spot-checked points, zero mismatches) — HiGHS is deterministic here, so the instability is not run-to-run solver noise. The 5 new instances push into ground the original 5 barely touched: both temperature extremes (against `temp_min_c=45`/`temp_max_c=60`, not just the mid-band), all three power stages evenly, and — the load-bearing change — a price range of 0.10–0.60 €/kWh against the original 0.25–0.40. Result, mean Δ vs each instance's own 2% baseline: +0.00% (2%), +0.87% (4%), +1.78% (7%), **+2.05% (10%)**, +3.98% (13%), then +9.58% (16%) / +6.59% (18%) / +7.35% (20%) / +8.51% (22%). Two things follow. First, **the 10%≈"nearly free" framing does not hold up**: 10 instances put the true mean at 10% around **3× higher** (+2.05% vs the 5-instance +0.65%) — the smaller sample was biased toward easier, more typical mid-band conditions and understated the cost of the two price-extreme instances added here. Second, **the per-gap spread (max − min across instances) grows from 0 at 2% to ~17 percentage points by 20%**, and one instance ("cool-ish, mid stage, very expensive power") returns the *identical* objective (+16.63%) at 16%, 18%, 20% and 22% — the same incumbent, because branch-and-bound already stopped there and every looser setting past that point is moot for that instance. That is a real structural property of this MILP (each instance has its own gap threshold where a step-jump happens, and averaging superimposes different thresholds), not measurement noise, so no single gap value produces a clean monotonic quality curve across a mixed fleet. Practically, **10% and 13% are the two defensible choices, not one clear optimum**: at 10%, nine of ten instances flip off `TimeLimit` and mean cost is the lowest post-binding value (+2.05%); at 13%, all ten flip (the first gap where every instance is `GapLimit`) but mean cost nearly doubles (+3.98%). Past 13%, cost keeps climbing with no further status-flip benefit, since everything is already off the clock. Phase 2 remains `TimeLimit` on all 90 solves regardless of gap, unchanged from every prior measurement — still the actual bottleneck
