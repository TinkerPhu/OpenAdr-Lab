---
title: Experiment Harness
type: component
created: 2026-07-11
updated: 2026-07-12
synced_commit: c5a1d03
sources: [experiments/, VEN/src/tasks/sim_tick/tick.rs]
tags: [experiments, kpi, scenarios, phase3]
---

# Experiment Harness

Phase 3's A-3 deliverable (`docs/BACKLOG.md` BL-33): a scripted way to compare the
control methods — price signals, capacity limits/reservations, grid alerts, SIMPLE
levels, direct dispatch — on KPIs, replacing manual comparison. Lives in
`experiments/` (Python, matching the E2E tooling), runs on the docker host like
`fleet.sh` ([[fleet-tooling]]).

## The real-time constraint

The phase plan's sim-time spike came back negative: `tick_once`
(`VEN/src/tasks/sim_tick/tick.rs`) stamps `Utc::now()` and every event window is an
absolute timestamp, so time acceleration is not externally drivable — it would need
an injectable clock through the whole tick/poll path, not just the planner (which
already has one per the determinism rule). Scenarios therefore run in **real time**:
S-1…S-6 are 30-minute same-day windows (~3 h for the full set), and the phase exit
demonstration runs as a deliberately scheduled window — the same deferral rationale
as Phase 2's N=10 fleet test.

## Pipeline

1. **`scenarios/*.yaml`** — declarative: a duration plus actions at relative minute
   offsets (`price_series`, `capacity_limit`, `capacity_reservation`, `alert`,
   `simple`, `dispatch`). S-1 is the flat-tariff baseline every other scenario's
   "energy shifted" KPI is measured against; `smoke.yaml` is a 3-minute harness
   self-test, not an experiment.
2. **`run_experiment.py`** — creates a program, posts each action's event at its
   offset, waits out the window, deletes everything it created (deletion ==
   cancellation, [[openadr-3]]), then snapshots each VEN's `history.sqlite`
   ([[history-store]]) **including the `-wal`/`-shm` sidecars** — the stores are
   WAL-mode and only checkpoint at the daily prune, so copying the main file alone
   captures nothing (found live) — plus the `lab_recorder` tables as CSV.
3. **`kpi.py`** — per-VEN over the run window: import/export energy, cost, peak
   import, load factor, energy shifted vs the S-1 baseline, and report-timeliness
   stats from the recorder's `report_lag_s` column (WP3.7) — windowed by
   `received_at`, since the recorder archive holds every report ever seen.
4. **`report.py`** — markdown comparison across runs; import-profile PNGs when
   matplotlib is importable, silently skipped otherwise (not installed on the Pi4
   host).

## Personas (Phase 4, WP4.5)

`run_experiment.py --personas` reads `VEN/fleet/manifest.json`
([[fleet-tooling]]) and, before the scenario's first action, gives every
persona-tagged fleet VEN its preset EV session (mode/target/departure/budget)
and comfort-curve override; both are removed in the teardown alongside the
event cleanup. Fleet VENs are auto-added to the snapshot set, and
`kpi.py --manifest` appends a per-persona block (mean import/cost/peak/shifted)
so the S-2/S-3/S-4 re-runs show the behavioural spread — or its absence, which
would itself be a finding. The full persona re-run is a scheduled real-time
window like the Phase-3 exit demo.

## Verified

A 3-minute smoke run on Pi4 exercised the full pipeline with real per-VEN KPI
values. That run also exposed an environment trap worth remembering: the
production trio + BFF had been running pre-Phase-1 binaries for four days (no
history store, no `lab_recorder` schema) — long-lived containers silently decouple
from `main`; rebuild them when a phase lands ([[deployment-topology]]).
