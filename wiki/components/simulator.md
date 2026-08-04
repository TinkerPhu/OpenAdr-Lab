---
title: Simulator
type: component
created: 2026-07-04
updated: 2026-08-03
synced_commit: 50961b5
sources: [VEN/src/simulator/, VEN/src/state/mod.rs, VEN/src/routes/sim.rs, docs/architecture/asset_simulation.md, docs/architecture/VEN_ARCHITECTURE.md]
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
  §determinism). R-24 (closed) extended this discipline further: `AssetConfig::forecast()`,
  `Asset::history()`/`simulate_free()`/`capability_trajectory()`, and
  `SimState::from_params()` now take an explicit `now` instead of calling `Utc::now()`
  internally, and `SimState` carries a seedable `StdRng` so `power_model::random_voltage()`
  no longer draws from unseeded `thread_rng()` — see [[asset-layer]] for the full list of
  call sites and the one dead-code exception found (R-62, `entities/site_meter.rs`).

## State injection (`POST /sim/inject`, `state.rs::SimInjectState`)

Four behaviour classes, replacing the older full-replace `/sim/override` API that
`docs/architecture/VEN_ARCHITECTURE.md` §4.5/D-06 still documents
([[ven-code-vs-docs-audit]]):

| Behaviour | Fields | Semantics |
|---|---|---|
| A — one-shot | `battery_soc`, `ev_soc`, `heater_temp_c` | applied once to physics state, then cleared |
| B — frozen + EMA return | `pv_irradiance`, `base_load_kw` (+ alphas) | held while active; offset decays exponentially on release |
| C — frozen + snap | `ev_plugged`, `ev_soc_target`, `heater_setpoint_c`, comfort band, ambient, grid limits, `pv_generation_limit_kw` | held while active; snaps to profile default on release |
| D — planning-only | `pv_plan_kw` | pins the PV forecast for all horizon slots; no physics effect |

Injected grid limits only apply when no VTN capacity event is active — real events win
(`tasks/sim_tick/helpers.rs`). `pv_generation_limit_kw` (Behaviour C, `PvCurtailmentSource::Manual`)
triggers a replan the same way `grid_import_limit_kw`/`grid_export_limit_kw` do — see
[[asset-layer]]'s PV curtailment section for the four-way resolution it participates in.

**Null-clear fix (`routes/sim.rs`)**: `POST /sim/inject`'s body originally used
`Option<serde_json::Value>` per field, but serde_json's blanket `Option<T>` impl collapses a
top-level JSON `null` to Rust `None` before `T::deserialize` ever runs — so an explicit
`{"field": null}` request body was indistinguishable from the field being absent entirely,
making the documented null-clear behaviour structurally unreachable via real HTTP calls
(confirmed live on Node1 for both `pv_generation_limit_kw` and `grid_export_limit_kw`). Every
field now deserializes through a `double_option` helper (`Option<Option<T>>`), restoring the
three-way absent/null/value distinction the endpoint's doc comment always claimed. The
original unit tests only constructed `PostSimInjectBody` directly in Rust, bypassing real JSON
deserialization — a false positive; the regression test now goes through `serde_json::from_str`.

`SimState::to_sensor_snapshot`/`to_sim_snapshot`/`to_timeline_snapshot` moved out of
`simulator/mod.rs` into `simulator/snapshot.rs` (file-size cap, `.claude/CLAUDE.md`) when
[[real-measurement-mqtt]] threaded two new `Option<f64>` parameters through `SimState::tick`.

## Role in planning

Simulator snapshots feed the [[milp-planner]] inputs (live SoC, temperatures, plugged
state — never profile initial values) and the flexibility envelope computation; per-asset
history ring buffers ground the obligation reports sent by [[openadr-interface]] and the
`/timeline` API. The PV forecast projects the live irradiance offset forward with
per-slot exponential decay, so a UI slider drag is visible in the plan and fades
realistically. Heater-tank thermal behaviour has its own MILP-facing model
(docs/architecture/heater_tank_milp_planning_model.md).
