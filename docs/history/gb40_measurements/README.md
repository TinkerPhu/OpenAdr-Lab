# GB-40 raw measurement logs

Raw solver output behind the GB-40 conclusions in `docs/BACKLOG.md` and the
summarised tables in `docs/reference/GB40_MIP_GAP_BENCHMARK.md`. Kept because
the published tables report *aggregates* (per-gap means, the fixture
definition) while these files hold the **per-instance rows** — the only record
of how individual scenarios behave, and the basis for any future re-analysis
that the aggregates cannot support. Regenerating the largest of them costs
~2 h 15 m of solve time.

Point-in-time records, exempt from the current-state documentation rule
(`docs/reference/DOCUMENTATION_STYLE.md` exempts `docs/history/**`). They are
not updated: a new measurement gets a new file.

| file | what it holds |
|---|---|
| `gap_sweep_10profiles.log` | 9 gaps × 10 heater scenarios = 90 solves, one row per combination (phase-1 time/status/objective/Δ vs 2 %, phase-2 time/status), plus the per-gap mean summary. The current basis for GB-40's gap recommendation. |
| `gap_quality_sweep.log` | The earlier 9 gaps × 5 scenarios = 45 solves, same row format. Superseded by the 10-scenario run, kept because its first five scenarios are identical inputs — the pair is what established that HiGHS reproduces bit-for-bit here, so the observed spread is real dispersion rather than solver noise. |
| `ab_baseline.log` | Committed-baseline arm of the heater-formulation A/B: 5 scenarios, phases timed separately. |
| `ab_arm1.log` | Arm 1 (tier-bounded continuous power). Faster but unsound — objectives ~25 % below baseline, i.e. it solved an easier, unphysical problem. |
| `ab_arm2.log` | Arm 2 (min-up/min-down dwell constraints, k=3). No speedup, worse objectives, phase 2 `Err` on 4 of 5. |
| `ab_gap20.log` | The 20 %-gap quality measurement that the original gap sweep failed to make, on the same 5 scenarios. |

Fleet-run logs are deliberately absent: `experiments/run_logs/` is gitignored,
so fleet run output is treated as disposable and is not mirrored here.

Reproduce with:

```text
wsl cargo test -p ven-app bench_mip_gap_quality_sweep -- --ignored --nocapture
wsl cargo test -p ven-app bench_heater_variants        -- --ignored --nocapture
```
