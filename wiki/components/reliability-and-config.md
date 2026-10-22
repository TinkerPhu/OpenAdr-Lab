---
title: VEN Reliability & Config Hygiene
type: component
created: 2026-07-04
updated: 2026-10-22
synced_commit: 650b626
sources: [VEN/src/tasks/mod.rs, VEN/src/tasks/backoff.rs, VEN/src/tasks/poll_config.rs, VEN/src/boot/background.rs, VEN/src/boot/mod.rs, VEN/src/state/task_status.rs, VEN/src/routes/system.rs, VEN/src/routes/error.rs, VEN/src/entities/error.rs, VEN/src/profile/, VEN/src/tasks/obligation.rs, VEN/src/vtn.rs, VEN/src/controller/milp_planner/mod.rs, VEN/src/config.rs, lab-core/src/vtn_auth.rs, docs/guidelines/ERROR_HANDLING.md, docs/architecture/ven_milp_planner.md]
tags: [reliability, config, error-handling, ven]
---

# VEN Reliability & Config Hygiene

Four small, related hardening measures close silent-failure modes that a plain `anyhow`+
`tokio::spawn` baseline leaves open: an unsupervised panic in a background task takes down
DR control with no restart, an unvalidated profile can drive wrong physics at runtime, and
undifferentiated errors block domain-specific recovery.

## Task supervision

A single `supervised_spawn(name, cooldown_s, state, make_task)` utility (`VEN/src/tasks/mod.rs`) wraps
every background loop the process owns, and `boot/background.rs` is the one place they are spawned:
`poll_programs`, `poll_events`, `poll_reports`, `sim_tick`, `obligation_check`, `planning`, `state_persist`,
`history_sampler`, `base_load_window` and `heuristics_job` (the last three run only with the history store). On panic
*or* unexpected exit it logs with the task name, waits 5 s (`TASK_COOLDOWN_S`) and re-spawns, so the restart loop
lives in one place; the closure must rebuild the task from scratch, which is why every captured value is cloned
into it. The HTTP server (and `GET /health`) keeps serving throughout the cooldown.

Restarts are observable: each (re)start and completion is recorded on the diagnostics state
(`state/task_status.rs`: last run, last success, restart count), served by `GET /tasks/status`
(only tasks actually spawned in this process appear, so it is not a fixed-length list), and each restart also
lands in the Event Log under category `task_supervisor` (ui-transparency; [[ven-ui]] Diagnostics).

## Typed domain errors

