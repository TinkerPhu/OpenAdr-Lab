---
title: VEN Reliability & Config Hygiene
type: component
created: 2026-07-04
updated: 2026-08-09
synced_commit: 4c4f149
sources: [VEN/src/tasks/mod.rs, VEN/src/tasks/backoff.rs, VEN/src/entities/error.rs, VEN/src/profile/, VEN/src/tasks/obligation.rs, VEN/src/vtn.rs, VEN/src/controller/milp_planner/mod.rs, VEN/src/services/hems.rs, VEN/src/config.rs, docs/guidelines/ERROR_HANDLING.md, docs/architecture/ven_milp_planner.md]
tags: [reliability, config, error-handling, ven]
---

# VEN Reliability & Config Hygiene

Four small, related hardening measures close silent-failure modes that a plain `anyhow`+
`tokio::spawn` baseline leaves open: an unsupervised panic in a background task takes down
DR control with no restart, an unvalidated profile can drive wrong physics at runtime, and
undifferentiated errors block domain-specific recovery.

## Task supervision

A single `supervised_spawn(name, cooldown_s, f)` utility (`VEN/src/tasks/mod.rs`) wraps all
seven background task spawns (`sim_tick`, `planning`, `poll_events`, `poll_programs`,
`poll_reports`, `obligation_check`, `state_persist`). On panic *or* unexpected exit: log
with task name, wait 5 s, re-spawn — the restart loop lives in one place. The HTTP server
(and `GET /health`) keeps serving throughout the cooldown.

## Typed domain errors

`entities::DomainError` (`thiserror`-based) defines `SessionConflict`, `NotFound`,
`PlanInfeasible`, `VtnUnreachable`, `ProfileInvalid`. Route/service code maps
`SessionConflict` → 409 and `NotFound` → 404 (`services/hems.rs`); the other two are
now constructed too (Phase 2, WP2.3), but as logged classifications rather than
propagated route errors — see below. `ProfileInvalid` remains unconstructed.

`PlanInfeasible` is built and logged in `milp_planner::run_planner`'s existing
solve-failure branch (`controller/milp_planner/mod.rs`) — deliberately **not**
returned as an error: `SolverPort::solve` stays infallible by design (its own doc
comment: "implementations must return a usable `Plan` even on internal solver
failure"), so the failure path already produces a fallback `Plan` carrying a
Critical `PlanWarning`; the typed variant now also reaches the logs at that same
point. `VtnUnreachable` is classified in `vtn.rs` from a connect/timeout-class
`reqwest::Error` at every `send()` call site (`classify_reqwest_error`), logged for
fleet debugging — `VtnPort`'s `Result<T, anyhow::Error>` contract is unchanged.

> **RESOLVED DRIFT** (the original domain-errors proposal motivated the type with
> "infeasibility can retain the last valid plan instead of being handled like a
> transport failure", but neither variant was ever constructed). Investigation for
> WP2.3 found the spec's "surfaced through the relevant route" framing didn't fit the
> actual architecture — there was never a route-level error to replace for either
> variant, only existing logging/fallback paths to extend. BL-25 documented this scope
> correction; the entry was retired from `docs/BACKLOG.md` on 2026-07-17 (user decision:
> not needed). `ProfileInvalid` simply stays reserved in `entities/error.rs` — no work
> is tracked for it. See [[ven-code-vs-docs-audit]].

A sibling case, now resolved differently than the two sat side-by-side originally: `EvSessionService` (the type `SessionConflict`/`NotFound` above actually came from) was deleted
outright by BL-41 — session start/end moved entirely into the unified `/user-requests` flow
(`state.cancel_request`, see [[hems-planning]]), leaving nothing for it to wrap. `HvacService`
(`services/hems.rs`) is the one piece kept: its `set_heater_target`/`clear_heater_target`
sketch the same session-lifecycle shape `EvSessionService` used to, still unwired to any
route (`#[allow(dead_code)]`), still tracked by `docs/BACKLOG.md` BL-23. The
`DomainError → (StatusCode, Json)` mapping that used to live beside both is now used only by
`routes/hems/sessions.rs::delete_request`.

## Poll-loop backoff (Phase 2, WP2.1 — BL-03)

`tasks/backoff.rs`'s `Backoff` replaced the poll loops' fixed `tokio::time::interval`:
on success the delay resets to the configured base; on failure it doubles (±10%
jitter from a seeded RNG, so tests stay deterministic) up to a 900s cap. A stuck-down
VTN no longer gets hammered at the configured cadence indefinitely. One trade-off
worth knowing: after a *sustained* outage, recovery latency is bounded by whatever
backoff delay was already in flight when the VTN comes back — the reset to the base
interval only takes effect on the next successful poll, not instantly on recovery
(found while verifying against a live 130s-outage Node1 scenario; see
[[ven-hexagonal-architecture]] for where this sits relative to `VtnPort`).

## Profile startup validation

`Profile::validate()` runs immediately after load, before any task spawns; all violations
are collected and reported together, and the process exits non-zero before touching the
[[simulator]] or planner state (`main.rs:134`).

## Config knobs, not magic numbers

| Value | Form |
|---|---|
| HiGHS time limit | `planner.solver_timeout_s` (profile field, default 60) — see [[milp-planner]] |
| Planning loop startup delay | `planner.planning_initial_delay_s` (profile field, default 5) |
| Replan interval | `planner.replan_interval_s` (profile field, default 300) |
| Peak-demand penalty rules | `planner.penalty_rules` (profile field, default `[]` = off) — see [[milp-planner]] |
| Real-measurement feed enablement | `measurements.pv_enabled` / `.base_load_enabled` (profile fields, default `false`) — second gate alongside `{PV,BASE_LOAD}_MEASUREMENT_MQTT_HOST` env vars, see [[real-measurement-mqtt]] |
| `POST /sim/inject` master switch | `simulator.sim_inject_enabled` (profile field, default `true`; `false` on `ven-1` — R-65) — see [[simulator]] |
| Poll intervals | `POLL_EVENTS_SECS` / `POLL_PROGRAMS_SECS` / `POLL_REPORTS_SECS` env vars (30/30/60), backed off adaptively per-poll (see above) |
| Poll-loop startup jitter | `POLL_STARTUP_JITTER_S` env var (default 0) — Phase 2 GB-09, one-time pre-loop delay so a fleet of VENs brought up together don't poll in lockstep; see [[fleet-tooling]] |
| Obligation check interval | `OBLIGATION_CHECK_INTERVAL_S` = 5, named constant, `tasks/obligation.rs` |
| OAuth token expiry margin | `TOKEN_EXPIRY_MARGIN_S` = 60, named constant, `vtn.rs` |

Counter-examples that still deserve promotion to config: the flexibility-envelope
constants (`max_acceptable_rate: 0.35`, `min_acceptable_rate: 0.05`) hardcoded in
`milp_planner/envelopes.rs` — see [[ven-code-vs-docs-audit]].
