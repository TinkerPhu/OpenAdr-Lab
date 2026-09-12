## Why

The 2026-08-31/09-08 fleet campaign's first-pass verdicts were wrong for three of ten scenarios
because the tooling judged compliance from planner-predicted `CAPACITY_VIOLATION` warnings and
fleet-aggregate peaks, ran with broadcast demo events silently overriding the scenario price, and
used S-9 caps that never came within 2.5 kW of binding (GB-46, `docs/BACKLOG.md`). Before the
fleet re-run that is meant to establish working fleet demand response, the harness must guarantee
that the scenario's signal actually reached every VEN and must report compliance the way the
agreed pass bar defines it: each VEN reaches the limit — or its physical floor, if the limit is
unreachable — within 5 minutes of the event start and holds it.

## What Changes

- **Run isolation**: `run_experiment.py` snapshots every event already on the VTN that the
  harness didn't create, deletes them for the run, and restores them afterwards (also after a
  crash, from the saved snapshot); `--keep-background-events` opts out.
- **Signal pre-flight**: after the first `price_series` action, every VEN's resolved tariff must
  equal the scenario's price within a timeout, or the run aborts (with cleanup) instead of
  producing invalid data.
- **Action log with parameters**: `run.json["actions"]` records each action's parameters
  (limits, durations, levels, setpoints), so KPIs derive effective limits from the log.
- **Plan retention**: the plan poller writes each newly adopted plan's per-slot allocations to
  `{ven}-plans.jsonl`, so plan-vs-actual can be analysed after the fact (GB-41, S-7 latency).
- **Suspend-safe waits**: long waits sleep in short wall-clock-checked chunks.
- **Measured compliance KPI** (new `experiments/compliance.py`, reported by `kpi.py`): per VEN
  and per limit window (import capacity limit, export capacity limit, alert = 0 kW import), the
  effective target is the limit or the VEN's physical floor, whichever is higher; report
  time-to-comply, sustained compliance, `engaged` (the limit mattered), and pass/fail against
  the 5-minute bar; plus a fleet summary.
- **Signal integrity KPI**: recorded `grid_samples` tariff compared minute by minute with the
  scenario's price series.
- **Fleet coincident peak**: `kpi.py` reports the fleet's summed per-minute peak alongside
  per-VEN peaks, so the two are no longer conflated.
- **S-9 recalibration**: evening import cap 4.0 → 1.0 kW, midday export cap 6.0 → 0.5 kW, chosen
  from the 2026-09-08/09 run's own per-VEN levels so the caps engage.

## Capabilities

### New Capabilities
- `fleet-experiment-harness`: run isolation (background events), signal pre-flight, action
  parameter log, plan retention, suspend-safe waits.
- `fleet-compliance-kpis`: measured per-VEN limit compliance against the physical floor with the
  5-minute pass bar, signal integrity, fleet coincident peak.

### Modified Capabilities
<!-- none: no openspec/specs/ exist -->

## Impact

- Code: `experiments/run_experiment.py`, `experiments/kpi.py`, new `experiments/compliance.py`,
  `experiments/scenarios/s9_diurnal.yaml`. No VEN/VTN code changes.
- Data: new `background-events.json`, `{ven}-plans.jsonl` in run dirs; `run.json["actions"]`
  gains fields (additive); `kpis.json` gains `compliance`, `signal_integrity`, `fleet` blocks
  (additive).
- Docs: `docs/guidelines/FLEET_EXPERIMENT_DESIGN.md` (pass bar, isolation, floor definition),
  BACKLOG GB-46.
- Operational: the VTN's demo events disappear for the duration of each run and come back with
  new IDs.
