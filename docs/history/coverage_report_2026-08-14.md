# VEN Rust Coverage Report

2026-08-14: first `cargo-tarpaulin` run on Node2 via `run_all_tests.sh --coverage`
(GB-30 in `docs/BACKLOG.md`). **2026-08-15: re-run** after adding targeted unit
tests for `simulator/persist.rs` and `simulator/plan_context.rs`
(`fix/sim-persist-plan-context-tests`, merged to `main`) — numbers below are that
second run. Point-in-time snapshot, not living documentation — regenerate rather
than hand-edit when it goes stale.

## Scope

Line coverage of the VEN Rust crate's own test suite (`cargo test --workspace`
under `cargo-tarpaulin` instrumentation) — the full 4-layer test pyramid
(domain / use-case / adapter-contract / integration, see
`docs/guidelines/TESTING.md`).

Does not include: VEN/VTN UI unit tests (vitest, separate JS coverage tooling),
E2E BDD (behave) or resilience suites (exercise the compiled binary externally
over HTTP/MQTT, not tracked by tarpaulin's ptrace instrumentation), or the
`openleadr-rs` submodule (upstream project, own CI/codecov).

## Headline

**67.80% — 5593/8249 lines covered** (up from 66.93%, 5431/8114 in the first run)

## What changed since the first run

| File | Before | After | Note |
|---|---|---|---|
| `src/simulator/persist.rs` | 0/33 (0%) | 27/33 (81.8%) | sim-state save/load/fallback — the exact mechanism behind the ven-1 SIGTERM-persistence incident, previously untested |
| `src/simulator/plan_context.rs` | 2/39 (5.1%) | 31/39 (79.5%) | `build_asset_contexts` MILP-context assembly — only the trivial no-PV no-op path was tested before |

## Coverage by module

| Module | Covered/Total | % |
|---|---|---|
| `src/routes/` | 204/988 | 20.6% |
| `src/(top-level)/` (bare `src/*.rs`) | 122/493 | 24.7% |
| `src/tasks/` | 561/1049 | 53.5% |
| `src/assets/` | 886/1199 | 73.9% |
| `src/services/` | 393/520 | 75.6% |
| `src/simulator/` | 293/371 | 79.0% |
| `src/state/` | 232/285 | 81.4% |
| `src/entities/` | 244/291 | 83.8% |
| `src/profile/` | 308/366 | 84.2% |
| `src/history_store/` | 353/415 | 85.1% |
| `src/controller/` | 1886/2148 | 87.8% |
| `src/common/` | 111/124 | 89.5% |

Reading the shape: `routes/` and `tasks/` sit lowest not because they're
untested but because they're Axum handlers and long-running loops exercised by
the E2E BDD/resilience suites against the live binary (out of this report's
scope, see Scope above), not by `cargo test`. `controller/` (MILP planner,
dispatcher, arbiter) and `entities/`/`profile`/`state` — the domain-heavy inner
rings per `docs/architecture/VEN_ARCHITECTURE.md`'s hexagonal rule — carry the
highest coverage, which is where unit-test rigor matters most per this
project's own testing philosophy (`.claude/CLAUDE.md`: "no enforced coverage
floor — keep domain and application layer tests meaningful").

## Zero-coverage files (unit-test scope only)

22 files, 996 lines. All either (a) wiring/entrypoints exercised only at the
E2E/BDD layer, or (b) task loops whose bodies are integration-shaped by
design (see `determinism` rule, `.claude/CLAUDE.md`) and are covered by their
own BDD scenarios instead of unit tests:

| File | Lines |
|---|---|
| `src/main.rs` | 165 |
| `src/routes/hems/sessions.rs` | 151 |
| `src/tasks/planning/cycle.rs` | 109 |
| `src/routes/mod.rs` | 70 |
| `src/routes/hems/misc.rs` | 60 |
| `src/routes/assets.rs` | 54 |
| `src/tasks/poll_events/mod.rs` | 54 |
| `src/routes/trace.rs` | 42 |
| `src/domain_params.rs` | 40 |
| `src/tasks/sim_tick/mod.rs` | 37 |
| `src/routes/hems/comfort.rs` | 32 |
| `src/tasks/obligation.rs` | 31 |
| `src/tasks/state_persist.rs` | 25 |
| `src/routes/events.rs` | 19 |
| `src/routes/hems/baseline_override.rs` | 16 |
| `src/routes/event_log.rs` | 15 |
| `src/tasks/poll_programs.rs` | 15 |
| `src/tasks/progress_ticker.rs` | 15 |
| `src/tasks/poll_reports.rs` | 14 |
| `src/routes/hems/ev.rs` | 12 |
| `src/config.rs` | 10 |
| `src/routes/hems/arbiter.rs` | 10 |

## Full per-file table

<details>
<summary>187 files, sorted by coverage % ascending</summary>

| File | Covered/Total | % |
|---|---|---|
| `src/config.rs` | 0/10 | 0.0% |
| `src/controller/milp_planner/tests/basic.rs` | 0/0 | 0.0% |
| `src/controller/milp_planner/tests/cost_sign.rs` | 0/0 | 0.0% |
| `src/controller/milp_planner/tests/heater.rs` | 0/0 | 0.0% |
| `src/controller/milp_planner/tests/mod.rs` | 0/0 | 0.0% |
| `src/controller/milp_planner/tests/modes.rs` | 0/0 | 0.0% |
| `src/controller/milp_planner/tests/penalty.rs` | 0/0 | 0.0% |
| `src/controller/milp_planner/tests/planner.rs` | 0/0 | 0.0% |
| `src/controller/milp_planner/tests/pv.rs` | 0/0 | 0.0% |
| `src/controller/milp_planner/tests/solver.rs` | 0/0 | 0.0% |
| `src/controller/milp_planner/tests/stale_rates.rs` | 0/0 | 0.0% |
| `src/controller/mod.rs` | 0/0 | 0.0% |
| `src/controller/settings_port.rs` | 0/0 | 0.0% |
| `src/controller/solver_port.rs` | 0/0 | 0.0% |
| `src/controller/tests/arbiter_tests.rs` | 0/0 | 0.0% |
| `src/controller/vtn_port.rs` | 0/0 | 0.0% |
| `src/domain_params.rs` | 0/40 | 0.0% |
| `src/entities/error.rs` | 0/0 | 0.0% |
| `src/entities/mod.rs` | 0/0 | 0.0% |
| `src/entities/user_request.rs` | 0/0 | 0.0% |
| `src/history_store/schema.rs` | 0/0 | 0.0% |
| `src/ids.rs` | 0/0 | 0.0% |
| `src/main.rs` | 0/165 | 0.0% |
| `src/planner_events.rs` | 0/0 | 0.0% |
| `src/profile/mod.rs` | 0/0 | 0.0% |
| `src/routes/assets.rs` | 0/54 | 0.0% |
| `src/routes/event_log.rs` | 0/15 | 0.0% |
| `src/routes/events.rs` | 0/19 | 0.0% |
| `src/routes/hems/arbiter.rs` | 0/10 | 0.0% |
| `src/routes/hems/baseline_override.rs` | 0/16 | 0.0% |
| `src/routes/hems/comfort.rs` | 0/32 | 0.0% |
| `src/routes/hems/ev.rs` | 0/12 | 0.0% |
| `src/routes/hems/misc.rs` | 0/60 | 0.0% |
| `src/routes/hems/mod.rs` | 0/0 | 0.0% |
| `src/routes/hems/sessions.rs` | 0/151 | 0.0% |
| `src/routes/mod.rs` | 0/70 | 0.0% |
| `src/routes/trace.rs` | 0/42 | 0.0% |
| `src/services/mod.rs` | 0/0 | 0.0% |
| `src/services/test_support/milp_mocks.rs` | 0/0 | 0.0% |
| `src/services/test_support/mock_history_port.rs` | 0/0 | 0.0% |
| `src/services/test_support/mock_simulator_port.rs` | 0/0 | 0.0% |
| `src/services/test_support/mock_solver_port.rs` | 0/0 | 0.0% |
| `src/services/test_support/mock_vtn.rs` | 0/0 | 0.0% |
| `src/services/test_support/mock_weather_port.rs` | 0/0 | 0.0% |
| `src/services/test_support/mod.rs` | 0/0 | 0.0% |
| `src/simulator/tests/base_load_noise_tests.rs` | 0/0 | 0.0% |
| `src/simulator/tests/peek_base_load_kw_tests.rs` | 0/0 | 0.0% |
| `src/simulator/tests/peek_pv_kw_tests.rs` | 0/0 | 0.0% |
| `src/tasks/obligation.rs` | 0/31 | 0.0% |
| `src/tasks/planning/cycle.rs` | 0/109 | 0.0% |
| `src/tasks/poll_events/mod.rs` | 0/54 | 0.0% |
| `src/tasks/poll_programs.rs` | 0/15 | 0.0% |
| `src/tasks/poll_reports.rs` | 0/14 | 0.0% |
| `src/tasks/progress_ticker.rs` | 0/15 | 0.0% |
| `src/tasks/sim_tick/mod.rs` | 0/37 | 0.0% |
| `src/tasks/sim_tick/tick_tests.rs` | 0/0 | 0.0% |
| `src/tasks/state_persist.rs` | 0/25 | 0.0% |
| `tests/architecture.rs` | 0/0 | 0.0% |
| `src/tasks/planning/mod.rs` | 2/36 | 5.6% |
| `src/routes/reports.rs` | 8/65 | 12.3% |
| `src/entities/planner_params.rs` | 4/32 | 12.5% |
| `src/controller/history_port.rs` | 4/28 | 14.3% |
| `src/routes/sim.rs` | 22/98 | 22.4% |
| `src/assets/asset_trait.rs` | 10/44 | 22.7% |
| `src/routes/hems/history.rs` | 15/66 | 22.7% |
| `src/simulator/snapshot.rs` | 17/69 | 24.6% |
| `src/measurement.rs` | 10/40 | 25.0% |
| `src/services/hems.rs` | 4/15 | 26.7% |
| `src/weather.rs` | 25/88 | 28.4% |
| `src/assets/mod.rs` | 43/104 | 41.3% |
| `src/state/grid_signals.rs` | 10/24 | 41.7% |
| `src/routes/notifications.rs` | 18/41 | 43.9% |
| `src/controller/trace.rs` | 9/20 | 45.0% |
| `src/routes/timeline.rs` | 45/99 | 45.5% |
| `src/tasks/history_sampler/mod.rs` | 38/83 | 45.8% |
| `src/entities/plan.rs` | 4/8 | 50.0% |
| `src/tasks/sim_tick/publish.rs` | 39/76 | 51.3% |
| `src/controller/user_request.rs` | 17/32 | 53.1% |
| `src/vtn.rs` | 74/137 | 54.0% |
| `src/services/forecast.rs` | 80/127 | 63.0% |
| `src/assets/ev.rs` | 77/122 | 63.1% |
| `src/routes/debug.rs` | 38/59 | 64.4% |
| `src/tasks/sim_tick/arbiter_glue.rs` | 40/62 | 64.5% |
| `src/assets/heater.rs` | 123/187 | 65.8% |
| `src/routes/measurement.rs` | 16/24 | 66.7% |
| `src/state/arbiter.rs` | 18/27 | 66.7% |
| `src/assets/battery.rs` | 59/87 | 67.8% |
| `src/tasks/heuristics_job/mod.rs` | 24/34 | 70.6% |
| `src/controller/milp_planner/types.rs` | 29/41 | 70.7% |
| `src/services/notify.rs` | 50/68 | 73.5% |
| `src/assets/history.rs` | 29/39 | 74.4% |
| `src/controller/arbiter.rs` | 58/78 | 74.4% |
| `src/routes/system.rs` | 32/43 | 74.4% |
| `src/history_store/plan_history.rs` | 59/79 | 74.7% |
| `src/entities/notification.rs` | 6/8 | 75.0% |
| `src/controller/milp_planner/solver_phase1.rs` | 137/180 | 76.1% |
| `src/profile/schema.rs` | 43/56 | 76.8% |
| `src/tasks/poll_signals.rs` | 50/65 | 76.9% |
| `src/tasks/sim_tick/post_lock.rs` | 24/31 | 77.4% |
| `src/assets/pv.rs` | 107/138 | 77.5% |
| `src/controller/milp_planner/mod.rs` | 73/94 | 77.7% |
| `src/simulator/energy.rs` | 7/9 | 77.8% |
| `src/assets/base_load.rs` | 71/91 | 78.0% |
| `src/simulator/plan_context.rs` | 31/39 | 79.5% |
| `src/history_store/settings.rs` | 20/25 | 80.0% |
| `src/tasks/mod.rs` | 21/26 | 80.8% |
| `src/history_store/forecast_accuracy.rs` | 43/53 | 81.1% |
| `src/services/planning.rs` | 108/133 | 81.2% |
| `src/controller/openadr_interface.rs` | 105/129 | 81.4% |
| `src/services/comfort.rs` | 44/54 | 81.5% |
| `src/simulator/persist.rs` | 27/33 | 81.8% |
| `src/profile/defaults.rs` | 159/192 | 82.8% |
| `src/controller/arbiter/arbiter_levers.rs` | 82/99 | 82.8% |
| `src/services/user_request.rs` | 29/35 | 82.9% |
| `src/entities/asset.rs` | 5/6 | 83.3% |
| `src/entities/asset_params.rs` | 25/30 | 83.3% |
| `src/routes/weather.rs` | 10/12 | 83.3% |
| `src/services/obligation.rs` | 30/36 | 83.3% |
| `src/state/mod.rs` | 147/173 | 85.0% |
| `src/tasks/backoff.rs` | 20/23 | 87.0% |
| `src/state/obligations.rs` | 22/25 | 88.0% |
| `src/history_store/mod.rs` | 147/166 | 88.6% |
| `src/entities/tariff_snapshot.rs` | 31/35 | 88.6% |
| `src/controller/milp_interactions.rs` | 55/62 | 88.7% |
| `src/assets/heater_milp.rs` | 113/127 | 89.0% |
| `src/profile/validate.rs` | 98/110 | 89.1% |
| `src/controller/reporter.rs` | 134/150 | 89.3% |
| `src/common/mod.rs` | 111/124 | 89.5% |
| `src/simulator/mod.rs` | 88/98 | 89.8% |
| `src/history_store/ticks.rs` | 36/40 | 90.0% |
| `src/state/event_log.rs` | 9/10 | 90.0% |
| `src/controller/dispatcher.rs` | 80/88 | 90.9% |
| `src/tasks/sim_tick/helpers.rs` | 55/60 | 91.7% |
| `src/history_store/notifications.rs` | 48/52 | 92.3% |
| `src/services/heuristics.rs` | 48/52 | 92.3% |
| `src/assets/grid.rs` | 25/27 | 92.6% |
| `src/controller/milp_planner/inputs.rs` | 154/166 | 92.8% |
| `src/entities/ring_buffer.rs` | 15/16 | 93.8% |
| `src/controller/timeline.rs` | 106/113 | 93.8% |
| `src/controller/milp_planner/stale_rates.rs` | 32/34 | 94.1% |
| `src/tasks/history_sampler/accumulator.rs` | 49/52 | 94.2% |
| `src/tasks/sim_tick/dispatch_override.rs` | 17/18 | 94.4% |
| `src/controller/milp_planner/results.rs` | 155/164 | 94.5% |
| `src/controller/rate_schedule.rs` | 62/65 | 95.4% |
| `src/controller/milp_planner/solver_phase2.rs` | 170/178 | 95.5% |
| `src/tasks/poll_events/detect.rs` | 32/33 | 97.0% |
| `src/controller/report_intervals.rs` | 82/84 | 97.6% |
| `src/entities/solar.rs` | 92/94 | 97.9% |
| `src/assets/ev_milp.rs` | 155/158 | 98.1% |
| `src/assets/battery_milp.rs` | 74/75 | 98.7% |
| `src/controller/milp_planner/envelopes.rs` | 84/85 | 98.8% |
| `src/controller/envelope.rs` | 13/13 | 100.0% |
| `src/controller/measurement_port.rs` | 3/3 | 100.0% |
| `src/controller/milp_planner/asset_port.rs` | 15/15 | 100.0% |
| `src/controller/milp_planner/penalty.rs` | 27/27 | 100.0% |
| `src/controller/milp_planner/solver_duals.rs` | 168/168 | 100.0% |
| `src/controller/monitor.rs` | 20/20 | 100.0% |
| `src/controller/residual.rs` | 7/7 | 100.0% |
| `src/controller/simulator_port.rs` | 2/2 | 100.0% |
| `src/controller/weather_port.rs` | 3/3 | 100.0% |
| `src/entities/arbiter_residual.rs` | 4/4 | 100.0% |
| `src/entities/asset_ledger.rs` | 3/3 | 100.0% |
| `src/entities/capacity.rs` | 8/8 | 100.0% |
| `src/entities/design_vocabulary.rs` | 10/10 | 100.0% |
| `src/entities/device_session.rs` | 2/2 | 100.0% |
| `src/entities/history.rs` | 9/9 | 100.0% |
| `src/entities/measurement.rs` | 3/3 | 100.0% |
| `src/entities/pv_snow.rs` | 10/10 | 100.0% |
| `src/entities/report_submission.rs` | 5/5 | 100.0% |
| `src/entities/sim_inject.rs` | 1/1 | 100.0% |
| `src/entities/timeline.rs` | 5/5 | 100.0% |
| `src/entities/weather.rs` | 2/2 | 100.0% |
| `src/measurement_translation.rs` | 9/9 | 100.0% |
| `src/models.rs` | 4/4 | 100.0% |
| `src/profile/weather_pv.rs` | 8/8 | 100.0% |
| `src/simulator/base_load_preview.rs` | 11/11 | 100.0% |
| `src/simulator/grid_meter.rs` | 12/12 | 100.0% |
| `src/simulator/power_model.rs` | 2/2 | 100.0% |
| `src/simulator/pv_preview.rs` | 13/13 | 100.0% |
| `src/simulator/pv_smoothing.rs` | 7/7 | 100.0% |
| `src/simulator/tests.rs` | 78/78 | 100.0% |
| `src/state/connection.rs` | 11/11 | 100.0% |
| `src/state/heuristics.rs` | 4/4 | 100.0% |
| `src/state/report_submissions.rs` | 5/5 | 100.0% |
| `src/state/task_status.rs` | 6/6 | 100.0% |
| `src/tasks/sim_tick/context.rs` | 23/23 | 100.0% |
| `src/tasks/sim_tick/tick.rs` | 127/127 | 100.0% |

</details>

## Regenerating

```
LOCK_HOST=Node2 bash scripts/docker_host_lock.sh acquire -m "<branch>: coverage refresh" -l 60
bash run_all_tests.sh --coverage   # DOCKER_HOST=Node2, opt-in, ~20-50 min depending on cache state
LOCK_HOST=Node2 bash scripts/docker_host_lock.sh release
```

Report also lands as HTML + JSON at `tests/coverage/ven/` on the docker host
(gitignored, host-local) — this file is the consolidated, committed summary.