`entities::DomainError` (`thiserror`) is the one vocabulary for failures that cross a layer boundary
(`docs/guidelines/ERROR_HANDLING.md`: translate at the boundary where the technical error occurs, structured
context over flattened strings, terse one-line Display, remediation hints only in the component's own words).
Its variants and where they come from:

| Variant | Raised at |
|---|---|
| `SessionConflict`, `NotFound` | `services/user_request.rs` (a request already in a terminal state, an unknown request id); `NotFound` also from a missing notification in the history store |
| `AssetNotFound`, `InvalidValue` | the roster port (`simulator/handle.rs`) and an asset's own `validate_values` (e.g. a battery refusing a SoC outside 0–1) |
| `VtnRejected { status, message }` | `vtn.rs`: any non-2xx VTN answer, so a caller can decide on the status (a 404 on a report means the source event or program is gone, GB-23; [[openadr-interface]]) |
| `StorageError` | the history store's boundary, every `rusqlite` failure ([[history-store]]) |
| `PlanInfeasible` | `milp_planner::run_planner`'s solve-failure branch — **logged, not propagated**: `SolverPort::solve` stays infallible by design and the failure already produces a fallback `Plan` with a Critical `PlanWarning` ([[milp-planner]]) |
| `VtnUnreachable` | `vtn.rs` classifies a connect- or timeout-class transport failure and logs it for fleet debugging; `VtnPort`'s `Result<T, anyhow::Error>` contract is unchanged |
| `ProfileInvalid` | still unconstructed: profile problems are collected as a list of violations at boot (below), and no work is tracked for it |

The `DomainError → (StatusCode, Json<ErrorBody>)` mapping lives in `routes/error.rs`, because choosing a status
is a presentation decision: `NotFound`/`AssetNotFound` → 404, `InvalidValue` → 400, `SessionConflict` → 409,
everything else 500, with the one body every route answers with, `{"error": "<message>"}` (R-133).
`EvSessionService` was deleted by BL-41 (session start and end moved into the unified `/user-requests`
flow, [[hems-planning]]) and the dead `HvacService` with `services/hems.rs`, so nothing wraps a device session any more.
Variants that still carry a bare `String` (`SessionConflict`, `PlanInfeasible`, `VtnUnreachable`, `StorageError`)
are the part of the guideline's "structured over stringified" rule not yet applied. See [[ven-code-vs-docs-audit]].

## Poll-loop backoff (GB-09 / BL-03)

`tasks/backoff.rs`'s `Backoff` replaced the poll loops' fixed `tokio::time::interval`: on success the
delay resets to the configured base; on failure it doubles (±10% jitter from a seeded RNG, so tests stay
deterministic) up to a cap of 30× the base, never above 900 s. `record_fail_sleep` does the failure, the
bookkeeping (`VtnConnectionStatus`: connected, last success, last error, `current_backoff_s`, shown on the
Dashboard) and an Event Log entry (`vtn_connection`), then the sleep. A stuck-down VTN no longer gets hammered at
the configured cadence indefinitely. One trade-off worth knowing: after a *sustained* outage, recovery latency is
bounded by whatever backoff delay was already in flight when the VTN comes back — the reset to the base interval
only takes effect on the next successful poll, not instantly on recovery ([[ven-hexagonal-architecture]] for
where this sits relative to `VtnPort`).

## Profile startup validation

`boot/mod.rs` loads the profile (`PROFILE_PATH`; absent means `Profile::default()` with a warning), then
`Profile::validate()` (`profile/validate.rs`) runs before any task spawns. It checks the planner (step, horizon,
zones, replan interval, phase-2 timeout, MIP gap), polling cadences, comms-loss and heuristics settings, and each asset's
parameters. All violations are collected and printed together as `profile error: …` lines, and the process exits
non-zero before touching the [[simulator]] or planner state.

## Config knobs, not magic numbers

The environment carries only what differs per deployment (`Config::from_env`, `config.rs`): `LISTEN_ADDR`,
`VTN_BASE_URL`, `CLIENT_ID`, `CLIENT_SECRET`, `VEN_NAME`, `PERSIST_PATH`, `PROFILE_PATH`. Everything else is the
profile, one per VEN instance.

| Value | Form |
|---|---|
| HiGHS time limits | `planner.solver_timeout_s` (default 60) and `planner.phase2_solver_timeout_s` — see [[milp-planner]] |
| Solver quality | `planner.mip_gap_target` (default 0.02); the achieved gap is not observable (R-65) |
| Planning loop startup delay | `planner.planning_initial_delay_s` (default 5) |
| Replan interval | `planner.replan_interval_s` (default 300) |
| Peak-demand penalty rules | `planner.penalty_rules` (default `[]` = off) — see [[milp-planner]] |
| Real-measurement feed enablement | the measured asset's own `measurement.enabled` (default `false`) — second gate alongside the `{PV,BASE_LOAD}_MEASUREMENT_MQTT_HOST` env vars, see [[real-measurement-mqtt]] |
| Poll cadence | `polling.events_secs` / `programs_secs` / `reports_secs` (30/30/60); the `POLL_*_SECS` env vars exist only as test overrides (`tasks/poll_config.rs`) |
| Poll-loop startup jitter | `polling.startup_jitter_fixed_pct` + `startup_jitter_random_max_pct`, as a share of `events_secs` (GB-09): a one-time pre-loop delay so a fleet brought up together does not poll in lockstep; see [[fleet-tooling]] |
| History retention | `history.retention_days` (default 90), see [[history-store]] |
| Obligation check interval | `OBLIGATION_CHECK_INTERVAL_S` = 5, named constant, `tasks/obligation.rs` |
| OAuth token expiry margin | `TOKEN_EXPIRY_MARGIN_S` = 60, named constant, `lab-core/src/vtn_auth.rs` (shared with the BFF, [[wire-contract]]) |

The `POST /sim/inject` master switch (`simulator.sim_inject_enabled`) no longer exists; see [[simulator]].

Counter-examples that still deserve promotion to config: the flexibility-envelope constants
(`max_acceptable_rate_eur_kwh: 0.35`, `min_acceptable_rate_eur_kwh: 0.05`) hardcoded in
`milp_planner/session_flexibility.rs` — see [[ven-code-vs-docs-audit]].
