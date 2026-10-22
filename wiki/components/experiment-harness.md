---
title: Experiment Harness
type: component
created: 2026-07-11
updated: 2026-10-22
synced_commit: 650b626
sources: [experiments/, VEN/src/tasks/sim_tick/tick.rs, docs/guidelines/FLEET_EXPERIMENT_DESIGN.md, docs/reference/R97_PLANNER_BENCHMARKS.md]
tags: [experiments, kpi, scenarios, phase3]
---

# Experiment Harness

A scripted way to compare the control methods — price signals, capacity limits and
reservations, grid alerts, SIMPLE levels, direct dispatch — on measured KPIs instead of by
hand (`docs/BACKLOG.md` BL-33). It lives in `experiments/` (Python, like the E2E tooling) and
runs from the docker host or, for a two-host fleet, from a workstation that can reach both
([[fleet-tooling]]). The design of a fleet run (what to measure, what pass means) is in
`docs/guidelines/FLEET_EXPERIMENT_DESIGN.md`.

## The real-time constraint

Scenarios run in **real time**. The sim tick reads the wall clock once per tick
(`VEN/src/tasks/sim_tick/tick.rs`) and every event window is an absolute timestamp, so
time acceleration is not externally drivable; it would need an injectable clock through the
whole tick and poll path, not only the planner (which has one under the determinism rule).
The short scenarios are 30-minute same-day windows; a full fleet matrix is about ten hours
of wall clock. `run_experiment.py` therefore retries transient network failures
(`_with_retry`, four attempts with backoff — a single dropped link once killed a whole
batch mid-scenario) and line-buffers its stdout, so a day-long run is observable from its
log.

## Scenarios (`experiments/scenarios/*.yaml`)

Declarative: a duration plus actions at relative minute offsets. Action types:
`price_series`, `capacity_limit`, `export_capacity_limit`, `capacity_reservation`, `alert`,
`simple`, `dispatch` and `budget_shortfall`. Each scenario has a `tier` (`realistic` or
`stress`):

| Scenario | Tier | What it exercises |
|---|---|---|
| `s1_flat` | realistic | Flat tariff; `is_baseline: true` |
| `s2_price_spike`, `s3_capacity_limit`, `s5_dispatch`, `s6_combined` | realistic | One control method each, then combined |
| `s4_alert`, `s7_stress` | stress | A grid alert; a tight capacity limit overlapping an alert |
| `s8_budget` | stress | A deliberately under-budgeted EV `MAX_COST` request |
| `s9_diurnal` | realistic | A full real day's duck-curve price, an evening import cap and a midday export cap |
| `s10_overexport` | stress | A tight export cap over peak PV |
| `smoke` | — | A 3-minute self-test of the harness, not an experiment |

`budget_shortfall` is the one action that bypasses the VTN: it is a direct
`POST /user-requests` to one target VEN, so that VEN's own planner raises a real
BudgetShortfall warning ([[hems-planning]], [[milp-planner]]). `s9` and `s10` are anchored
to real solar time with `--start-at` (the PV arrays face west, so the export peak is mid to
late afternoon, not solar noon) and run without the paired baseline.

## Pipeline

1. **`run_experiment.py`** — creates a program, posts each action's event at its offset,
   waits out the window, deletes what it created (deletion is cancellation, [[openadr-3]]),
   then snapshots each VEN's `history.sqlite` ([[history-store]]) **including the
   `-wal`/`-shm` sidecars** (the stores are WAL-mode and only checkpoint at the daily
   prune, so copying the main file captures nothing) plus the `lab_recorder` tables as CSV,
   windowed to the run. Remote VENs are pulled over scp/ssh according to
   `experiments/fleet_map.json`; `--pg-host` routes the recorder dump through ssh. With a
   fleet map it also polls every VEN's plan during the run and fetches each VEN's
   `GET /history/plans` and `GET /history/forecast-accuracy` for the window, plus the
   arbiter's decisions from `GET /trace/events` ([[deviation-arbiter]]).
2. **`run_batch.py`** — runs a list of scenarios as separate `run_experiment.py`
   subprocesses and **resumes** by default: a scenario with a completed result directory is
   skipped, so re-running the same command after an interruption picks up where it stopped.
   Baseline scenarios get `--no-paired-baseline` automatically.
3. **`kpi.py`** (with `compliance.py`) — per-VEN KPIs from the snapshot, written to
   `kpis.json`; see below.
4. **`report.py`** — a markdown comparison across runs, with import-profile PNGs when
   matplotlib is importable (silently skipped otherwise).

