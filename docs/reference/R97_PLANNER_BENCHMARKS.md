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
