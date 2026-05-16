# OpenAdr-Lab — Application Documentation

> Audience: code reviewers and users. This document covers purpose, features, architecture, and operational guidance.

---

## Table of Contents

1. [Purpose & Overview](#1-purpose--overview)
2. [Feature Reference](#2-feature-reference)
   - 2.1 [Simulation Engine](#21-simulation-engine)
   - 2.2 [Energy Planning (MILP)](#22-energy-planning-milp)
   - 2.3 [Real-time Deviation Absorption](#23-real-time-deviation-absorption)
   - 2.4 [User Energy Requests](#24-user-energy-requests)
   - 2.5 [VTN Integration (OpenADR 3)](#25-vtn-integration-openadr-3)
   - 2.6 [Report Obligations](#26-report-obligations)
   - 2.7 [Flexibility Envelope](#27-flexibility-envelope)
   - 2.8 [Simulation Injection & Overrides](#28-simulation-injection--overrides)
   - 2.9 [Persistence & Recovery](#29-persistence--recovery)
   - 2.10 [Observability](#210-observability)
3. [HTTP API Reference](#3-http-api-reference)
4. [Architecture](#4-architecture)
   - 4.1 [Philosophy](#41-philosophy)
   - 4.2 [Deployment Topology](#42-deployment-topology)
   - 4.3 [Ring Map (Hexagonal Architecture)](#43-ring-map-hexagonal-architecture)
   - 4.4 [Module Responsibilities](#44-module-responsibilities)
   - 4.5 [Background Tasks](#45-background-tasks)
   - 4.6 [State Management & Locking](#46-state-management--locking)
   - 4.7 [Control Flow End-to-End](#47-control-flow-end-to-end)
5. [Configuration Reference](#5-configuration-reference)
6. [Deployment](#6-deployment)
7. [Testing](#7-testing)

---

## 1. Purpose & Overview

**OpenAdr-Lab** is a self-hosted laboratory platform for experimenting with demand-response (DR) automation under the **OpenADR 3.0** protocol. It simulates a residential or small-commercial energy site with multiple controllable assets (battery, EV, heat pump, PV, base load) managed by a Virtual End Node (VEN). A co-hosted Virtual Top Node (VTN), running the open-source **openleadr-rs** stack, issues programs, events, and pricing signals to which the VEN responds.

The platform has two primary audiences:

| Audience | Use |
|----------|-----|
| **Researchers / tinkerers** | Observe, experiment with, and tune DR algorithms without touching real hardware |
| **Protocol implementors** | Verify OpenADR 3 message flows, report obligations, and program/event parsing against a live VTN |

Three independent VEN instances run simultaneously (ven-1, ven-2, ven-3), each with its own asset profile, planner parameters, and state — enabling multi-VEN coordination experiments.

---

## 2. Feature Reference

### 2.1 Simulation Engine

Each VEN hosts a physics engine that advances asset states every simulation tick (default: 1 second).

**Simulated assets:**

| Asset | Physical model | Controllable |
|-------|---------------|--------------|
| Battery | SoC rate = `setpoint_kw / capacity_kwh`; round-trip efficiency losses | Yes — continuous setpoint (kW) |
| EV | SoC rate while plugged; V2G-capable `REM: NEW to me, make it optional in profile`; departure guard enforces target SoC | Yes — charge/discharge kW |
| Heat pump / Heater | Thermal mass ODE: `dT/dt = (T_amb − T) / τ + Q / C`; switching penalty `REM: not only for these assets` | Yes — ON/OFF duty cycle |
| PV | Irradiance-driven sinusoidal model with EMA smoothing; non-curtailable | No (read-only) |
| Base load | Fixed consumption profile; non-controllable | No (read-only) |
| Grid (virtual) | Aggregates all asset powers; clamps to VTN import/export limits | Via VTN limits |

`TODO: add a chapter for Assets and a table with extended properties, additional: simulation state manipulation api, forecast possibility, controllable tiers in detail, settings in profile, etc. especially document the shared physics between simulated forecast and planning with physics simulation`

The simulation can run in **headless mode** (pure physics) or with **sensor injection** (see §2.8) to override individual parameters.

### 2.2 Energy Planning (MILP)

A mixed-integer linear program (MILP) solved by **HiGHS** computes a 24-hour optimal energy schedule every planning interval (default: 5 minutes) or on demand.

**Inputs to the solver:**
- Current asset SoC / temperature / capability
- Tariff time series (import/export prices, CO₂ intensity)
- VTN capacity limits per slot
- Asset physics constraints (max charge rate, min SoC floor, comfort deadband)
- Active user energy requests (deadlines, energy budgets)
- Planner objective (cost minimisation, emissions, grid stress, or weighted multi-objective)

**Output — a Plan:**
- Per-asset power allocation (kW) for each 5-minute slot over the horizon (default `plan_step_s` = 300 s)
- Cost breakdown per slot (grid cost, battery cycling cost, discomfort penalty)
- Plan warnings (infeasibility, capacity breaches)
- Flexibility envelope per slot (how much more or less can be offered to the grid)

`TODO: add detailed explanation how to keep a plan a) as unfragmented as possible, b) as stable over time as possible for a reliable forecast report. State clearly if this goal is not implemented yet.`

**Two-phase lexicographic solve:**
1. **Phase 1 (MIP — cost minimisation)** — minimises economic cost only (import tariff, export revenue, battery cycling). No startup/ramp auxiliary variables; finds the optimal cost floor `c_star`.
2. **Phase 2 (MIP — friction minimisation)** — minimises operational friction (startup penalties, ramp costs, switching penalties, tier penalties) subject to the constraint `phase1_cost ≤ c_star + phase2_epsilon_eur` (default ε = 0.02 €). Phase 1's solution is used as the warm-start incumbent so Phase 2 immediately has a feasible integer point. Setting `phase2_epsilon_eur = 0` collapses to a single-phase solve.

The planner runs in a blocking Tokio thread to avoid starving the async runtime.

`explain how to configure phase 2. and how/why the optimization of phase 1 is kept untouched while defragmenting in phase 2. clearly state in case this goal in not implemented yet.`

**Acceptance gate:** A new plan is adopted only if its total cost is below a threshold relative to the current plan. Hard triggers (VTN rate change, capacity alert, user request, device deviation) bypass the gate.

`Explain the threshold decay.`

### 2.3 Real-time Deviation Absorption

Between planning cycles, a **two-tier control architecture** keeps the site on its MILP plan:

#### Tier 1 — Absorber (every tick, ~1 second)

The absorber corrects deviations between planned and actual grid power without triggering a full replan.

**Deviation computation:**
```
deviation_kw = actual_net_kw − planned_net_kw
```
Positive = site is importing more than planned; correction must reduce import (reduce heater, curtail EV, discharge battery).  
Negative = site is importing less than planned (e.g. PV spike); correction must increase import (charge battery, ramp EV up).

`Todo: curtail PV is not implemented yet but will be required for some openADR signals`

**Per-asset correction state machine:**

Each asset in the absorber configuration independently tracks a *correction overlay* (delta from the MILP setpoint):

```
Idle  ─── |deviation| > dead_band ──►  Correcting
           overlay = planned_sp + delta        │
                                               │ |deviation| ≤ dead_band
                                               │ for dead_band_clearing_ticks ticks
                                               ▼
                                          Settling
                                          (overlay ramps to 0 over 1 tick)
                                               │
                                               ▼
                                          Idle  (back to clean MILP setpoint)
```

The `dead_band_clearing_ticks` wait-gate prevents chattering: if the deviation momentarily dips inside the dead-band but then rises again, the settling counter resets to zero and the overlay is held.

`TODO: explain in more detail and with examples.`

**Constraints applied before each asset correction:**

| Constraint | Effect |
|------------|--------|
| Dead-band (`dead_band_kw`, default 0.1 kW) | Corrections smaller than this are ignored entirely |
| Asset priority order (profile-configured) | Assets are tried in priority order: lower number = first (default: battery 0, EV 1) |
| Headroom bounds | Battery: bounded by SoC vs. min-SoC floor; EV charge: bounded by curtailable setpoint (down) or remaining capacity-to-target (up); Heater: discrete step (off → mid → max) |
| Relay wear linger (`min_state_linger_s`) | Asset is skipped if fewer than `min_state_linger_s` seconds have passed since its last state change |
| EV departure guard (`ev_departure_guard_s`) | If an EV session is active, departure is within the guard window (ven-1 profile: 1800 s), AND the EV's current SoC is below its target, the absorber will NOT reduce EV charging (positive deviation). The guard does not apply when SoC ≥ target (already satisfied), when deviation is negative (surplus absorption — EV charging is always increased), or when no session is active |

**SSE telemetry:** When a correction is applied the absorber broadcasts a `PlannerEvent::CorrectionActive` SSE event with planned vs. actual net power and the correction magnitude. When the correction clears it emits `PlannerEvent::CorrectionCleared`. Events are deduplicated: a new SSE fires only when the total correction changes by > 0.2 kW.

`TODO: explain the purpose/use case of those events`

#### Tier 2 — DeviceDeviation escalation

After the absorber runs, the uncovered `residual_kw` (what could not be absorbed) is accumulated tick-by-tick. When the residual exceeds the dead-band for `deviation_trigger_ticks` consecutive ticks, a `DeviceDeviation` trigger is sent to the planning loop, which wakes up immediately and runs a full MILP replan. This ensures sustained under- or over-performance that the absorber cannot handle is corrected at the planning level.

**Summary:** Tier 1 handles fast, transient deviations within seconds at zero planning cost. Tier 2 escalates only when Tier 1 is genuinely exhausted over multiple ticks.

### 2.4 User Energy Requests

Users can request energy services with deadline constraints:

| Session type | What it does |
|--------------|-------------|
| **EV charge** | Charge EV to a target SoC by a specified departure time |
| **EV discharge (V2G)** | Export from EV battery to grid/home up to a budget |
| **Heater** | Reach and hold a temperature target |
| **Shiftable load** | Schedule a fixed-energy task (e.g., dishwasher) within a window |
| **Baseline override** | Manually shift the expected base load profile for a period |

`TODO: heat pump and air condition should be appended to the shiftable loads. if you disagree, explain why.`

Requests are tracked through states: `Pending → Scheduled → Active → Completed / Failed`.  
The planner integrates user requests as hard constraints (must-meet) or soft constraints (best-effort) depending on configuration.

**Opportunistic EV charging** can be toggled independently: when on, any surplus generation charges the EV even without an explicit user request.

`TODO: Explain in process schematic how they are competing/not competing with deviation absorber. TODO add opportunistic load to Heater and Boiler as well and create a priority list or use the one from the deviation absorber, if that makes sense` 

### 2.5 VTN Integration (OpenADR 3)

The VEN polls the VTN continuously over authenticated HTTPS.

**Polling loops:**

| Loop | Default interval | What it fetches |
|------|-----------------|----------------|
| Programs | 30 s (code default); 300 s in Docker Compose | Active demand-response programs |
| Events | 30 s | Price, GHG, curtailment, and alert signals |
| Reports | 60 s | Confirmation of received reports |

`REM: I assume programs will hardly change, is this correct? in any case, make those interval configurable in config files`

**Authentication:** OAuth 2.0 client-credentials flow. The token is cached with a 60-second safety margin and automatically refreshed on expiry or a 401 response.

`TODO: in order to reduce VEN to VTN traffic, make the token update interval configurable or even add possibility to disable and fetch on 401.`

**Signal parsing:**
- `PRICE` signals → import/export tariff time series
- `GHG` signals → CO₂ intensity series (feeds multi-objective planner)
- `SIMPLE` / `LOAD_DISPATCH` → curtailment targets
- `ALERT` → triggers an immediate replan

A rate-change event (new pricing) immediately triggers a plan recomputation via a `tokio::sync::watch` channel, without waiting for the next periodic interval.

`TODO: Explain in detail the grade of implementation of those signals and their function.`

### 2.6 Report Obligations

When a VTN program specifies reporting requirements, the VEN fulfils them automatically.

**Flow:**
1. VTN program carries `OadrReportObligation` (interval, attributes, baseline type)
2. The obligation service tracks due timestamps
3. When due, the reporter computes:
   - Actual asset power averages over the interval
   - Baseline comparison (if required)
   - Accumulated energy, cost, CO₂
4. The report is POSTed to the VTN `/reports` endpoint
5. The obligation is marked fulfilled

### 2.7 Flexibility Envelope

At any moment the VEN can report its site-level flexibility to the VTN:

- **`up_kw`** — how much grid import can be reduced right now (demand reduction)
- **`down_kw`** — how much grid import can be increased (load increase, e.g., charge EV faster)
- **`up_duration_s`** / **`down_duration_s`** — estimated duration before the constraint binds

This is used by the VTN operator to understand available DR capacity across VENs.

`TODO: explain in detail, how those numbers are calculated and where they are calculated.`

### 2.8 Simulation Injection & Overrides

For experimentation, any simulated physics parameter can be overridden via the API without restarting:

| Injection mode | Fields | Behaviour |
|---------------|--------|-----------|
| **A — one-shot** | `battery_soc`, `ev_soc`, `heater_temp_c` | Applied once to physics state, then cleared automatically on the next tick |
| **B — frozen + EMA return** | `pv_irradiance`, `base_load_kw` | Value is held constant while the override is active. On release, the physics model blends back toward the natural value exponentially (EMA) |
| **C — frozen + snap** | `ev_plugged`, `ev_soc_target`, `heater_setpoint_c`, `heater_temp_min/max_c`, `ambient_temp_c`, `grid_import/export_limit_kw` | Value is held constant while active; on release snaps immediately to the profile default |
| **D — planning only** | `pv_plan_kw` | Seen by the MILP planner only; has no effect on the physics simulator |

`TODO: explain why C is needed or could be dropped and explain whether it is used in UI and how`
`TODO: explain why pv_plan_kw is only for milp planner. will it not show up on the ui forecast timeline? if so, why?`

Supported overrides include: PV irradiance, base-load power, battery SoC, EV SoC, ambient temperature, grid import/export limits, and asset setpoints.

A `POST /sim/inject/reset` clears all active overrides simultaneously.

### 2.9 Persistence & Recovery

VEN state is persisted in two separate mechanisms:

- **Sim physics state** (asset SoCs, temperatures, setpoints, history) is written to `/data/state.json` inside the tick loop at every `persist_every_s` ticks (ven-1 profile: 15 s). This happens in Phase 8 of the tick and also on graceful shutdown (Ctrl-C).
- **AppState** (polled programs, events, sensor snapshots) is written separately by the `state_persist` background task, if `PERSIST_PATH` is configured.

On restart, the physics state is reloaded and simulation resumes from where it left off. Profile parameters (physics constants, planner weights) are recomputed from the YAML profile file and merged back into the loaded state, so config changes take effect cleanly without manual state migration.

`TODO: a required feature is to also persist plans in for restart to have reliable VTN forcast reports.`
### 2.10 Observability

| Signal | Endpoint / mechanism |
|--------|---------------------|
| Health check | `GET /health` — returns 200 OK with VEN name and uptime |
| Prometheus metrics | `GET /metrics` — counters and gauges for ticks, solves, reports |
| Controller trace | `GET /trace/events` (SSE stream) and `GET /trace/history` — last 500 controller events with timestamps, event type, and payload |
| Structured logs | JSON tracing output via `tracing-subscriber`; level controlled by `RUST_LOG` |
| Planner progress SSE | Server-Sent Events pushed during MILP solve (solving started, phase progress, plan adopted) |

`TODO: also plan a asset power log with configurable window size and window agregation (mean) and length.`

---

## 3. HTTP API Reference

All endpoints are served by each VEN on its configured port (default: `8211` for ven-1, `8212` for ven-2, `8213` for ven-3). CORS is open to all origins.

### System

| Method | Path | Description |
|--------|------|-------------|
| `GET` | `/health` | Health check |
| `GET` | `/metrics` | Prometheus metrics |

### OpenADR State

| Method | Path | Description |
|--------|------|-------------|
| `GET` | `/programs` | Currently polled VTN programs |
| `GET` | `/events` | Currently polled VTN events |
| `GET` | `/sensors` | Current sensor snapshot |
| `POST` | `/sensors` | Push external sensor reading |

### Simulation

| Method | Path | Description |
|--------|------|-------------|
| `GET` | `/sim` | Full simulator state snapshot |
| `GET` | `/sim/schema` | UI control descriptors per asset |
| `POST` | `/sim/reset/:asset_id` | Reset asset physics to profile defaults |
| `PUT` | `/sim/config/battery` | Update battery physics config |
| `GET` | `/sim/inject` | Current injection overrides |
| `POST` | `/sim/inject` | Apply injection override(s) |
| `POST` | `/sim/inject/reset` | Clear all injection overrides |

### Planning & HEMS

| Method | Path | Description |
|--------|------|-------------|
| `GET` | `/plan` | Current active plan (slots, allocations, cost) |
| `PUT` | `/plan/objective` | Change planner objective (cost / emissions / grid) |
| `POST` | `/plan/trigger` | Force immediate replan |
| `GET` | `/plan/events` | SSE stream of planner lifecycle events (solve start/progress/adopted) |
| `GET` | `/tariffs` | Current tariff time series |
| `GET` | `/capacity` | VTN-imposed site capacity limits |
| `GET` | `/obligations` | Active report obligations |
| `GET` | `/flexibility` | Site flexibility envelope |
| `GET` | `/ledger` | Cumulative asset energy / cost / CO₂ |

### User Requests & Device Sessions

| Method | Path | Description |
|--------|------|-------------|
| `GET` | `/user-requests` | List all user requests |
| `POST` | `/user-requests` | Create new energy request |
| `DELETE` | `/user-requests/:id` | Cancel a user request |
| `GET` | `/ev-session` | Current EV session state |
| `POST` | `/ev-session` | Start a new EV session |
| `DELETE` | `/ev-session` | End the current EV session |
| `GET` | `/ev-settings` | Get opportunistic EV charging settings |
| `PUT` | `/ev-settings` | Toggle opportunistic EV charging |
| `GET` | `/heater-target` | Get current heater temperature target |
| `POST` | `/heater-target` | Set heater temperature target |
| `DELETE` | `/heater-target` | Clear heater temperature target |
| `GET` | `/shiftable-loads` | List shiftable load tasks |
| `POST` | `/shiftable-loads` | Schedule a shiftable load |
| `DELETE` | `/shiftable-loads/:id` | Cancel a shiftable load task |
| `GET` | `/baseline-override` | Current baseline override |
| `POST` | `/baseline-override` | Set a baseline override period |
| `DELETE` | `/baseline-override` | Clear baseline override |

### Assets

| Method | Path | Description |
|--------|------|-------------|
| `GET` | `/forecast/:asset_id` | Forward power forecast for asset |
| `GET` | `/history/:asset_id` | Historical power / state trace |
| `GET` | `/capability/:asset_id` | Current max import / export kW |

### Timeline

| Method | Path | Description |
|--------|------|-------------|
| `GET` | `/timeline/all` | Planned trajectories for all assets |
| `GET` | `/timeline/:asset_id` | Planned trajectory for one asset |

### Reports

| Method | Path | Description |
|--------|------|-------------|
| `GET` | `/reports` | Aggregated report list |
| `POST` | `/reports` | Create and submit a report |
| `PUT` | `/reports/:id` | Update a report |

### Diagnostics

| Method | Path | Description |
|--------|------|-------------|
| `GET` | `/trace/events` | SSE stream of controller events |
| `GET` | `/trace/history` | Last 500 controller events |

---

## 4. Architecture

### 4.1 Philosophy

The VEN backend follows **Hexagonal Architecture** (Ports and Adapters) combined with **Clean Architecture** layering. The core rule is:

> **Inner rings never import outer rings.**

This keeps domain logic free of infrastructure concerns and makes all external dependencies (VTN, solver, simulator) replaceable via traits (ports). The UI is a thin read/write layer on top of the HTTP API with no business logic.

**Key design decisions:**

- **Snapshot-and-release locking**: A lock is acquired, data is cloned, the lock is dropped, then computation (including the potentially multi-second MILP solve) proceeds on the snapshot. This prevents lock contention between the real-time tick loop and the planning loop.
- **No DTO normalization**: Field names from the OpenADR spec flow unchanged through all layers (backend → BFF → UI), reducing vocabulary divergence and debugging overhead.
- **Unit suffixes on physical quantities**: Every variable or field representing a physical quantity carries its unit as a suffix (e.g., `power_kw`, `energy_kwh`, `soc_pct`, `temperature_c`). This is a hard convention.
- **File size limits**: No `VEN/src/` file exceeds 500 lines; `tasks/` files must stay below 200 lines. This enforces single-responsibility at the file level.

### 4.2 Deployment Topology

```
┌─────────────────────────────────────────────────────────┐
│  Raspberry Pi 4  (Docker Compose — Pi4-Server)          │
│                                                         │
│  ┌─────────────────────────────────────────────────┐    │
│  │  VTN Stack                                      │    │
│  │  openleadr-rs (Rust)  ←→  PostgreSQL            │    │
│  │  BFF Proxy            ←→  VTN UI (React)        │    │
│  └─────────────────────────────────────────────────┘    │
│            ↑  /programs  /events  /reports              │
│            │  OAuth 2.0  (HTTP)                         │
│  ┌─────────┴───────────────────────────────────────┐    │
│  │  VEN Stack (×3 independent instances)           │    │
│  │                                                 │    │
│  │  ven-1 :8211   ven-2 :8212   ven-3 :8213       │    │
│  │  (Rust / Axum)                                  │    │
│  │                                                 │    │
│  │  VEN UI (React)  :8214                          │    │
│  └─────────────────────────────────────────────────┘    │
└─────────────────────────────────────────────────────────┘
```

Network: a shared Docker bridge `vtn_openadr-net` connects all containers. State files are volume-mounted at `/data/state.json` per VEN.

### 4.3 Ring Map (Hexagonal Architecture)

```
╔══════════════════════════════════════════════════════╗
║  Adapters  (routes/, tasks/)                         ║
║  ┌────────────────────────────────────────────────┐  ║
║  │  Application  (services/)                      │  ║
║  │  ┌──────────────────────────────────────────┐  │  ║
║  │  │  Domain  (entities/, controller/)        │  │  ║
║  │  │  ┌────────────────────────────────────┐  │  │  ║
║  │  │  │  Infra  (assets/, simulator/,      │  │  │  ║
║  │  │  │          vtn.rs, milp_planner/)    │  │  │  ║
║  │  │  └────────────────────────────────────┘  │  │  ║
║  │  └──────────────────────────────────────────┘  │  ║
║  └────────────────────────────────────────────────┘  ║
╚══════════════════════════════════════════════════════╝
```

**Port obligations (must use traits, never concrete types):**

| Port | Caller | Implementor |
|------|--------|-------------|
| `SimulatorPort` | `services/` | `simulator/` |
| `SolverPort` | `services/` | `controller/milp_planner/` |
| `VtnPort` | `services/` | `vtn.rs` |
| `AssetMilpContext` | `milp_planner/` | per-asset impls in `assets/` |

**Verifiable invariants** (must be empty before any PR):
```sh
grep -r "use crate::profile" VEN/src/entities VEN/src/controller VEN/src/routes
grep -r "use crate::assets::" VEN/src/controller/milp_planner
grep "serde_json::Value" VEN/src/vtn.rs
```

### 4.4 Module Responsibilities

#### `src/main.rs`
Entry point. Constructs `AppState` (all runtime state), `AppCtx` (shared application context — cloned by Axum per request), wires the Axum router, and spawns seven background task loops.

#### `src/state.rs` — AppState
Thread-safe container with three independently locked sections:

| Section | Contents |
|---------|----------|
| `PollingState` | Programs, events, reports from VTN polls |
| `ControllerSimState` | Asset snapshots, simulation state, injection overrides, controller trace ring buffer |
| `HemsState` | Active plan, tariffs, capacity limits, asset ledger, user requests, device sessions |

**Invariant:** No function may hold more than one section lock at a time.

#### `src/entities/` — Domain types
Pure data: no I/O, no async, no framework imports.

- `plan.rs` — `Plan`, `PlanTimeSlot`, `AssetAllocation`, `CostBreakdown`, `PlanWarning`
- `asset.rs` — `AssetType`, `PlanTrigger`, `CompletionPolicy`, `UserRequestMode`
- `asset_params.rs` — `AssetParams` (per-type physics configuration)
- `capacity.rs` — `OadrCapacityState`, `OadrReportObligation`
- `device_session.rs` — `EvSession`, `HeaterTarget`, `ShiftableLoad`, `BaselineOverride`
- `user_request.rs` — `UserRequest`, `UserRequestStatus`, `SessionType`
- `tariff_snapshot.rs` — `TariffTimeSeries` (import/export prices, CO₂)
- `planner_params.rs` — `PlannerObjective`, `PlannerParams`

#### `src/assets/` — Asset physics
One file per asset type. Each implements `capability()` (current kW bounds) and `step()` (ODE advance). Physics constants are injected as typed `*Params` structs constructed in the infra layer — no direct profile imports.

#### `src/simulator/` — Simulation engine
Manages the collection of `AssetEntry` objects; advances physics each tick; maintains 3600-row per-asset history ring buffers (`VecDeque`); persists state to disk.

`REM: History (like forecast) should not be part of simulator but should be part of asset. correct me if you think otherwise. There might be two types of simulator needed: device state simulator, instead of real actors and sensors (purpose: replace real devices) and device forecast simulator, which is required by planning (purpose: accurate planning). the two simulations should be separated as they have different purpose.`

#### `src/controller/` — Control domain
The largest domain package. Sub-modules:

| Sub-module | Responsibility |
|------------|---------------|
| `milp_planner/` | HiGHS MILP formulation, two-phase solve, result translation |
| `dispatcher.rs` | Translate plan allocations → per-asset setpoints each tick |
| `absorber.rs` | Tier-1 real-time deviation correction; returns residual for Tier-2 escalation |
| `envelope.rs` | Compute site flexibility envelope |
| `timeline.rs` | Extrapolate per-asset planned trajectories (SoC, temperature, power) |
| `reporter.rs` | Compute interval report payloads |
| `trace.rs` | `ControllerTrace` — 500-entry ring buffer of controller events |
| `openadr_interface.rs` | Parse VTN event signals into domain types |
| `vtn_port.rs` | `VtnPort` trait |
| `simulator_port.rs` | `SimulatorPort` trait + `SimSnapshot` / `GridSnapshot` types |

`REM: I miss the opportunistic load. I assume, it is in the asset since currently only EV is implemented. Maybe it needs common functionality in a separate source file.`

#### `src/services/` — Application services
Stateless orchestration; calls ports, updates state.

| Service | Responsibility |
|---------|---------------|
| `planning.rs` | Acceptance gate logic; plan adoption decisions |
| `hems.rs` | EV session lifecycle; heater target updates |
| `user_request.rs` | Create / validate / transition user requests |
| `obligation.rs` | Identify due obligations; mark fulfilled |

#### `src/tasks/` — Background async loops
Each loop is a `tokio::spawn`'d future that runs forever:

| Task file | What it does |
|-----------|-------------|
| `poll_events.rs` | Polls VTN events; parses signals; triggers replan on rate change |
| `poll_programs.rs` | Polls VTN programs |
| `poll_reports.rs` | Polls VTN report confirmations |
| `sim_tick/tick.rs` | Main simulation + dispatch + absorption loop (1 s default) |
| `planning.rs` | MILP planning loop (5 min default; immediate on hard trigger) |
| `obligation.rs` | Report obligation fulfillment loop |
| `state_persist.rs` | Periodic state-to-disk serialisation |

#### `src/routes/` — HTTP adapters
Axum handlers. Extract state from `AppCtx`, delegate to services or state accessors, return JSON. No business logic. Modules mirror the API surface (system, events, sim, hems, assets, timeline, reports, trace).

#### `src/vtn.rs` — VTN HTTP client
OAuth2 bearer-authenticated `reqwest` client. Implements `VtnPort`. Token cached in `Arc<RwLock<Option<Token>>>` with auto-refresh.

#### `src/profile.rs` — Configuration / YAML profile
Loads asset physics parameters, planner weights, poll intervals, and grid limits from a YAML file at startup. Profile values are injected into inner layers as typed structs — never imported directly by domain code.

#### `src/common/` — Shared utilities
`TimeSeries` with `Linear` / `Step` interpolation; `time_weighted_mean`; `Aggregation` enum.

### 4.5 Background Tasks

Seven `tokio::spawn` loops run concurrently:

```
┌────────────┐   ┌──────────────┐   ┌────────────┐
│poll_events │   │poll_programs │   │poll_reports│
│  (30 s)    │   │(30 s default)│   │  (60 s)    │
└─────┬──────┘   └──────────────┘   └────────────┘
      │  rate-change / alert signal
      │  → watch channel send
      ▼
┌─────────────────────────────────────────────────┐
│  planning loop  (300 s periodic + hard trigger) │
│   1. snapshot state (clone, drop lock)          │
│   2. solve MILP in blocking thread              │
│   3. acceptance gate                            │
│   4. adopt plan → write HemsState               │
└────────────────────┬────────────────────────────┘
                     │ new plan written
                     ▼
┌─────────────────────────────────────────────────┐
│  sim_tick loop  (1 s)                           │
│   Phase 1: apply one-shot injections            │
│   Phase 2: build setpoints (dispatcher)         │
│   Phase 3: Tier-1 absorber → correct deviation  │
│   Phase 4: physics tick (step all assets)       │
│   Phase 5: update snapshots, history, envelope  │
│   Phase 6: Tier-2 residual accumulation →       │
│            DeviceDeviation trigger if sustained │
│   Phase 7: measurement reports (if due)         │
│   Phase 8: persist sim state to disk            │
└─────────────────────────────────────────────────┘
      ┌──────────────┐
      │obligation    │
      │check loop    │  → compute report → POST VTN
      └──────────────┘
      ┌──────────────┐
      │state_persist │  → serialize AppState (events,
      │ (if enabled) │    programs, sensor) to JSON
      └──────────────┘
```

Note: `state_persist` is only spawned when `PERSIST_PATH` is set. Sim physics state is persisted inside the tick loop (Phase 8), using the same directory.

### 4.6 State Management & Locking

`AppState` wraps three `Arc<RwLock<…>>` sections. The locking protocol is strict:

1. **Never hold two locks simultaneously** — prevents deadlock by design
2. **Snapshot-and-release** — for expensive operations (MILP solve, report computation):
   ```
   let snapshot = {
       let guard = state.read().await;
       guard.data.clone()
   }; // lock dropped here
   expensive_compute(snapshot);
   ```
3. **Read-heavy** — `RwLock` allows many concurrent readers; writers are rare (plan adoption, tick update)

### 4.7 Control Flow End-to-End

A complete demand-response cycle:

```
VTN issues new PRICE event
        │
        ▼
poll_events detects rate change
        │
        ├── parses price signals → HemsState.planned_tariffs
        └── sends RateChange watch signal → planning loop wakes immediately
                │
                ▼
        planning loop snapshots HemsState + SimState (clone, drop lock)
                │
                ▼
        MILP solve (HiGHS, blocking Tokio thread)
        [Phase 1: MIP (cost minimisation) → Phase 2: MIP (friction minimisation)]
                │
                ▼
        acceptance_gate() evaluates cost delta vs. current plan
                │
                ▼ (accepted)
        new Plan written to HemsState
                │
                ▼
        sim_tick (every 1 s):
          Phase 2: dispatcher → planned setpoints for current slot
          Phase 3: Tier-1 absorber → correct deviations
                    │ residual uncorrectable?
                    ▼ (sustained N ticks)
          Phase 6: DeviceDeviation watch signal → planning loop wakes
          Phase 7/8: measurement reports + persist sim state
                │
                ▼
        obligation loop:
          due obligations → reporter.compute() → POST /reports to VTN
```

---

## 5. Configuration Reference

### Environment Variables

The code default for `LISTEN_ADDR` is `0.0.0.0:8080`. Docker Compose maps external ports (8211/8212/8213) to the container's port 8080.

| Variable | Code default | Docker Compose (ven-1) | Description |
|----------|-------------|------------------------|-------------|
| `LISTEN_ADDR` | `0.0.0.0:8080` | `0.0.0.0:8080` | Axum bind address |
| `VTN_BASE_URL` | — (required) | `http://vtn:3000` | VTN base URL |
| `CLIENT_ID` | — (required) | `ven-1` | OAuth2 client ID |
| `CLIENT_SECRET` | — (required) | `ven-1` | OAuth2 client secret |
| `VEN_NAME` | `ven-1` | `ven-1` | VEN identifier (sent in reports) |
| `PROFILE_PATH` | unset | `/config/profile.yaml` | Asset/planner YAML profile |
| `PERSIST_PATH` | unset | `/data/state.json` | State persistence file (disables persistence if unset) |
| `POLL_EVENTS_SECS` | `30` | `30` | VTN event poll interval (s) |
| `POLL_PROGRAMS_SECS` | `30` | `300` | VTN program poll interval (s) |
| `POLL_REPORTS_SECS` | `60` | unset (uses default) | VTN report poll interval (s) |
| `RUST_LOG` | `info` | `info` | Tracing log level |

### Profile YAML (key sections)

Profile files live in `VEN/profiles/` (one per VEN). Below is a representative excerpt from `ven-1.yaml`:

```yaml
assets:
  - type: ev
    id: ev
    max_charge_kw: 7.4
    initial_soc: 0.40
    battery_kwh: 60.0
    soc_target: 0.80
  - type: pv
    id: pv
    rated_kw: 8.0
  - type: battery
    id: battery
    capacity_kwh: 10.0
    max_charge_kw: 5.0
    max_discharge_kw: 5.0
    initial_soc: 0.50
    round_trip_efficiency: 0.92
    min_soc: 0.10
  - type: base_load
    id: base_load
    baseline_kw: 0.4

simulator:
  tick_s: 1
  persist_every_s: 15
  report_interval_s: 60

planner:
  plan_adoption_threshold_eur: 0.20
  replan_interval_s: 300
  deviation_trigger_ticks: 120

absorber:
  enabled: true
  dead_band_kw: 0.1
  dead_band_clearing_ticks: 1
  assets:
    - id: battery
      priority: 0
      min_state_linger_s: 0
    - id: ev
      priority: 1
      min_state_linger_s: 0
      ev_departure_guard_s: 1800
```

Note: not all VENs carry all asset types. VEN-1 has EV + PV + battery + base load (no heater). Other VEN profiles vary. Assets not present in the profile are simply absent from the simulation.

---

## 6. Deployment

### Running on Pi4-Server

All Docker operations run via SSH on the Pi4-Server in `/srv/docker/openadr_lab`.

**Start the full stack:**
```sh
ssh pi4-server
cd /srv/docker/openadr_lab
docker compose up -d
```

**View VEN logs:**
```sh
docker compose logs -f ven-1
```

**Rebuild after code change:**
```sh
docker compose build ven
docker compose up -d ven-1 ven-2 ven-3
```

**Never stop containers outside this project** — other productive containers share the host.

### Local Rust Development (WSL)

For local `cargo check` and unit tests (no HiGHS integration):
```sh
wsl cargo check          # inside VEN/
wsl cargo test           # unit tests only
```

For full integration tests including HiGHS, use the Pi4-Server Docker stack.

### VEN UI

The React UI is served from the same Docker Compose stack on port 8214. It connects to all three VEN instances and provides:
- Per-asset control sliders and switches
- Real-time plan and timeline visualisation
- Event / program monitoring
- Ledger and cost tracking
- Controller trace diagnostics

---

## 7. Testing

### BDD Integration Tests (Behave + Playwright)

Located in `tests/features/`. Run against a live Docker Compose stack.

**Coverage areas:**
- VTN authentication and health
- VEN event and program polling
- Enrollment lifecycle
- Asset simulation physics (EV, heater, battery)
- User request creation and completion
- Timeline, reporting, and obligation fulfilment
- UI end-to-end scenarios (Playwright)

**Running:**
```sh
cd tests
behave features/ven_polling.feature    # single feature
behave                                  # all features
```

### Rust Unit Tests

```sh
wsl cargo test -p ven   # inside VEN/
```

Unit test coverage includes:
- Physics models (battery SoC, heater thermal ODE)
- MILP solver formulation and result parsing
- Planner acceptance gate
- Report obligation scheduling
- Controller absorber dead-band logic

### Architecture Invariant Checks

Run before any VEN PR to verify ring-map compliance:

```sh
# Must return empty — no profile imports in inner rings
grep -r "use crate::profile" VEN/src/entities VEN/src/controller VEN/src/routes

# Must return empty — no concrete asset imports in MILP planner
grep -r "use crate::assets::" VEN/src/controller/milp_planner

# Must return empty — no raw JSON Values in VTN client
grep "serde_json::Value" VEN/src/vtn.rs
```
