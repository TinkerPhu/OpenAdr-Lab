## Context

`experiments/run_experiment.py` drives one scenario (optional paired baseline + scenario window)
against the live VTN and snapshots each VEN's `history.sqlite`; `run_batch.py` sequences
scenarios; `kpi.py` turns a run dir into `kpis.json`. `kpi.py` already has measured per-VEN
envelope compliance (`grid_envelope_compliance`, from `grid_samples.import/export_limit_kw`) and
a 20%-drop latency heuristic, but no physical-floor normalisation, no alert target (alerts don't
set `import_limit_kw`), no pass bar, and nothing checks that the scenario's price was applied.
The VTN carries demo events seeded once by `scripts/seed_vtn.py` (not republished by any job).
Agreed pass bar (user decision 2026-09-12): each VEN reaches the limit, or its physical floor if
the limit is unreachable, within 5 minutes of the event start, and holds it.

## Goals / Non-Goals

**Goals:** trustworthy verdicts for the fleet re-run — a signal that provably reached the VENs,
compliance measured at the meter against what each VEN could physically do, and data retained
to explain failures.

**Non-Goals:** VEN-side instrumentation (tariff source event id in `grid_samples`, an effective
limit column including alerts) — deferred, the harness can derive both for experiments;
injectable simulation time; per-VEN targeted caps.

## Decisions

**D1 — Background events: snapshot → delete → restore, per `run_experiment.py` invocation.**
Before the first window, `GET /events`, keep every event not created by this invocation, write
them to `{run_dir}/background-events.json`, then delete them. Restore in the outermost `finally`
by re-POSTing each body stripped of server-assigned fields (`id`, `createdDateTime`,
`modificationDateTime`, `objectType`). A standalone `--restore-background-events FILE` re-POSTs a
saved snapshot for manual recovery after a hard kill. *Alternative:* suspend once per batch in
`run_batch.py` — fewer toggles, but a crash of the batch driver would strand the fleet without
its demo content and the single-scenario path would stay unprotected. Per-invocation keeps every
run self-contained. Programs are left alone (events reference them by id).

**D2 — Pre-flight on the resolved tariff, not the recorded one.** After posting the first
`price_series` action, poll each VEN's `GET /tariffs` until the snapshot covering `now` carries
the scenario's first price (±1e-6), timeout 180 s (poll interval + VTN propagation). Any VEN
failing → abort: clean up created events/program/requests, restore background events, exit
non-zero with the offending VENs listed. Uses `/tariffs` because it is what the VEN resolved
(GB-45's single schedule), available immediately, whereas `grid_samples` lags by a minute.

**D3 — Effective limit windows come from the action log.** `run.json["actions"]` entries become
`{"at_minute", "type", "started_at", **action_params}`. `compliance.py` builds windows:
`capacity_limit` → import ≤ `import_kw`; `export_capacity_limit` → export ≤ `export_kw`; `alert`
→ import ≤ 0. Overlapping windows combine to the strictest target per direction and minute.
Reservations and dispatch setpoints are not hard limits and are excluded (reported separately
as-is).

**D4 — Physical floor from the VEN's own tick data and profile.** Per minute, import floor =
max(0, base_load + shiftable-load power + heater power *only while the heater is at or below
its own `temp_min_c`* (a genuine emergency) + PV power (negative when generating) − available
battery discharge (`max_discharge_kw` while SoC > `min_soc`, else 0)). EV and heater (outside a
genuine emergency) are fully controllable to 0. Export floor = 0 (PV is curtailable). The
effective target per minute = max(limit, floor). *Why this floor:* it is the least a VEN could
import if every controllable asset did the right thing; anything above it is avoidable. The
heater rule is deliberately conservative (latched hysteresis above `temp_min_c` counts as
controllable), so it cannot excuse GB-44-style overrides.

**D5 — Compliance metrics.** For each VEN × window: `time_to_comply_s` = first minute from which
actual ≤ target + 0.05 kW holds for every remaining minute of the window (sustained, not
first-touch), `None` if never; `pass` = `time_to_comply_s` ≤ 300; `engaged` = the VEN's actual
in the window reached ≥ 80% of the limit, or its floor exceeded the limit, or it was above the
limit at window start (a limit that never mattered to a VEN is reported but excluded from the
fleet pass rate); plus overshoot kWh after the 5-minute grace. Fleet summary: engaged count,
pass count, fail list.

**D6 — Signal integrity** compares `grid_samples.import_tariff_eur_kwh` per minute with the value
the scenario's `price_series` defines for that minute (interval index from the action's
`started_at`), reporting mismatched minutes. Complements D2 (pre-flight = first interval, live;
integrity = whole window, post hoc).

**D7 — Plan retention**: in the existing 60 s plan poller, when `plan.id` differs from the last
seen id, append `{id, created_at, trigger, solve_status, slots: [{start, end, allocations:
[{asset_id, power_kw}]}]}` to `{ven}-plans.jsonl`. Only on change, so volume ≈ number of adopted
plans.

**D8 — Suspend-safe waits**: one `sleep_until(target)` helper sleeping `min(60 s, remaining)`
in a loop against the wall clock, used for `--start-at`, action offsets and window ends.

## Risks / Trade-offs

- [A hard kill between delete and restore strands the VTN without demo events] → snapshot is
  written to disk *before* deletion; `--restore-background-events` recovers; the run log prints
  the exact recovery command.
- [Restored demo events get new ids and a new `createdDateTime`] → acceptable (user decision);
  under GB-45's tie-break a restored equal-priority event now outranks older ones — irrelevant
  while no scenario runs.
- [Floor model is approximate (1-minute resolution, battery SoC margin)] → documented; errs
  toward counting a VEN as able to do more, never less, except the explicit heater rule.
- [Baseline windows now run with no price event at all] → the VEN falls back to its default
  import price; baseline and scenario differ only in the scenario's own signal, which is the
  point. Documented in FLEET_EXPERIMENT_DESIGN.md.

## Migration Plan

Tooling only. Old run dirs stay readable: `compliance.py` skips windows whose action entries lack
parameters (pre-change `run.json`) with a note, rather than failing.

## Open Questions

None blocking.
