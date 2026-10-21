---
title: VEN Hexagonal Architecture
type: architecture
created: 2026-07-04
updated: 2026-10-21
synced_commit: 4c4f149
sources: [.claude/CLAUDE.md, docs/architecture/VEN_ARCHITECTURE.md, docs/architecture/module_dependency_graph.md, VEN/src/]
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
| `SimRosterPort` / `SimReadPort` / `HeadroomPort` | services/routes → simulator, via `simulator::SimHandle` | roster changes, read-only asset/series reads, headroom queries; `SimulatorPort` is deleted and only `tasks/` hold the `SimState` lock |
| `VtnPort` | tasks/services/routes → `vtn.rs` | fetch programs/events/reports, upsert reports (`controller/vtn_port.rs`); held as `app_ctx::Vtn` so a handler is driven with `MockVtn` |
| `AssetMilpContext` | planner input | solver receives `Vec<Box<dyn AssetMilpContext>>`; each asset declares its own part of the model and of the plan in `assets/<kind>_declared.rs`, so the planner core names no asset kind and the plan keys each asset by id ([[milp-planner]]) |
| `SolverPort` | services → `controller/milp_planner` | `solve(SolveRequest) -> Plan` (`controller/solver_port.rs`); `MilpSolver` (in `milp_planner/mod.rs`) is the real implementation, wrapping `run_planner()`; `services::PlanningService::solve_plan` is the only caller |
| `HistoryPort` | domain/routes/tasks → `history_store` | append/query/prune for ticks, grid samples, events received, reports sent, ledger periods, and forecast-accuracy samples (`controller/history_port.rs`); `SqliteHistoryStore` is the real implementation, all methods synchronous (`rusqlite`), called from async contexts via `tokio::task::spawn_blocking` — see [[history-store]] |
| `WeatherForecastPort` | services/tasks → `weather.rs` | `latest()`/`is_alive()` (`controller/weather_port.rs`); `MqttWeatherAdapter` is the real implementation — see [[weather-forecast]] |
| `MeasurementPort` | tasks/routes → `measurement.rs` | `latest_kw()`/`is_alive()` (`controller/measurement_port.rs`); `MqttMeasurementAdapter` is the real implementation, one instance per signal (PV, baseline load) — see [[real-measurement-mqtt]] |

The ports are real traits with a concrete implementation and a mock
(`services/test_support/mock_solver_port.rs`, `mock_history_port.rs`, `mock_weather_port.rs`,
alongside the pre-existing simulator/VTN mocks) — `tasks/planning.rs`'s planning loop calls
`SolverPort::solve` through the trait object, not `milp_planner::run_planner()` directly.

## Invariants (audit rules, current)

`python ci/audits/audit_ven_architecture.py` applies these to production code (comments and
`#[cfg(test)]` stripped) and runs in the [[ci-pipeline]] quick tier. Beyond the original profile/assets
import bans: no `Utc::now()` below the adapters (they read the clock once and pass `now` down); no
`.val("…")` string-key read of an asset's `state_values()` outside `assets/` (a typed `Asset` method
carried on `AssetSnapshot`); no `crate::services` in `assets/` or `simulator/`; no `crate::vtn` or
`crate::simulator` in `entities/`, `services/`, `state/`; no hand-authenticated request outside
`lab-core/src/vtn_auth.rs`; no inline unit conversion outside `entities/units.rs` and no `_w`
identifier (power is kW everywhere); no handler taking the whole `AppCtx` (each part has a `FromRef`);
no raw read of an incoming event value outside `lab_core::wire_contract::PayloadReader`
([[wire-contract]]); no asset kind in the planner core or `entities/plan.rs`; an
`AssetSolutionDetail` variant is matched only in its own `assets/<kind>_declared.rs`. Generated UI types
(ts-rs) come from the Rust types, so the UI cannot drift from them. The module import graph
(`docs/architecture/module_dependency_graph.md`) is generated and its coverage checked.

## Profile and import bans

From `.claude/CLAUDE.md`: no `use crate::profile` in `entities/`, `controller/`, `routes/`
(profile values arrive as typed parameter structs); no `use crate::assets::` inside
`milp_planner/` or `entities/`; no `serde_json::Value` leaking out of `vtn.rs`.
All four greps pass. The GET /reports pass-through goes through the typed
`OadrReport` (known fields by name, everything else preserved verbatim via
serde flatten), so no `serde_json::Value` crosses `VtnPort`.

Two placements enforce the ring boundaries around planning and state:
`entities/asset_ledger.rs` holds the per-asset energy/cost/CO₂ ledger entry (a
domain type; `controller/monitor.rs` consumes it without reaching into
`state/`), and `simulator/plan_context.rs` holds the plan-cycle helpers that
need the concrete `SimState` (sim-snapshot clone, pending-PV-inject patch,
`build_asset_contexts`) — so `services/` touches the simulator only through
`SimulatorPort` ([[milp-planner]]).

File-size caps: 500 **production** lines in `VEN/src/`, 200 in `tasks/` —
non-blank lines excluding `#[cfg(test)]` blocks and test-only paths, exactly
what `ci/audits/audit_file_sizes.py` measures (the allowlist is empty and stays so). The audit
passes repo-wide; `tasks/planning.rs` sits near its cap and is the first
candidate for a split when it next grows.

The rationale for this ring shape is in [[hexagonal-refactoring]]; the two-speed runtime
behaviour of the rings is described in [[hems-planning]]. `.claude/CLAUDE.md`
§ven-architecture points to `docs/architecture/VEN_ARCHITECTURE.md` and the module
dependency graph as the canonical references.
