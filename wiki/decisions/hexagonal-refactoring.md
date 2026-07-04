---
title: "Decision: Hexagonal Refactoring of the VEN Backend"
type: decision
created: 2026-07-04
updated: 2026-07-04
synced_commit: eb8831a
sources: [.claude/CLAUDE.md, specs/archive/, openspec/changes/archive/, docs/architecture/module_dependency_graph_post_refactoring.md]
tags: [decision, architecture, refactoring]
---

# Decision: Hexagonal Refactoring of the VEN Backend

The VEN backend is organized into the ring architecture described in
[[ven-hexagonal-architecture]] — ports isolate the domain from HiGHS, Docker, and the VTN
so it can be tested and evolved independently. A flat, handler-and-globals structure
cannot offer that isolation: it has no seam to substitute a mock adapter, and it lets
config and concrete asset types leak into domain logic. Spec series 015–029 and the
follow-up `fix-arch-layer-violations` change (both archived — `specs/archive/`,
`openspec/changes/archive/`) established and then closed the remaining gaps: `*Params`
structs and timeline data-carrier types now live in `entities/`
(`openspec/specs/arch-params-in-entities/spec.md`,
`openspec/specs/arch-timeline-in-entities/spec.md`), and `assets/battery·ev·heater` no
longer re-export MILP types into the domain.

Rule enforcement is incremental by design: grep invariants (below) catch regressions
going forward, but they only cover violations someone has already found — periodic full
architectural review remains necessary to catch the rest.

## Why

- **Testability**: ports allow the four-layer test pyramid ([[testing-strategy]]) —
  domain and use-case tests run against mock adapters
  (`VEN/src/services/test_support/`) without HiGHS, Docker, or a VTN.
- **Swappability**: `SimulatorPort` is the seam where real hardware can replace physics
  models later ([[simulator]], [[asset-layer]]).
- **Config hygiene**: profile decoupling (021, 023) means domain code receives typed
  parameter structs (e.g. `BatteryParams`), never reads global config.

## Enforcement

The dependency rule is kept honest by grep-based invariant checks in `.claude/CLAUDE.md`
(no `crate::profile` in inner rings, no concrete assets in the solver, no
`serde_json::Value` beyond `vtn.rs`) plus the post-refactoring module graph snapshot
(docs/architecture/module_dependency_graph_post_refactoring.md) — re-verified quarterly
per `docs/reference/SESSION_START.md`.
