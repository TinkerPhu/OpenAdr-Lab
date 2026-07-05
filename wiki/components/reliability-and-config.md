---
title: VEN Reliability & Config Hygiene
type: component
created: 2026-07-04
updated: 2026-07-05
synced_commit: e138861
sources: [VEN/src/tasks/mod.rs, VEN/src/entities/error.rs, VEN/src/profile.rs, VEN/src/tasks/obligation.rs, VEN/src/vtn.rs, openspec/specs/task-supervisor/spec.md, openspec/specs/domain-errors/spec.md, openspec/specs/profile-validation/spec.md, openspec/specs/planner-config/spec.md]
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
`PlanInfeasible`, `VtnUnreachable`, `ProfileInvalid`. In practice only the first two are
live: route/service code maps `SessionConflict` → 409 and `NotFound` → 404
(`services/hems.rs`).

> **DRIFT** `openspec/specs/domain-errors/spec.md` motivates the type with "infeasibility
> can retain the last valid plan instead of being handled like a transport failure" — but
> `PlanInfeasible`, `VtnUnreachable`, and `ProfileInvalid` are never constructed anywhere
> in production code. Solver failure is handled inside the planner by returning a
> fallback plan carrying a Critical `PlanWarning` (`milp_planner/results.rs`), VTN errors
> stay `anyhow`, and profile validation returns a plain `Vec<String>`. The retain-last-
> plan behaviour exists (the acceptance gate simply isn't offered a broken plan), but not
> via these variants. Wire them or trim the enum. See [[ven-code-vs-docs-audit]].

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
| Poll intervals | `POLL_EVENTS_SECS` / `POLL_PROGRAMS_SECS` / `POLL_REPORTS_SECS` env vars (30/30/60) |
| Obligation check interval | `OBLIGATION_CHECK_INTERVAL_S` = 5, named constant, `tasks/obligation.rs` |
| OAuth token expiry margin | `TOKEN_EXPIRY_MARGIN_S` = 60, named constant, `vtn.rs` |

Counter-examples that still deserve promotion to config: the flexibility-envelope
constants (`max_acceptable_rate: 0.35`, `min_acceptable_rate: 0.05`) hardcoded in
`milp_planner/envelopes.rs` — see [[ven-code-vs-docs-audit]].
