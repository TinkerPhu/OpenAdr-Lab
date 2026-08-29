# GB-40 MIP-Gap Benchmark

Reference for `bench_mip_gap_quality_sweep` and `bench_heater_variants`
(`VEN/src/controller/milp_planner/tests/solve_cost.rs`), the harness that
measures the heater MILP's sensitivity to `planner.mip_gap_target`. Ignored by
default (production-sized solves); run explicitly:

```text
wsl cargo test -p ven-app bench_mip_gap_quality_sweep -- --ignored --nocapture
```

## Fixture

All 10 scenarios share one profile (`bench_profile(true)` in the harness): a
ven-3-shaped site on the production three-zone grid (96 slots × 300 s + 96 ×
600 s + 96 × 900 s, 48 h horizon), with EV (11 kW max charge, 75 kWh battery,
30 % → 80 % SoC target), 6 kW PV, and a 0.6 kW base load held fixed. Only the
heater's own state and the import tariff vary between scenarios.

Heater: `max_kw: 6.0`, `power_stages: 2` (three power levels: 0 / 3 / 6 kW),
`temp_min_c: 45.0`, `temp_max_c: 60.0`, `switching_penalty_eur: 0.50`.
`solver_timeout_s` is the default 60 s per phase (120 s two-phase ceiling).

## The 10 scenarios

Each varies tank temperature, the heater's already-committed power stage, and
the import tariff — three axes chosen so the set covers both temperature
extremes (against the 45–60 °C bounds), all three power stages evenly, and a
price range wider than the fleet's typical 0.25–0.40 €/kWh band.

| # | tank °C | initial stage (kW) | import €/kWh | label |
|---|---|---|---|---|
| 1 | 47.82 | 6.0 | 0.25 | cool tank, emergency full |
| 2 | 46.0 | 0.0 | 0.25 | near T_min, starting off |
| 3 | 50.0 | 3.0 | 0.25 | mid-band, mid stage |
| 4 | 55.0 | 0.0 | 0.25 | warm tank, little need |
| 5 | 47.82 | 6.0 | 0.40 | cool tank, expensive power |
| 6 | 45.5 | 3.0 | 0.25 | near T_min, mid stage |
| 7 | 59.5 | 6.0 | 0.25 | near T_max, emergency full |
| 8 | 52.0 | 0.0 | 0.15 | mid-band, off, cheap power |
| 9 | 48.0 | 3.0 | 0.60 | cool-ish, mid stage, very expensive power |
| 10 | 56.0 | 6.0 | 0.10 | warm tank, full power, very cheap power |

Scenarios 1–5 are the original set; 6–10 were added to extend coverage.
Scenarios 7 and 10 are deliberately "contradictory" combinations (full power
already committed near the top of the temperature band; full power at a warm
tank with cheap import) that a real fleet VEN can land in mid-transition.

## Results: quality cost and time by gap

9 gap values (2/4/7/10/13/16/18/20/22 %) × 10 scenarios = 90 solves, phase 1
and phase 2 timed and reported separately since `solve_milp_two_phase`
returns only the winning (phase-2) solution status. "Quality cost" is each
scenario's phase-1 objective at the given gap versus its own value at 2 %,
averaged across all 10 scenarios.

| gap | mean phase-1 time | mean quality cost | phase-1 timeouts | mean phase-2 time | phase-2 timeouts |
|---|---|---|---|---|---|
| 2 % | 48.63 s | +0.00 % | 8/10 | 55.45 s | 10/10 |
| 4 % | 55.24 s | +0.87 % | 10/10 | 55.14 s | 10/10 |
| 7 % | 29.45 s | +1.78 % | 3/10 | 55.55 s | 10/10 |
| 10 % | 23.00 s | +2.05 % | 1/10 | 54.59 s | 10/10 |
| 13 % | 11.14 s | +3.98 % | 0/10 | 55.89 s | 10/10 |
| 16 % | 16.89 s | +9.58 % | 2/10 | 55.50 s | 10/10 |
| 18 % | 11.78 s | +6.59 % | 0/10 | 55.19 s | 10/10 |
| 20 % | 7.22 s | +7.35 % | 0/10 | 55.99 s | 10/10 |
| 22 % | 7.12 s | +8.51 % | 0/10 | 55.39 s | 10/10 |

"Timeouts" counts scenarios whose phase reported `TimeLimit` rather than
`GapLimit`/`Optimal` — i.e. the solver used its full budget rather than
stopping once within the target gap of the best known bound.

## Reading the table

- **Phase 2 does not respond to `mip_gap_target` at all.** ~55 s and 10/10
  timeouts at every gap tested. `mip_gap_target` is applied to phase 2's own
  solve too, but its friction-minimisation objective never gets close enough
  to its bound within any tested value. Phase 2 is therefore the binding
  constraint on total solve time regardless of this setting.
- **The relationship between gap and quality cost is not smooth.** 4 % costs
  more phase-1 time than 2 % (55.24 s vs 48.63 s) because two scenarios that
  happened to prove optimal early at 2 % did not repeat that luck at 4 % —
  branch-and-bound's interaction with a gap target is not monotonic
  pointwise, only in aggregate trend.
- **A `TimeLimit` phase still returns a feasible, usable incumbent** — it is
  a status meaning "did not prove optimality in budget," not a marker of a
  broken plan (`docs/BACKLOG.md` GB-38).
- **Diminishing returns set in around 10–13 %.** 2 %→10 % trades +2.05 %
  quality cost for 25.6 s of phase-1 time; 10 %→13 % trades another +1.93 %
  for only 11.9 s more — less than half the time benefit for a similar price.
