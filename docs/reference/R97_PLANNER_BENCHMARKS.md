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
