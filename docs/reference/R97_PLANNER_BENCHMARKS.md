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
