---
title: Simulator
type: component
created: 2026-07-04
updated: 2026-10-22
synced_commit: 650b626
sources: [VEN/src/simulator/, VEN/src/state/mod.rs, VEN/src/routes/sim.rs, VEN/src/entities/sim_inject.rs, VEN/src/tasks/sim_tick/, VEN/src/boot/serve.rs, docs/architecture/asset_simulation.md, docs/architecture/VEN_ARCHITECTURE.md]
tags: [simulator, physics, determinism]
---

# Simulator

Physics-based device models standing in for real hardware: PV, battery, EV, heater, base load
(`VEN/src/simulator/`, physics per asset in `VEN/src/assets/`, models specified in
docs/architecture/asset_simulation.md). The [[dispatcher]] and [[deviation-arbiter]] decide a setpoint
per asset; the [[asset-layer]] owns each asset's physics; the simulator owns the roster and the
tick that steps it, derives the site meter from the power sum, and keeps the site's `Grid` meter
(net power, VTN limits, history — the meter, not an asset).

## The roster and the tick

`SimState` (`simulator/mod.rs`) holds two parallel vectors: `asset_configs: Vec<Box<dyn Asset>>` (the
physics, rebuilt from the profile on every boot and never persisted) and `assets: Vec<AssetEntry>` (mutable
`AssetState`, last setpoint, last power, an energy counter and a history ring buffer). An asset
is built from its `AssetParams` variant in one place (`asset_config_and_state_from_params`), so a
profile can carry several assets of one kind. Shiftable loads join and leave the roster at run time; an
asset that reports itself finished (`is_removable`) is dropped in a separate pass after the tick.

