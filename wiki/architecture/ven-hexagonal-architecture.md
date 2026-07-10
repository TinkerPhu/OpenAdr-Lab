---
title: VEN Hexagonal Architecture
type: architecture
created: 2026-07-04
updated: 2026-07-10
synced_commit: 88e0e25
sources: [.claude/CLAUDE.md, docs/architecture/VEN_ARCHITECTURE.md, docs/architecture/module_dependency_graph_post_refactoring.md, VEN/src/]
tags: [architecture, hexagonal, ports, ven]
---

# VEN Hexagonal Architecture

`VEN/src/` follows Hexagonal + Clean Architecture with a strict dependency rule:
**inner rings never import outer rings** (`.claude/CLAUDE.md` §ven-architecture).

## Ring map (outer → inner)

| Ring        | Modules |
|-------------|---------|
| Adapters    | `routes/`, `tasks/` |
| Application | `services/` |
| Domain      | `entities/`, `controller/` |
| Infra       | `assets/`, `simulator/`, `vtn.rs`, `controller/milp_planner/` |

The MILP solver sits in infra even though it lives under `controller/` — its asset
inputs arrive only through the `AssetMilpContext` port. See [[milp-planner]] and
[[simulator]] for the two big infra blocks.

## Ports (traits — never bypassed with concrete types)

| Port | Direction | Purpose |
|---|---|---|
| `SimulatorPort` | domain/services → simulator | `snapshot()` (`controller/simulator_port.rs`) |
| `VtnPort` | tasks/services → `vtn.rs` | fetch programs/events/reports, upsert reports (`controller/vtn_port.rs`) |
| `AssetMilpContext` | planner input | solver receives `Vec<Box<dyn AssetMilpContext>>`; concrete asset types implement it in `assets/*.rs` (`controller/milp_planner/asset_port.rs`) |
| `SolverPort` | services → `controller/milp_planner` | `solve(SolveRequest) -> Plan` (`controller/solver_port.rs`); `MilpSolver` (in `milp_planner/mod.rs`) is the real implementation, wrapping `run_planner()`; `services::PlanningService::solve_plan` is the only caller |
| `HistoryPort` | domain/routes/tasks → `history_store` | append/query/prune for ticks, grid samples, plan snapshots, events, reports, ledger periods (`controller/history_port.rs`); `SqliteHistoryStore` is the real implementation, all methods synchronous (`rusqlite`), called from async contexts via `tokio::task::spawn_blocking` — see [[history-store]] |

All five ports are now real traits with a concrete implementation and a mock
(`services/test_support/mock_solver_port.rs`, `mock_history_port.rs`, alongside the
pre-existing simulator/VTN mocks) — `tasks/planning.rs`'s planning loop calls
`SolverPort::solve` through the trait object, not `milp_planner::run_planner()` directly.

## Enforced invariants (grep checks, run before any VEN PR)

From `.claude/CLAUDE.md`: no `use crate::profile` in `entities/`, `controller/`, `routes/`
(profile values arrive as typed parameter structs); no `use crate::assets::` inside
`milp_planner/` or `entities/`; no `serde_json::Value` leaking out of `vtn.rs`.
All four greps still pass (the raw-report pass-through `VtnPort::fetch_reports_raw`
→ `PollingState.reports` is a deliberate, commented exception for `GET /reports`).

File-size caps: 500 lines in `VEN/src/`, 200 in `tasks/`. **These are currently
violated** and the violation count is growing, not shrinking, as normal feature work
lands (e.g. `routes/timeline.rs` and `controller/timeline.rs` both grew further past the
cap during the 2026-07 timeline-forecast fix) — current register with per-file
complexity/risk: `docs/reference/TECHNICAL_DEBTS.md`. `tasks/planning.rs` is 398 lines
against the 200 cap for `tasks/`. Split-or-amend options in [[ven-code-vs-docs-audit]];
R4 in `docs/plans/review_items_resolution_strategy.md` is the open decision item.

The rationale for this ring shape is in [[hexagonal-refactoring]]; the two-speed runtime
behaviour of the rings is described in [[hems-planning]]. `.claude/CLAUDE.md`
§ven-architecture points to `docs/architecture/VEN_ARCHITECTURE.md` and the module
dependency graph as the canonical references.
