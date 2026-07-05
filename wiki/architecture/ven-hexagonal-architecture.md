---
title: VEN Hexagonal Architecture
type: architecture
created: 2026-07-04
updated: 2026-07-05
synced_commit: e138861
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

> **DRIFT** `.claude/CLAUDE.md` §ven-architecture and `docs/architecture/VEN_ARCHITECTURE.md`
> list a fourth port, `SolverPort: services → controller/milp_planner`. No such trait
> exists — `tasks/planning.rs:266` calls `milp_planner::run_planner()` (a free function)
> directly inside `spawn_blocking`. The solver *is* isolated from concrete assets (via
> `AssetMilpContext`), but there is no trait seam between the planning loop and HiGHS;
> `services/test_support/milp_mocks.rs` mocks inputs, not a solver port. Either introduce
> the port or drop it from the rule. Full audit: [[ven-code-vs-docs-audit]].

## Enforced invariants (grep checks, run before any VEN PR)

From `.claude/CLAUDE.md`: no `use crate::profile` in `entities/`, `controller/`, `routes/`
(profile values arrive as typed parameter structs); no `use crate::assets::` inside
`milp_planner/` or `entities/`; no `serde_json::Value` leaking out of `vtn.rs`.
All four greps pass at e138861 (the raw-report pass-through `VtnPort::fetch_reports_raw`
→ `PollingState.reports` is a deliberate, commented exception for `GET /reports`).

File-size caps: 500 lines in `VEN/src/`, 200 in `tasks/`. **These are currently
violated** — nine files exceed 500 production lines (worst: `assets/heater.rs` 799,
`profile.rs` 777) and `tasks/planning.rs` is 363; split-or-amend options in
[[ven-code-vs-docs-audit]].

One more `.claude/CLAUDE.md` mismatch: the shared mock adapters in
`services/test_support/` are `#[cfg(test)]`-gated (`services/mod.rs:2`), while the
testing rule says they are not.

The rationale for this ring shape is in [[hexagonal-refactoring]]; the two-speed runtime
behaviour of the rings is described in [[hems-planning]]. `.claude/CLAUDE.md`
§ven-architecture points to `docs/architecture/VEN_ARCHITECTURE.md` and the module
dependency graph as the canonical references.
