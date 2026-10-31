---
title: "Decision: Hexagonal Refactoring of the VEN Backend"
type: decision
created: 2026-07-04
updated: 2026-10-31
synced_commit: 102f7fa
sources: [.claude/CLAUDE.md, ci/audits/audit_ven_architecture.py, docs/architecture/module_dependency_graph.md]
tags: [decision, architecture, refactoring]
---

# Decision: Hexagonal Refactoring of the VEN Backend

The VEN backend is organized into the ring architecture described in
[[ven-hexagonal-architecture]] — ports isolate the domain from HiGHS, Docker, and the VTN
so it can be tested and evolved independently. A flat, handler-and-globals structure
cannot offer that isolation: it has no seam to substitute a mock adapter, and it lets
config and concrete asset types leak into domain logic. Spec series 015–029 (git history
only — the working spec artifacts were pruned as redundant with this synthesis) and the
follow-up `fix-arch-layer-violations` change established and then closed the remaining
gaps: `*Params` structs and timeline data-carrier types now live in `entities/` — enforced by
the invariants of `.claude/CLAUDE.md`'s `ven-architecture` rule (`entities does not import from assets`, etc.),
which `ci/audits/audit_ven_architecture.py` checks on production code — and `assets/battery·ev·heater` no longer re-export MILP types into the
domain.

Rule enforcement is incremental by design: the audit's rules (below) catch regressions going forward, but
they only cover violations someone has already found — every one a review finds becomes a rule — and a periodic
full architectural review remains necessary to catch the rest.

## Why

- **Testability**: ports allow the four-layer test pyramid ([[testing-strategy]]) —
  domain and use-case tests run against mock adapters
  (`VEN/src/services/test_support/`) without HiGHS, Docker, or a VTN.
- **Swappability**: the simulator ports (`SimRosterPort`, `SimReadPort`, `HeadroomPort`, reached through
  `simulator::SimHandle`) are the seam where real hardware can replace physics
  models later ([[simulator]], [[asset-layer]]).
- **Config hygiene**: profile decoupling (021, 023) means domain code receives typed
  parameter structs (e.g. `BatteryParams`), never reads global config.

## Enforcement

The dependency rule is kept honest by `ci/audits/audit_ven_architecture.py`, part of the merge gate
([[ci-pipeline]]), and described in `.claude/CLAUDE.md` (no `crate::profile` in inner rings, no concrete assets in
the solver, no `serde_json::Value` beyond `vtn.rs`, no asset kind in the planner core, one ranking of events
by priority, one place for unit conversion, and more) plus the post-refactoring module graph snapshot
(docs/architecture/module_dependency_graph.md, with a draw.io component diagram and import graph beside it),
re-verified on the schedule `jobs.json` records (`docs/reference/SESSION_START.md` §4, recurring controls).