### Run hygiene

- **Paired baseline** (GB-28, on by default): each run first spends one more window with no
  events, immediately before the scenario, so "energy shifted" compares against a baseline
  minutes away rather than a separately run S-1 hours away — which would confound the result
  with real time-of-day drift. It roughly doubles a run's length.
- **Background events** (GB-45/46): events already on the VTN (a seeded demo tariff) are
  saved, deleted for the run and re-created afterwards, because they compete with the
  scenario's own signal; `--restore-background-events` recovers from a run killed before it
  could restore them.
- **Price pre-flight** (GB-46): the run aborts unless every VEN's resolved tariff matches
  the scenario's first price within 180 s (`--skip-preflight` is debugging only), so a run
  whose signal never arrived cannot yield price KPIs.
- **Reports are requested** (GB-27): the first event of each window carries
  `reportDescriptors` for `BASELINE` and `USAGE`, so the report-derived KPIs get data.
- **EV state** (GB-37): `--ev-session-mode` gives every EV in `fleet_ev_roster.json` one
  identical session (the deadline defaults to the scenario end, sessions are soft by
  default), and every roster EV is reset to a known SoC before the run so runs start
  comparable. Only the `BY_DEADLINE`, `ASAP` and `MAX_COST` modes route the real per-slot
  tariff into the EV's reward; the PV-gated ones cannot show price response. Scenarios that
  do not care leave the EVs unsessioned.
- **Personas** (`--personas`): reads `VEN/fleet/manifest.json` and gives each
  persona-tagged VEN its preset EV session and comfort curve before the first action,
  removed in teardown; `kpi.py --manifest` adds a per-persona block.

## KPIs (`kpi.py`)

Framed by stakeholder (the fleet-KPI reframe):

- **Per VEN** — import and export energy, cost (import times tariff minus export times
  tariff, per 1-minute sample), peak import, load factor, and `energy_shifted_kwh` against
  the paired baseline. Samples that straddle a window or price boundary are not scored.
- **Grid** — envelope compliance in each direction, signal-to-response latency, and (GB-46)
  **measured limit compliance**: per hard-limit window (`capacity_limit`,
  `export_capacity_limit`, `alert`) a VEN passes if it reaches the limit, or its physical
  floor when the limit is unreachable, within 5 minutes of the event start and holds it
  after. It is judged from the VEN's own meter and asset traces, never from planner-predicted
  warnings. GB-47 adds the **utilisation** of the allowance (guarded where the allowance is
  near zero) and **comfort** against the baseline.
- **Energy business** — `tariff_response`: the Pearson correlation of import with the
  tariff, plus the cheapest-tercile versus dearest-tercile difference, `None` below five
  distinct prices (which is why `s9` exists).
- **VEN stakeholder** — the money impact of participating, and the plan and forecast
  evidence per VEN: plan-history summary with warning-kind counts, forecast accuracy, plan
  diagnostics and the arbiter decisions inside the window.
- **Report-derived** — `report_lag_s` timeliness and `event_impact_kwh` (archived BASELINE
  minus USAGE energy), both filtered to this run's own event ids because the recorder
  archive holds every report ever seen. Units come from each report's own
  `payloadDescriptors` (GB-50): a report that declares `KWH` is summed as energy, one that
  declares `KW` is multiplied by its duration, and one that declares nothing is read as watts
  (the pre-cutover format) with a printed note; the KPI no longer assumes the wire's unit
  ([[wire-contract]], [[heuristics-pipeline]] for the BASELINE source).
- **Signal integrity** — the recorded tariff is compared with the scenario's price series
  after a short propagation allowance; mismatching VENs are listed in the fleet block.

## Planner solve-cost log

`experiments/planner_benchmarks/` holds an append-only JSON-lines record of planner
solve-cost experiments (parameters, commit, results), deliberately outside the bulk
`experiments/results/` path that is cleaned for disk and apart from the prose conclusions
in `docs/reference/R97_PLANNER_BENCHMARKS.md`: measurements that cannot be regenerated do
not live where a cleanup can take them, and when the prose and the rows disagree the rows
win ([[milp-planner]]).

## Verified

`kpi.py`, `compliance.py` and `run_experiment.py` each carry a `--self-check` that runs
without a stack. The long-lived-containers trap still applies: a production trio running
pre-feature binaries has no history store and no `lab_recorder` schema, so rebuild them when
a phase lands before an experiment ([[deployment-topology]]).