`SimState::tick(TickInputs)` steps every asset once, in this order per asset: apply the tick's overrides to
its state (`TickOverridable::apply_tick_overrides`), step its physics at the commanded setpoint
(`Asset::step`, falling back to the asset's `default_setpoint`), integrate its energy. Then
`derive_grid_meter` sums the asset powers into the meter: net, import, export, a random voltage from the
seeded RNG, and cumulative kWh. The simulated meter is exactly that sum; the site's unmetered consumption
is the `base_load` asset, not a perturbation of the meter ([[sign-convention]]: import positive).

`TickInputs` (`simulator/tick_inputs.rs`) is everything one tick is told about the outside world, as a named
struct with `TickInputs::new(dt_s, now, setpoints)` for "nothing unusual" — it replaced 25 positional
parameters, 20 of them `Option`. It carries the sim-inject overrides, the arbiter's per-asset heater
emergency modes and EV departures, request targets under the [[opportunistic-objective]], and
`AssetTickFeeds`: per asset, a PV array's weather estimate and forecast series ([[weather-forecast]]),
the real measured reading of the asset a feed measures ([[real-measurement-mqtt]]), and a base load's
learned heuristic ([[heuristics-pipeline]]). An asset without an entry has no such feed this tick. The
generation limit is shared over the PV arrays by their AC ceilings (`generation_limit_share`), so the
arrays' limits add up to the site's.

Behaviour B smoothing (the PV irradiance perturbation and each base load's offset) lives in `SimState`
and is resolved before the asset loop, so each asset's override step only assigns fields. Previews
(`peek_pv_kw`, `peek_base_load_kw`) answer what the tick will produce under a given generation limit
without stepping, which is how the arbiter counts previewed power rather than a one-tick-stale snapshot.

## Boundaries

- Services and routes never hold `SimState`: they use `SimRosterPort` (add or cancel a shiftable,
  reset or reconfigure an asset), `SimReadPort` (timeline snapshot, an asset's capability, comfort
  bands, usage schedule, request slices) and `HeadroomPort` (site headroom, capacity curves), all
  implemented by `simulator::SimHandle` (`simulator/handle.rs`). Every method locks for one call and
  releases before returning, so a caller cannot hold the simulator across an `.await`. The tick tasks
  alone take the lock and read `SimState::to_sim_snapshot` ([[ven-hexagonal-architecture]]).
- Plan-cycle helpers that need concrete simulator state (`plan_context.rs`) live in this ring and clone the
  state under the lock, because a MILP solve takes tens of seconds on Node1 and must never hold it.
  Site headroom, capacity curves, the plan-driven forward re-simulation (`forecast.rs`) and the
  opportunistic forecast also sit here, because they read `&SimState` and resolve concrete assets; what
  leaves the module is an entity ([[milp-planner]], [[tariffs-and-capacity]]).
- `/sim` REST endpoints exist **for the UI and tests only** — the controller must not depend on them.
- `tick()` takes `now` and `dt_s` as parameters, and `SimState` carries a seedable `StdRng` (the voltage
  draw), so tests reproduce identical trajectories without sleeps ([[testing-strategy]], determinism rule).
  `SimState::from_params` and the forecast functions take an explicit `now`.
- Persistence (`simulator/persist.rs`): the state is written atomically (temp file and rename) to
  `sim_state.json` every `simulator.persist_every_s` and on shutdown. Only mutable runtime state
  is restored on load; configs are rebuilt from the current profile, so a config change takes effect on restart.
  A missing or corrupt file, a changed asset list, or an id that now names another asset kind each fall back
  to the fresh state for that part, so a profile edit cannot crash-loop the boot. `boot/serve.rs` persists on
  `SIGTERM` as well as `SIGINT`, so a container recreate does not leave the file stale against the
  continuously decaying PV offset.

## State injection (`POST /sim/inject`, `entities/sim_inject.rs::SimInjectState`)

Four behaviour classes. The body is a partial merge, and every field goes through a `double_option` helper
(`Option<Option<T>>`) so the three JSON shapes stay apart: absent leaves the field alone, `null` releases the
override, a value sets it. A plain `Option<T>` collapses a top-level `null` into "absent" before the value
type is read, which once made the documented null-clear unreachable over real HTTP; the regression test
goes through `serde_json::from_str` for that reason.

| Behaviour | Fields | Semantics |
|---|---|---|
| A — one-shot | `battery_soc`, `ev_soc`, `heater_temp_c` | applied once to every asset of that kind (`simulator/inject.rs`, one row per field), then cleared |
| B — frozen + EMA return | `pv_irradiance` (`pv_tau_s`), `base_load_kw` (`base_load_alpha`) | held while active; the offset decays exponentially on release |
| C — frozen + snap | `ev_plugged`, `ev_soc_target`, `heater_setpoint_c`, `heater_temp_min_c`/`_max_c`, `heater_emergency_curtail`/`_absorb`, `ambient_temp_c`, `grid_import_limit_kw`, `grid_export_limit_kw`, `pv_generation_limit_kw` | held while active; snaps to the profile default on release |
| D — planning-only | `pv_plan_kw` | pins the PV forecast for all horizon slots; no physics effect and no replan |

An injected grid limit substitutes for a VTN limit where none applies and never overrides one — real events win
(`entities/capacity.rs::effective_capacity_limit`, the only home of that rule). `pv_generation_limit_kw` is the `PvCurtailmentSource::Manual` source of the generation
limit ([[asset-layer]]). A replan (`PlanTrigger::AssetStateChange`) is requested for every planner-relevant
field (SoCs, plug state, targets, temperatures, grid limits, PV limit and irradiance) and deliberately not for
`base_load_kw`/`base_load_alpha` or `pv_plan_kw`, which would race a test's assertion window by adopting a new
plan mid-test. `POST /sim/inject/reset` releases everything; `POST /plan/trigger` forces a replan without
touching the sim. Both inject routes log at `warn!` with the caller's peer address and the body.

Other routes: `GET /sim` (the live state, 503 until the simulator exists), `GET /sim/schema` (control descriptors
precomputed at boot, so it answers without the sim lock while the planner runs), `POST /sim/reset/{asset_id}`
(jump an asset's SoC), `PUT /sim/config/battery/{asset_id}` (capacity and minimum SoC), `GET /sim/inject`.

## Why `/sim/inject` is logged loudly

`/sim/inject` once caused unattributed production PV-irradiance overrides on ven-1 with no trace of the
caller. The root cause turned out to be a VEN UI integration test whose `VITE_VEN_URL` fallback defaulted to ven-1's
production address, so any `npm test` on the LAN POSTed a real override (`docs/history/project_journal.md`, "the
ven-1 PV-injection mystery"). It is fixed and the tracking scaffolding is gone; what stays is the `warn!`
logging with the peer address. `ConnectInfo` is the direct TCP peer only, so a caller behind the `ven-ui` nginx
proxy shows up as nginx, and attributing it needs nginx's access log as well.

## Role in planning

Simulator snapshots feed the [[milp-planner]] inputs (live SoC, temperatures, plugged state — never profile
initial values) and the flexibility envelope; per-asset history ring buffers ground the obligation reports sent
by [[openadr-interface]] and the `/timeline` API. The PV forecast projects the live irradiance offset forward
with per-slot exponential decay, so a UI slider drag is visible in the plan and fades realistically. The
forward re-simulation starts from each asset's real current state, never from the plan's solve-time snapshot.
Heater-tank thermal behaviour has its own MILP-facing model
(docs/architecture/heater_tank_milp_planning_model.md).

> **DRIFT** `docs/architecture/asset_simulation.md` still describes the pre-trait shapes: `build_setpoints` taking
> `&[AssetConfig]`, a positional `SimState::tick(dt_s, setpoints, now, …)` signature, and Behaviour A injects applied by the
> tick task. In the code, `AssetConfig` is gone (one `Asset` trait), `tick` takes a
> `TickInputs`, and the one-shot injects are applied by `simulator/inject.rs`. The physics sections are
> still the reference for the models themselves ([[ven-code-vs-docs-audit]]).
