## 1. Compliance KPIs (`experiments/compliance.py`, new) — self-checks first

- [x] 1.1 Self-checks for `limit_windows(actions)`: alert-inside-capacity-limit strictest target; export window; actions without parameters skipped with a note
- [x] 1.2 Self-checks for `import_floor_kw(minute_assets, profile)`: storage-less base load; heater counted only at/below its own `temp_min_c`; battery discharge only above `min_soc`; PV reduces the floor; never negative
- [x] 1.3 Self-checks for `window_compliance(rows, targets)`: late-but-sustained (360 s, fail), relapse (not sustained), immediate pass, engaged/not-engaged rule, overshoot after grace
- [x] 1.4 Self-checks for `signal_integrity(rows, price_action)` and `fleet_coincident_peak(per_ven_rows)`
- [x] 1.5 Implement `compliance.py` until `python experiments/compliance.py --self-check` passes

## 2. `kpi.py` integration

- [x] 2.1 Load per-VEN profile YAML (`--profiles-dir`, default `VEN/profiles`) and per-minute asset rows from `tick_samples`
- [x] 2.2 Emit per-VEN `compliance` (per window) and `signal_integrity`; top-level `fleet` block (coincident peak, per-window engaged/pass/fail summary); print a pass/fail table
- [x] 2.3 `python experiments/kpi.py --self-check` still passes; run `kpi.py` on the 2026-08-31 S-7 and S-3 and the 2026-09-08 S-9 run dirs as a smoke check (old `run.json` without parameters → windows skipped with a note, no crash)

## 3. Harness (`experiments/run_experiment.py`)

- [x] 3.1 Self-check: `strip_server_fields(event)` and background-event selection (created-by-run excluded)
- [x] 3.2 Background events: snapshot → delete → restore in outermost `finally`; `--keep-background-events`; `--restore-background-events FILE`
- [x] 3.3 Self-check + implement `sleep_until(target, now_fn, sleep_fn)` (chunked, wall-clock); use it for `--start-at`, action offsets and window ends
- [x] 3.4 Action log carries the action's parameters
- [x] 3.5 Pre-flight: self-check `tariff_matches(tariffs, now, price)`; after the first `price_series`, poll every VEN's `/tariffs`, abort with cleanup + restore + non-zero exit on mismatch after 180 s
- [x] 3.6 Plan retention in the plan poller (`{ven}-plans.jsonl` on plan-id change), with a self-check for the change detection
- [x] 3.7 `python experiments/run_experiment.py --self-check` passes

## 4. Scenario + live verification

- [x] 4.1 `s9_diurnal.yaml`: import cap 1.0 kW (18:00–22:00 local), export cap 0.5 kW (11:00–16:00 local); comment carries the calibration source
- [ ] 4.2 Live smoke on Pi4 against the real fleet after GB-44/GB-45 are deployed: a short custom scenario (5 min: price + 1.5 kW cap) — verify demo events removed then restored, pre-flight passes, `run.json` params, `{ven}-plans.jsonl`, `kpi.py` compliance block

## 5. Docs, merge

- [ ] 5.1 `docs/guidelines/FLEET_EXPERIMENT_DESIGN.md`: pass bar, isolation, floor definition, signal integrity, baseline-without-price note
- [ ] 5.2 BACKLOG GB-46: mark the harness/KPI half done, keep the VEN-side items (tariff source id, effective-limit column) open; project journal entry
- [ ] 5.3 Delete this openspec change; rebase, fast-forward merge, push; pull on Pi4 (the harness runs there)
