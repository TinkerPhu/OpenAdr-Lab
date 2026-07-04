---
title: VEN Hexagonal Architecture
type: architecture
created: 2026-07-04
updated: 2026-07-04
synced_commit: 4695762
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

The MILP solver sits in infra even though it lives under `controller/` — it is reached
only through a port. See [[milp-planner]] and [[simulator]] for the two big infra blocks.

## Ports (traits — never bypassed with concrete types)

| Port | Direction | Purpose |
|---|---|---|
| `SimulatorPort` | domain/services → simulator | snapshot, inject |
| `SolverPort` | services → `controller/milp_planner` | solve |
| `VtnPort` | services → `vtn.rs` | fetch programs/events/obligations |
| `AssetMilpContext` | planner input | solver accepts `Vec<Box<dyn AssetMilpContext>>`; never imports concrete assets |

## Enforced invariants (grep checks, run before any VEN PR)

From `.claude/CLAUDE.md`: no `use crate::profile` in `entities/`, `controller/`, `routes/`
(profile values arrive as typed parameter structs); no `use crate::assets::` inside
`milp_planner/` or `entities/`; no `serde_json::Value` leaking out of `vtn.rs`.
File-size caps: 500 lines in `VEN/src/`, 200 in `tasks/`.

The refactoring that produced this shape is chronicled in [[hexagonal-refactoring]];
the two-speed runtime behaviour of the rings is described in [[hems-planning]].

(A stale reference in `.claude/CLAUDE.md` to the deleted
`docs/plans/ven_backend_architecture_refactoring.md` was fixed on 2026-07-04; it now
points to `docs/architecture/VEN_ARCHITECTURE.md` and the module dependency graph.)
