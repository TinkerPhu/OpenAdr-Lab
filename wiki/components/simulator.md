---
title: Simulator
type: component
created: 2026-07-04
updated: 2026-07-05
synced_commit: e138861
sources: [VEN/src/simulator/, VEN/src/state.rs, docs/architecture/asset_simulation.md, docs/architecture/VEN_ARCHITECTURE.md]
tags: [simulator, physics, determinism]
---

# Simulator

Physics-based device models standing in for real hardware: PV, battery, EV, heater,
base load (`VEN/src/simulator/`, physics per asset in `VEN/src/assets/`, models specified
in docs/architecture/asset_simulation.md). The [[dispatcher]] writes setpoints to it; the
[[asset-layer]] reads state from it. `SimState.tick()` steps every asset through its
`AssetConfig::step()`, derives the grid meter from the power sum, and maintains a virtual
**Grid asset** with its own history and the active VTN capacity limits.

## Boundaries

- Controller logic reads it only through `SimulatorPort::snapshot()` / precomputed
  `SimSnapshot`s — domain and services never touch `SimState` types directly
  ([[ven-hexagonal-architecture]]).
- `/sim` REST endpoints exist **for the UI and tests only** — the controller must not
  depend on them.
- `tick()` takes `now` and `dt_s` as parameters (injectable clock), so tests reproduce
  identical trajectories without sleeps ([[testing-strategy]], `.claude/CLAUDE.md`
  §determinism).

## State injection (`POST /sim/inject`, `state.rs::SimInjectState`)

Four behaviour classes, replacing the older full-replace `/sim/override` API that
`docs/architecture/VEN_ARCHITECTURE.md` §4.5/D-06 still documents
([[ven-code-vs-docs-audit]]):

| Behaviour | Fields | Semantics |
|---|---|---|
| A — one-shot | `battery_soc`, `ev_soc`, `heater_temp_c` | applied once to physics state, then cleared |
| B — frozen + EMA return | `pv_irradiance`, `base_load_kw` (+ alphas) | held while active; offset decays exponentially on release |
| C — frozen + snap | `ev_plugged`, `ev_soc_target`, `heater_setpoint_c`, comfort band, ambient, grid limits | held while active; snaps to profile default on release |
| D — planning-only | `pv_plan_kw` | pins the PV forecast for all horizon slots; no physics effect |

Injected grid limits only apply when no VTN capacity event is active — real events win
(`tasks/sim_tick/helpers.rs`).

## Role in planning

Simulator snapshots feed the [[milp-planner]] inputs (live SoC, temperatures, plugged
state — never profile initial values) and the flexibility envelope computation; per-asset
history ring buffers ground the obligation reports sent by [[openadr-interface]] and the
`/timeline` API. The PV forecast projects the live irradiance offset forward with
per-slot exponential decay, so a UI slider drag is visible in the plan and fades
realistically. Heater-tank thermal behaviour has its own MILP-facing model
(docs/architecture/heater_tank_milp_planning_model.md).
