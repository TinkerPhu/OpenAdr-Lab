---
title: Simulator
type: component
created: 2026-07-04
updated: 2026-07-04
synced_commit: 4695762
sources: [VEN/src/simulator/, docs/architecture/asset_simulation.md, docs/architecture/VEN_ARCHITECTURE.md]
tags: [simulator, physics, determinism]
---

# Simulator

Physics-based device models standing in for real hardware: PV, battery, EV, heater,
base load (`VEN/src/simulator/`, models specified in docs/architecture/asset_simulation.md).
The [[dispatcher]] writes setpoints to it; the [[asset-layer]] reads state from it.

## Boundaries

- Reached only through `SimulatorPort` (snapshot, inject) — domain and services never
  touch simulator types directly ([[ven-hexagonal-architecture]]).
- `/sim` REST endpoints (simulation params, overrides, schema, reset) exist **for the UI
  only** — the controller must not depend on them
  (docs/architecture/VEN_ARCHITECTURE.md §1).
- Deterministic by construction: takes an injectable clock, so tests reproduce identical
  trajectories without sleeps ([[testing-strategy]], `.claude/CLAUDE.md` §determinism).

## Role in planning

Simulator snapshots feed the [[milp-planner]] inputs (current SoC, temperatures,
connectivity) and the flexibility envelope computation; the same snapshots ground the
`USAGE`/`DEMAND` reports sent by [[openadr-interface]]. Heater-tank thermal behaviour has
its own MILP-facing model (docs/architecture/heater_tank_milp_planning_model.md).
