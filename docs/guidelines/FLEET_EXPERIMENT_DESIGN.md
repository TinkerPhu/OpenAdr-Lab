# Fleet Experiment Design Guidelines

Guidance for designing and launching `experiments/run_experiment.py` scenarios
against a VEN fleet — specifically, matching how much asset-mix and EV-mode
diversity a scenario carries to what it's actually trying to measure. This is
not one of the four CI-facing suites in `docs/guidelines/TESTING.md`;
fleet experiments are exploratory/analysis runs, not gating tests.

## Two different kinds of scenario intent

A fleet scenario is either measuring the system in aggregate, or trying to
isolate the behavior of one mechanism. These need opposite population designs.

**System/aggregate scenarios** — does the VTN-VEN system behave correctly
under realistic, heterogeneous load: compliance with capacity limits, event
latency, plan robustness, notification correctness. Heterogeneity (mixed
asset types, mixed EV urgency modes) is desirable here — it's what makes a
fleet run more informative than a single-VEN unit test. Report only
fleet-aggregate numbers from these; don't attribute a result to one asset
category.

**Causal/attribution scenarios** — does a *specific* asset type or mechanism
respond correctly to a specific signal (e.g. "does the EV chase cheap tariff
windows"). These need a controlled, not diverse, population: hold every other
variable fixed and vary only the factor under test. Mixing asset types or EV
modes here doesn't add realism, it adds confounds.

## Why this matters for the hand-authored fleet specifically

`VEN/profiles/ven-1..20.yaml` are 20 individually hand-authored households —
asset-mix categories (PV+battery, EV-only, heater-only, EV+heater, …) have
between 1 and 5 members each (ven-14..20, added 2026-08-23, filled some of
the thinnest gaps — battery+heater, residential PV+heater, battery+EV — but
did not turn any category into a large, statistically powered sample). Any
per-category split of a KPI computed on this fleet is statistically thin. The S-9 diurnal run's
`energy_business.tariff_response` is the concrete case that motivated this
document (`docs/history/fleet_run_journal.md`): a per-category read looked
like a real trend at first glance, and turned out to be dominated by two
confounds (one VEN on real-weather ground truth instead of the modeled
curve, and every EV silently inert for lack of a session — see GB-37 in
`docs/BACKLOG.md`) rather than genuine asset-type behavior.

`scripts/personas.py` / `--personas` currently targets a *different*,
independently-generated fleet (`scripts/gen_fleet_profiles.py`, which writes
its own `VEN/fleet/manifest.json` + `VEN/docker-compose.fleet.yml`) — it is
not wired to the hand-authored `ven-1..20` fleet that `experiments/fleet_map.json`
and every `S-1`..`S-10` scenario target. Don't assume `--personas` is usable
against the S-1..S-10 fleet without checking GB-37's status first.

## Rules

1. **State the scenario's intent** (aggregate/system vs. causal/attribution)
   in the scenario YAML's own header comment before choosing an asset-mix or
   EV-mode strategy — the same convention `s9_diurnal.yaml`/`s10_overexport.yaml`
   already use for PV-azimuth timing guidance.
2. **Causal/attribution scenarios**: force every VEN relevant to the factor
   under test onto a single, appropriate mode for the whole run (e.g. every
   EV-bearing VEN on `BY_DEADLINE` for a tariff-response test). Never a mixed
   persona spread, and never a mode whose reward is gated by something other
   than the signal under test (`OPPORTUNISTIC`/`ASAP_FREE` are gated on PV
   surplus, not price — see GB-37).
3. **Aggregate/system scenarios**: mixed asset types and mixed personas are
   fine and preferred. Still flag any per-category breakdown you report as
   underpowered rather than silently presenting it as a trend.
4. **If a question genuinely needs a larger, controlled population** for
   statistical power (e.g. 10+ EV-only VENs to get a real correlation
   estimate), spin up a dedicated homogeneous sub-fleet with
   `scripts/gen_fleet_profiles.py --count N` rather than subdividing the
   fixed 20-VEN fleet's already-thin categories. Check host capacity first
   (`free -h`, `docker stats`, `df -h` on the target host) — Node2 also
   carries build/test offload, so don't assume idle-container memory
   footprint scales linearly under a concurrent build.

## Run isolation and signal verification

- **Background events are suspended for every run.** `run_experiment.py` saves
  every event already on the VTN (the demo events `scripts/seed_vtn.py` seeds,
  including a broadcast day-ahead TOU price) to `<run>/background-events.json`,
  deletes them, and re-creates them afterwards — also on an exception or
  `SIGTERM`. After a hard kill, `run_experiment.py --restore-background-events
  <file>` recovers them. `--keep-background-events` opts out, at the cost of a
  confounded run. Consequence: paired-baseline windows run with no price event
  at all, so the VEN uses its default import price there; the scenario window
  differs from its baseline only by the scenario's own signal.
- **Pre-flight**: right after the first `price_series` action every VEN's
  resolved tariff (`GET /tariffs`) must equal the scenario's price within
  180 s, or the run aborts (events cleaned up, background restored, non-zero
  exit, no `run.json`, so `run_batch.py --resume` retries it).
- **Signal integrity** (`kpi.py`): the recorded `grid_samples` tariff is compared
  minute by minute with the scenario's price series (first 2 minutes of each
  interval skipped for propagation); any mismatch is listed in `kpis.json`
  `fleet.signal_integrity_mismatched_vens`.

## Judging limits: the pass bar

A VEN **passes** a hard-limit window (capacity limit, export capacity limit,
alert = 0 kW import) when it reaches the limit — or its **physical floor** if
the limit is unreachable — within **5 minutes** of the event start and stays
there for the rest of the window (sustained, not first touch). Implemented in
`experiments/compliance.py` (`python3 experiments/compliance.py --self-check`
shows the cases), reported by `kpi.py` per VEN (`compliance`) and per window
(`fleet.compliance`: engaged / passed / failing).

- **Measured, never predicted.** Compliance comes from each VEN's own
  per-minute meter data. `CAPACITY_VIOLATION` plan warnings are the planner's
  forecast and diverge from what happened in both directions; don't score
  from them.
- **Physical floor** per minute = base load + running shiftable loads + PV
  (negative while generating) − available battery discharge, never below 0.
  EVs and heaters count as controllable to 0 — a heater only counts toward
  the floor while at or below its own `temp_min_c` (a genuine thermostat
  emergency), so a thermostat override above it can never be excused. Export
  floor is 0 (PV is curtailable).
- **Engaged**: the window mattered to that VEN (it reached ≥ 80 % of the
  limit, its floor exceeded the limit, or it started above the limit). Pass
  rates are over engaged VENs only; a limit nobody gets near is not a pass,
  it is an uncalibrated scenario — recalibrate it (as `s9_diurnal.yaml`'s
  caps were, from the previous run's per-VEN levels).
- **Peaks**: `raw.peak_import_kw` is one VEN's own peak; the fleet's
  simultaneous peak is `fleet.coincident_peak_import_kw`.

## Related

- GB-37 (`docs/BACKLOG.md`) — closing the `--personas`/manifest gap for the
  hand-authored fleet, and forcing tariff-sensitive EV modes for
  tariff-response scenarios.
- `docs/history/fleet_run_journal.md` — the S-9 diurnal run write-up, the
  case study behind this document.
