---
title: "Decision: Hexagonal Refactoring of the VEN Backend"
type: decision
created: 2026-07-04
updated: 2026-07-04
synced_commit: 4695762
sources: [.claude/CLAUDE.md, specs/, docs/architecture/module_dependency_graph_post_refactoring.md]
tags: [decision, architecture, refactoring]
---

# Decision: Hexagonal Refactoring of the VEN Backend

The VEN began as a flat Rust app (`main.rs` with all handlers, `models.rs`, `state.rs` —
docs/history/project_journal.md §5). It was progressively refactored into the ring
architecture described in [[ven-hexagonal-architecture]], executed as a numbered spec
series in `specs/`: 016 (refactor backend), 019 (SimulatorPort), 020 (MILP asset port),
021 (decouple profile from domain), 023 (remove profile from routes), 025–026 (typed VTN
report / reporter domain types), 027 (clean timeline infra), 029 (arch invariant tests).

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
