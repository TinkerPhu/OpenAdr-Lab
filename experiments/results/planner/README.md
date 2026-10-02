# Planner solve-cost experiment log

Append-only record of planner solve-cost experiments: the **parameters** that
defined each run, the **code** it ran against, and the **results**. One JSON object
per line (`*.jsonl`), so it greps, diffs in git, and stays small.

## Why a log and not an archive

Results split into three classes, and only one needs disk:

| class | example | reproducible? | stored |
|---|---|---|---|
| 1. deterministic benchmark | `bench_phase1_vs_tank_slack` (solves finish; switch counts identical across runs) | yes, from the commit | **parameters + results + commit.** Raw stdout: no |
| 2. non-reproducible measurement | time-limited phase-2 solves (same 60 s gave friction 2.0805 and 3.4555); any fleet observation | **no** | parameters + results + commit, same schema. Irrecoverable if skipped |
| 3. derived artefact | `VEN/target` (9.5 GB), docker build cache (~70 GB/host) | yes, mechanically | **not stored.** Reclaim only what the next run will not reuse |

Each line is a few hundred bytes. A thousand runs is under a megabyte, so there is
no compression problem here — the storage pressure is entirely class 3.

## Schema

```json
{
  "ts": "2026-10-01T06:12:00Z",
  "commit": "0f0c75c8",
  "bench": "bench_phase1_vs_tank_slack",
  "class": 1,
  "params": {"volume_l": 200, "band_k": 15, "mip_gap": 0.06, "slots": 288, "hours": 48},
  "results": {"phase1_s": 1.29, "switches": 53, "status": "GapLimit", "objective_eur": -7.3168}
}
```

`params` and `results` are free-form per bench; `ts`, `commit`, `bench` and `class`
are always present. `commit` is what makes a class-1 line regenerable — check out
that SHA and re-run the named bench.

## How records get written

Benchmarks print `@@RESULT {json}` lines. `scripts/run_planner_experiment.sh <bench>`
runs one, stamps each record with the timestamp, commit and class, and appends to
the right `.jsonl`. Nothing is hand-copied, so the log cannot drift from the code
the way a prose table does.

## Reading conclusions out of it

```bash
# every tank-slack measurement, slack vs phase-1 time
jq -r 'select(.bench=="bench_phase1_vs_tank_slack")
       | [.params.volume_l, .params.band_k, .results.phase1_s] | @tsv' \
   experiments/results/planner/solve_cost.jsonl

# has a change made phase 1 faster for a given grid, across commits?
jq -r 'select(.bench=="bench_phase1_vs_zones")
       | [.commit, .params.slots, .results.phase1_s] | @tsv' \
   experiments/results/planner/solve_cost.jsonl
```

Narrative conclusions live in `docs/reference/R97_PLANNER_BENCHMARKS.md`; this file
is the evidence they are drawn from.

## Cleanup rule

After a run, remove only what the next experiment will not reuse. Do **not** prune
docker build cache or `target/` between runs on the same code — rebuilding it costs
more than it saves. Clean when switching away from a line of work, not between
iterations of one.
