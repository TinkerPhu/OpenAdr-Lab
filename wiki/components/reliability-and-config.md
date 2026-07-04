---
title: VEN Reliability & Config Hygiene
type: component
created: 2026-07-04
updated: 2026-07-04
synced_commit: eb8831a
sources: [VEN/src/tasks/mod.rs, VEN/src/entities/error.rs, VEN/src/profile.rs, VEN/src/tasks/obligation.rs, VEN/src/vtn.rs, openspec/changes/archive/2026-07-04-fix-tech-debt-gaps/, openspec/specs/task-supervisor/spec.md, openspec/specs/domain-errors/spec.md, openspec/specs/profile-validation/spec.md, openspec/specs/planner-config/spec.md]
tags: [reliability, config, error-handling, ven]
---

# VEN Reliability & Config Hygiene

Four small, related hardening measures close silent-failure modes that a plain `anyhow`+
`tokio::spawn` baseline leaves open: an unsupervised panic in a background task takes down
DR control with no restart, an unvalidated profile can drive wrong physics at runtime, and
undifferentiated errors block domain-specific recovery (e.g. keeping the last valid plan
on infeasibility).

## Task supervision

A single `supervised_spawn(name, cooldown_s, f)` utility (`VEN/src/tasks/mod.rs`) wraps all
seven background task spawns (`sim_tick`, `planning`, `poll_events`, `poll_programs`,
`poll_reports`, `obligation`, `state_persist`). On panic: log at ERROR with task name and
panic message, wait 5 s, then re-spawn — the restart loop lives in one place, not repeated
per call site. The HTTP server (and `GET /health`) keeps serving throughout the cooldown.

## Typed domain errors

`entities::DomainError` (`thiserror`-based) covers the failure modes callers actually need
to distinguish: `SessionConflict`, `NotFound { id }`, `PlanInfeasible`, `VtnUnreachable`,
`ProfileInvalid`. Route handlers map variants to HTTP status (e.g. `SessionConflict` → 409,
`NotFound` → 404). This is **additive**, not a migration — internal helpers that don't need
domain discrimination keep `anyhow::Result`. The main payoff is at the
[[milp-planner]] boundary: `PlanInfeasible` is now distinguishable from a `VtnUnreachable`
network error, so infeasibility can retain the last valid plan instead of being handled
identically to a transport failure.

## Profile startup validation

`Profile::validate()` runs immediately after `Profile::load()`, before any task spawns.
Checks include: every `absorber.assets[].id` must reference a declared asset; numeric
bounds (`ev.soc_target ∈ [0,1]`, `battery.round_trip_efficiency ∈ (0,1]`,
`planner.replan_interval_s > 0`, etc.); at least one asset declared. All violations are
collected and reported together — an operator fixing a bad profile doesn't restart the VEN
once per typo. Failure exits non-zero before touching the [[simulator]] or planner state.

## Config knobs, not magic numbers

| Value | Form |
|---|---|
| HiGHS time limit | `planner.solver_timeout_s` (profile field, default 60) — see [[milp-planner]] |
| Planning loop startup delay | `planner.planning_initial_delay_s` (profile field, default 5) |
| Obligation check interval | `OBLIGATION_CHECK_INTERVAL_S` named constant, `tasks/obligation.rs` |
| OAuth token expiry margin | `TOKEN_EXPIRY_MARGIN_S` named constant, `vtn.rs` |

The first two are profile-configurable (operators can tune them per deployment); the
latter two are internal constants, named for readability rather than exposed as config.
