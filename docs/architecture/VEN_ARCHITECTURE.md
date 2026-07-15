# VEN Architecture

**Authoritative reference for VEN components, data flows, planning algorithm, simulator, and API.**
Domain vocabulary is in [docs/REQUIREMENTS.md](../REQUIREMENTS.md).
VTN/BFF architecture is in [docs/architecture/VTN_ARCHITECTURE.md](VTN_ARCHITECTURE.md).

---

## 1. Component Overview

The VEN is a Rust/Axum application. It runs as a Docker container and communicates with the VTN
via the OpenADR 3 REST API. Internally it has two major subsystems: the **HEMS Controller**
(planner-based, multi-step scheduling) and the **Simulator** (physics-based device models).

```
┌──────────────────────────────────────────────────────────────────────────────┐
│                              VEN Container                                   │
│                                                                              │
│  ┌─────────────────────────────────────────────────────────────────────────┐ │
│  │                         HEMS Controller                                 │ │
│  │                                                                         │ │
│  │  ┌──────────────┐   ┌──────────────┐   ┌───────────────────────────┐    │ │
│  │  │  OpenADR     │   │    User      │   │     Monitor               │    │ │
│  │  │  Interface   │   │   Request    │   │     (Deviation Detector)  │    │ │
│  │  └──────┬───────┘   └──────┬───────┘   └────────────┬──────────────┘    │ │
│  │         │                  │                        │                   │ │
│  │         └──────────────────┤◄───────────────────────┘                   │ │
│  │                            ▼                                            │ │
│  │                   ┌──────────────┐                                      │ │
│  │                   │   Planner    │ ← PlanTrigger channel                │ │
│  │                   └──────┬───────┘                                      │ │
│  │                          ▼                                              │ │
│  │                   ┌──────────────┐                                      │ │
│  │                   │  Dispatcher  │  (1 s tick)                          │ │
│  │                   └──────────────┘                                      │ │
│  └─────────────────────────────────────────────────────────────────────────┘ │
│                                                                              │
│  ┌─────────────────────────────────────────────────────────────────────────┐ │
│  │                     Asset Layer  (Vec<AssetEntry>)                      │ │
│  │                                                                         │ │
│  │  ┌──────────────────────────────────────────────────────────────────┐   │ │
│  │  │  Asset: step() · capability() · simulate_forward()                │   │ │
│  │  │  AssetHandle: id() · current_state() · history(window)            │   │ │
│  │  └──────────────────────────────────────────────────────────────────┘   │ │
│  │          ▲                                           ▲                  │ │
│  │  ┌───────┴────────┐                       ┌──────────┴──────────┐       │ │
│  │  │  AssetConfig   │  ← physics models     │  MeasuredAsset      │       │ │
│  │  │ PV · Battery   │    per asset type     │  (future: real HW,  │       │ │
│  │  │ EV · Heater    │    (implemented)      │   not yet built)    │       │ │
│  │  │ BaseLoad       │                       └─────────────────────┘       │ │
│  │  └───────┬────────┘                                                     │ │
│  │          │ UI only                                                      │ │
│  │  ┌───────▼────────┐                                                     │ │
│  │  │ /sim endpoints │  ← simulation params, overrides, schema, reset      │ │
│  │  └────────────────┘                                                     │ │
│  └─────────────────────────────────────────────────────────────────────────┘ │
│                                                                              │
│  REST API (Axum, port 8080 internal / 821x host)                             │
└──────────────────────────────────────────────────────────────────────────────┘
                        │
                        │ OpenADR 3 REST (OAuth2 + polling, default 30 s)
                        ▼
                   ┌──────────┐
                   │   VTN    │
                   └──────────┘
```

**Source layout (current):**
```
VEN/src/
  main.rs              — startup, task spawning; routes registered in routes/mod.rs (§4)
  routes/              — HTTP handlers, one module per resource (adapter ring)
  tasks/                — background loops (sim_tick, planning, poll_*, obligation) (adapter ring)
  services/            — planning/user-request/obligation application logic
  controller/          — dispatcher, monitor, openadr_interface, milp_planner, reporter, timeline, envelope, trace
  entities/            — asset, capacity, device_session, plan, tariff_snapshot, site_meter, user_request
  assets/              — Asset trait implementations (Battery, EvCharger, Heater, PvInverter, BaseLoad)
  simulator/           — SimState, persist, power_model
  reactor/             — REMOVED (see §3.3)
```

See `docs/BACKLOG.md §Refactoring` for any pending layout migrations.

---

## 2. HEMS Controller

### 2.1 Components & Responsibilities

| Component | Module | Cycle / Trigger | Owns |
|---|---|---|---|
| **OpenADR Interface** | `controller/openadr_interface` | `POLL_EVENTS_SECS` poll (default 30 s, env-configurable) + event-driven | `TariffSnapshot` / `TariffTimeSeries`, `OadrCapacityState`, `OadrReportObligation` |
| **User Request Manager** | `controller/user_request` | Event-driven (API call) | `UserRequest`, `EvSession` / `HeaterTarget` / `ShiftableLoad` |
| **Monitor** | `controller/monitor` | 1 s tick (`tasks/sim_tick`) | Per-asset energy/cost/CO₂ ledger (`state::AssetLedgerEntry`) |
| **Planner** | `controller/milp_planner`, reached via `SolverPort` | Watch channel + `replan_interval_s` periodic (default 300 s, profile-configurable) | `Plan`, `FlexibilityEnvelope`s, `PlanWarning`s |
| **Dispatcher** | `controller/dispatcher` | 1 s tick (`tasks/sim_tick`) | Per-asset setpoints written to the simulator |
| **Entities** | `entities/` | Shared state | `Plan`, `TariffSnapshot`, `UserRequest`, `EvSession` / `HeaterTarget` / `ShiftableLoad` |

#### OpenADR Interface

Translates between VTN REST JSON and the internal domain model. The only component that
knows about OpenADR HTTP, OAuth, and event payload formats. Transport lives in `vtn.rs`
behind the `VtnPort` trait; parsing is pure functions in `controller/openadr_interface.rs`.

**VTN → internal translation:**

| OpenADR EventType | Internal target | Status |
|---|---|---|
| `PRICE` / `EXPORT_PRICE` | `TariffSnapshot.import_tariff_eur_kwh` / `.export_tariff_eur_kwh` | ✅ implemented (supports looping daily-price events, e.g. `duration: P9999Y`) |
| `GHG` | `TariffSnapshot.co2_g_kwh` | ✅ implemented |
| `IMPORT_CAPACITY_LIMIT` / `EXPORT_CAPACITY_LIMIT` | `OadrCapacityState.import_limit_kw` / `.export_limit_kw` (strictest active event wins) | ✅ implemented |
| `IMPORT_CAPACITY_SUBSCRIPTION` / `IMPORT_CAPACITY_RESERVATION` | `OadrCapacityState.import_subscription_kw` / `.import_reservation_kw` | ✅ implemented |
| `EXPORT_CAPACITY_SUBSCRIPTION` / `EXPORT_CAPACITY_RESERVATION` | — | ❌ **GAP** (`docs/reference/TECHNICAL_DEBTS.md` R-14) — not parsed; `OadrCapacityState` has no export-side subscription/reservation fields at all |
| `ALERT_GRID_EMERGENCY` / `ALERT_BLACK_START` / `ALERT_FLEX_ALERT` | `PlanTrigger::Alert` (defined) → planner shed/import-limit enforcement | ❌ **GAP** (cert backlog BL-04) — `PlanTrigger::Alert` is defined but never sent by any code path; every detected event/rate/capacity change fires `PlanTrigger::RateChange` instead. The shed/import-limit enforcement itself is also unimplemented. |
| `DISPATCH_SETPOINT` | — | ❌ **GAP** (`docs/reference/TECHNICAL_DEBTS.md` R-13) — no code path parses this payload; it survives only as a dead field on the unreferenced `OadrEventCache` struct (`entities/capacity.rs`) |
| `CHARGE_STATE_SETPOINT` | `EvSession` create/modify with target SoC | ❌ **GAP** (cert backlog BL-06) — not yet implemented |

**Internal → VTN report generation** (`controller/reporter.rs`):

| Report obligation | Source | Status |
|---|---|---|
| `USAGE` | Time-weighted mean of net site import power over the obligation interval (`TimeSeries::resample_uniform`) | ✅ implemented |
| `STORAGE_CHARGE_LEVEL` | Point-in-time SoC (EV/battery) sampled at each obligation interval end | ✅ implemented |
| `OPERATING_STATE` | Hardcoded `"ACTIVE"` | 🟡 partial — the `DeviceResponsiveness` enum this should derive from is unreferenced |
| `IMPORT_CAPACITY_RESERVATION` / `EXPORT_CAPACITY_RESERVATION` | Live `SiteFlexibilityEnvelope` up/down kW | ✅ implemented |
| `DEMAND` | — | ❌ not built |
| `USAGE_FORECAST` | — | ❌ **GAP** (`docs/reference/TECHNICAL_DEBTS.md` R-15) — never built. The MILP already computes the exact per-slot forecast internally (`planned_state_by_asset`, used today only by `/timeline`) — it is simply never turned into a report. `reportDescriptor.historical` is never parsed, so the VEN cannot distinguish a forecast request from a historical one. |

#### User Request Manager

Translates user-facing energy requests (from `POST /user-requests`) into device-specific
session types (`EvSession`, `HeaterTarget`, `ShiftableLoad`), then emits `PlanTrigger::UserRequest`
to the Planner watch channel.

- Applies default `CompletionPolicy` per asset type
- Calculates energy requirements from SoC delta × capacity for battery-like assets

#### Monitor (Ledger)

Runs every 1 s via `controller::monitor::record_tick()`, called from `tasks/sim_tick/publish.rs`.
Updates the per-asset cumulative energy/cost/CO₂ ledger (`state::AssetLedgerEntry`) using the
current sim snapshot and the tariff active at `now` (Step/LOCF lookup). Only importing assets
accrue cost/CO₂; export is not credited as revenue in the ledger.

#### Dispatcher

Pure-function module (`controller/dispatcher.rs`) driven by the 1 s tick in
`tasks/sim_tick/`. `build_setpoints()` translates the current `PlanTimeSlot` into device
setpoints:

1. Seeds every asset with its `default_setpoint_kw`
2. For each `AssetAllocation` in the plan slot covering `now`: overwrites that asset's setpoint
3. Caps PV export at the active capacity limit
4. Applies the opportunistic surplus-EV overlay (`apply_surplus_ev_overlay`): routes live
   PV surplus (after all other loads and any planned battery charging) to the EV when no
   plan-level EV allocation is active for this slot

❌ **GAP** (`docs/plans/review_items_resolution_strategy.md` R5, `docs/BACKLOG.md` BL-22):
there is no "auto-follow" concept and no `NetDeviation` distribution across assets.
`apply_battery_correction_overlay` — a dead-beat P-controller that reacts to grid
deviation — is fully implemented and unit-tested but deliberately `#[allow(dead_code)]`:
it is never wired into `build_setpoints()`. This was built, then left unintegrated; R5
resolved to keep it (not delete) — BL-22 tracks wiring it behind a profile flag. Ledger
accounting is **not** the Dispatcher's responsibility — see Monitor above.

### 2.2 Two-Speed Loop

The controller operates at two timescales:

| Loop | Period | Driver | Purpose |
|---|---|---|---|
| **Fast** (Dispatcher + Monitor) | 1 s | `tasks/sim_tick` Tokio interval | Execute current plan slot; accumulate ledger |
| **Slow** (Planner) | `replan_interval_s` periodic (default 300 s, profile-configurable) + watch channel | `PlanTrigger` watch channel | Produce new Plan from current rates, sessions, and asset state |

The watch channel (`PlanTrigger`) decouples triggering from execution: any component can emit
a trigger; the Planner processes them in order. This prevents redundant replanning while ensuring
every relevant event causes exactly one new plan.

Trigger senders in code today: HTTP routes (`PlanTrigger::UserRequest`), `POST /plan/trigger`
and sim state injection (`PlanTrigger::AssetStateChange`), the event poll loop
(`PlanTrigger::RateChange` — fired for **any** detected change, including capacity changes),
and shiftable-load completion (`PlanTrigger::UserRequest`). `PlanTrigger::Alert` and
`::CapacityChange` are defined but never sent — see the OpenADR Interface event table above.

### 2.3 Planning Algorithm

The Planner is a **3-tier, two-phase MILP solver** (`controller/milp_planner/`), reached
through the `SolverPort` trait (`controller/solver_port.rs`) — `services/planning.rs` is
the only caller of `SolverPort::solve`, so nothing outside the planner module touches
HiGHS or `run_planner` directly.

**Full design reference:** [`docs/architecture/ven_milp_planner.md`](ven_milp_planner.md)

**Key concepts:**

- **Three tiers** with variable step sizes: fine-grained near-horizon (e.g. 5 min slots),
  coarser mid-horizon, sparse far-horizon. Controlled by `PlannerParams.plan_zones`.
- **Assets as MILP variables**: EV continuous power `p_ev_kw[t]`, heater discrete tiers
  (`z_heat_mid[t]`, `z_heat_full[t]`), battery SoC tracking, etc.
- **Session intent as constraints**: `EvSession`/`HeaterTarget`/`ShiftableLoad` provide
  energy target, deadline, and mode; the solver iterates over asset variables, not session
  objects. See §2.3.1 below.
- **Adoption gate**: a new plan is adopted only if it beats the current plan's cost+friction
  by the effective threshold (which decays over the current plan's age), or if the current
  plan's slots have all expired, or on any non-periodic trigger — prevents churn from noise
  replans.

**VTN-unreachable behaviour (current):** tariff lookups use Step/LOCF extrapolation
(`common::TimeSeries`), carrying the last known rate forward; slots with no tariff
data at all fall back to hardcoded defaults (0.25 €/kWh import, 0.08 €/kWh export,
300 g/kWh CO₂). `PlanTimeSlot.rate_estimated` is hardcoded `false`, so no
`PlanWarning` flags a stale-rate plan. A configurable stale-rate policy (choice of
last-known / heuristic-forecast / defer-to-flexible / safe-average, plus a real
`rate_estimated` flag) is future work — tracked as `docs/BACKLOG.md` BL-07.

#### 2.3.1 Session Intent in the MILP

Device sessions (`EvSession`, `HeaterTarget`, `ShiftableLoad`) provide user intent as solver
constraints — the solver does not iterate over session objects directly:

| Session field | MILP use |
|---|---|
| `EvSession.soft_deadline` | `false` → `MilpLoadMode::MustRun`; `true` → `MayRun` |
| `EvSession.departure_time` | → horizon constraint step `t_ev_dead_step` |
| `HeaterTarget` presence | present → `MustRun` (hard deadline); absent → `MayRun` (autonomous, no deadline) |
| `HeaterTarget.ready_by` | → horizon constraint step `t_dead_step` |
| `EvSession.target_soc` / `HeaterTarget.target_temp_c` | → energy/thermal requirement |

Session tracking (accumulated cost, per-slot power history, status lifecycle) is handled
by the Dispatcher and reporting layer — not by the solver.

### 2.4 Data Flows

**One heartbeat (5 min PlanTimeStep, steady state):**

```
t=0s     Asset Controller polls devices + grid meter
           → AssetState (power, SoC, temperature, IsConnected)
           → SiteMeter.NetImport_kW

t=0.05s  Dispatcher reads current PlanTimeSlot
           → DispatchCommand[] to Simulator
           → AccumulatedCost/CO₂ updated in asset ledger

t=0.1s   Monitor
           → AssetLedger updated (energy/cost/CO₂ per asset)

t=30s    OpenADR Interface polls VTN (POLL_EVENTS_SECS, default 30 s)
           → New events → translate to TariffSnapshot, OadrCapacityState
           → PlanTrigger::RateChange if anything changed (see §2.2 — this fires for
             capacity changes too; there is no separate CapacityChange trigger in use)

t=300s   Planner (if triggered, or on replan_interval_s timeout — default 300 s)
           → Reads all state
           → Produces new Plan
           → Emits FlexibilityEnvelopes
           → Writes PlanWarnings → UserNotifications
```

❌ **GAP** (`docs/BACKLOG.md` BL-20): the last line above overstates current behaviour —
`PlanWarning`s are written into the `Plan`, but no `UserNotifications` feed exists
anywhere in the VEN today (no queue, no route, no UI surface). `UserNotificationSeverity`
(`entities/design_vocabulary.rs`) is the only trace of this intended feature.

---

## 3. Asset Layer

### 3.0 Asset Abstraction

Each asset exposes a uniform interface to the controller. The controller never calls
physics functions directly or reads simulation parameters.

```rust
/// Physics contract for one asset type. Implemented by Battery, EvCharger, Heater,
/// PvInverter, BaseLoad (VEN/src/assets/*.rs).
trait Asset: Send + Sync {
    fn step(&self, state: &AssetState, setpoint_kw: f64, dt: Duration) -> (AssetState, f64);
    fn capability(&self, state: &AssetState) -> AssetCapability;
    fn simulate_forward(&self, initial: &AssetState, setpoints: &[(DateTime<Utc>, f64)]) -> Trajectory; // default impl
    fn simulate_free(&self, initial: &AssetState, duration: Duration) -> Trajectory;                    // default impl
    fn capability_trajectory(&self, initial: &AssetState, duration: Duration, resolution: Duration)
        -> Vec<(DateTime<Utc>, AssetCapability)>;                                                        // default impl

    // Identity/history — callable only via AssetHandle; panic on a bare physics type.
    fn id(&self) -> &str;
    fn current_state(&self) -> AssetState;
    fn history(&self, window: Duration) -> Vec<HistoryPoint>;
}
```

`AssetConfig` (`VEN/src/assets/mod.rs`) is the concrete enum-dispatch implementation —
one variant per asset type (`Battery`, `Ev`, `Heater`, `Pv`, `BaseLoad`) — holding the
physics parameters loaded from the profile. `AssetHandle` wraps a `(&AssetConfig,
&AssetEntry)` pair to implement the identity/history methods. Per-asset history is a
ring buffer (`AssetHistoryBuffer`, 3600 points ≈ 1 h at 1 s tick) with LOCF lookups and
time-weighted averaging.

| Implementation | Backend | Status |
|---|---|---|
| `AssetConfig` (Battery/Ev/Heater/Pv/BaseLoad variants) | Physics model (sin, SoC, thermal), `VEN/src/assets/` | ✅ implemented — all current VENs |
| `MeasuredAsset` | Real sensor / hardware API | Future — not yet built |

From the controller's perspective a future `MeasuredAsset` would be identical to
`AssetConfig`: swapping one for the other should require no changes outside that asset's
module. This is a design intent for future real deployments, not a present capability.

**Simulation parameters** (irradiation curve, initial SOC, rated power, thermal constants)
are only accessible through the `/sim` API endpoints. The controller never reads them.

**Full trait contract:** [`docs/architecture/ven_asset_interface_spec.md`](ven_asset_interface_spec.md).

### 3.1 Generic Asset Model

The simulator implements the asset interface using a generic model: `SimState.assets: Vec<AssetEntry>`.

```rust
struct SimState {
    assets: Vec<AssetEntry>,
    grid:   GridMeter,
}

struct AssetEntry {
    id:         String,
    state:      AssetState,    // enum dispatch to per-type physics
    setpoint:   f64,           // last commanded value from Dispatcher
    last_power_kw: f64,        // result of last physics tick
    energy:     EnergyCounter, // cumulative kWh for this asset
}
```

`AssetState` is an enum (`PvInverter(PvState)`, `EvCharger(EvState)`, `Battery(BatteryState)`,
`Heater(HeaterState)`, `BaseLoad(BaseLoadState)`). Each variant implements the physics tick.

Adding a new asset type requires only a new enum variant and its actor module — no changes to
the simulator loop, API handlers, or profile parser.

**API compatibility:** `GET /sim` returns both the new `assets: HashMap<String, AssetSnapshot>`
and backward-compatible named fields (`ev`, `heater`, `pv`, `battery`, `base_load_w`) derived
from typed `AssetState`. This allows UI and tests to migrate incrementally.

**Profile format:**
```yaml
assets:
  - type: ev
    id: ev
    max_charge_kw: 7.4
    capacity_kwh: 50.0
    initial_soc: 0.20
  - type: battery
    id: battery
    max_charge_kw: 2.0
    max_discharge_kw: 2.0
    capacity_kwh: 10.0
```

### 3.2 Physics Models Per Asset Type

#### PV Inverter

Irradiation is the primary simulated quantity; P_pv is derived from it:

```
irradiation(t) = irradiation_peak × sin(π × (hour − 6) / 12)   for 06:00 ≤ hour ≤ 18:00
irradiation(t) = 0                                               otherwise (clamped)

P_pv(t) = −P_max × (irradiation(t) / irradiation_stc)
```

`irradiation_stc` = 1000 W/m² (Standard Test Conditions reference).
Irradiation is clamped to zero outside daylight hours regardless of manual UI overrides.
Sign convention: `P_pv` is negative (generation, exported or self-consumed).
Curtailment: if `ExportCapacityLimit` is set and `|P_pv| > limit`, the inverter is cropped to `−limit`.

**Forecast:** `PvAsset.forecast(horizon)` applies the same irradiation model over future
time slots. The planner calls this — it does not contain a PV formula of its own.

#### Battery

```
dSOC/dt = P_charge × efficiency / capacity_kwh   (charging: P > 0)
dSOC/dt = P_discharge / capacity_kwh              (discharging: P < 0)
```

Hard bounds: `SOC ∈ [MinSoC, MaxSoC]`. Power clamped to `[MinPower_kW, MaxPower_kW]`.

#### EV Charger

Stepless, range `[min_charge_kw, max_charge_kw]`. Minimum active charge rate = 1.5 kW
(cannot charge below minimum once active). Discharge not modelled (charge-only in lab).
SOC integration same as battery. Response delay ~10 s (modelled as single-step lag).

#### Heater (Thermal Model)

```
dT/dt = (P_heater × efficiency − ambient_loss_rate × (T_room − T_ambient)) / thermal_mass
```

`ambient_loss_rate` default: 0.1 kW/°C. Thermostat override at `T_min` / `T_max` bounds.
Power levels: discrete `[0, 3, 6]` kW (STEPPED adjustability).

#### Base Load

Static consumption profile (`W` constant or time-varying). Not controllable.
Represents appliances, lighting, standby — the uncontrollable fraction of site demand.

### 3.3 Control Path

The controller is the **single control authority** — exactly one writer produces the
`Setpoints` struct each cycle. (A separate reactive FSM layer alongside the planner was
rejected: two independent writers to `Setpoints` make arbitration ambiguous, with the
Dispatcher silently overriding one of them. Transition smoothing, where needed, lives in
the Dispatcher execution layer.)

**Control path:**
```
VTN events → openadr_interface → rates + capacity constraints
                                            │
User requests ──────────────────────────────┤
                                            ▼
                                        Planner
                                            │
                                        Dispatcher → Simulator setpoints
```

**Tracing:** `GET /trace/events` serves an in-memory ring buffer of `ControllerEvent`s
(capacity 500) with controller-level decisions — rate/capacity changes, plan cycles,
request transitions. `GET /trace/history` serves per-asset recent history.

---

## 4. API Contract

All routes are registered in `VEN/src/routes/mod.rs::build_router`. CORS is open. All
handlers receive `State(ctx: AppCtx)`.

### 4.1 Infrastructure

| Method | Path | Description |
|---|---|---|
| GET | `/health` | Returns `"ok"` |
| GET | `/metrics` | Prometheus metrics (text format) |

### 4.2 OpenADR Proxy

Forwards queries to the VTN via `VtnClient`.

| Method | Path | Description |
|---|---|---|
| GET | `/events` | Active OpenADR events from VTN; optional `?limit=N` |
| GET | `/programs` | Available programs from VTN |

### 4.3 Sensors

Manual sensor snapshot — UI and test injection.

| Method | Path | Description |
|---|---|---|
| GET | `/sensors` | Current sensor snapshot (temperature, power, voltage) |
| POST | `/sensors` | Create/update sensor snapshot (local only, not sent to VTN) |

### 4.4 Reports

VTN report submission.

| Method | Path | Description |
|---|---|---|
| GET | `/reports` | Reports submitted to VTN by this VEN |
| POST | `/reports` | Submit new report to VTN (proxied) |
| PUT | `/reports/:id` | Update existing report at VTN (proxied) |

### 4.5 Simulator

Physics-based device simulation.

| Method | Path | Description |
|---|---|---|
| GET | `/sim` | Full simulator state: device states, power flows, energy counters |
| GET | `/sim/schema` | JSON schema for the profile YAML |
| POST | `/sim/reset/:asset_id` | Reset a specific asset to its profile defaults |
| PUT | `/sim/config/battery` | Update battery configuration at runtime |
| GET | `/sim/inject` | Current injection overrides (`SimInjectState`) |
| POST | `/sim/inject` | Set one or more injection overrides — **full-replace** semantics (see D-06) |
| POST | `/sim/inject/reset` | Clear all injection overrides |
| POST | `/plan/trigger` | Force a `PlanTrigger::AssetStateChange` replan |

`POST /sim/inject` replaced the earlier `/sim/override` and supports four independent
behaviour classes (`state.rs::SimInjectState`):

| Class | Fields | Semantics |
|---|---|---|
| A — one-shot | `battery_soc`, `ev_soc`, `heater_temp_c` | Applied once to physics state, then cleared automatically |
| B — frozen + EMA return | `pv_irradiance` (+`pv_irradiance_alpha`), `base_load_kw` (+`base_load_alpha`) | Held while active; EMA-blended back to the natural model on release |
| C — frozen + snap | `ev_plugged`, `ev_soc_target`, `heater_setpoint_c`, `heater_temp_min_c`, `heater_temp_max_c`, `ambient_temp_c`, `grid_import_limit_kw`, `grid_export_limit_kw` | Held while active; snaps to the profile default on release |
| D — planning-only | `pv_plan_kw` | Pins the PV forecast for all horizon slots; no physics effect, no replan trigger |

### 4.6 Timeline & Asset Forecast

| Method | Path | Description |
|---|---|---|
| GET | `/timeline/all` | Merged past+future timeline for all known assets + grid (registered before `/timeline/:asset_id` — more specific route first) |
| GET | `/timeline/:asset_id` | Merged past+future timeline for one asset |
| GET | `/forecast/:asset_id` | Physics-projected future power for one asset |
| GET | `/history/:asset_id` | Raw per-asset history slice |
| GET | `/capability/:asset_id` | Point-in-time feasible power range (`AssetCapability`) |

### 4.7 HEMS Controller

| Method | Path | Stage | Description |
|---|---|---|---|
| GET | `/tariffs` | 2 | `TariffSnapshot` array parsed from active events |
| GET | `/capacity` | 2 | `OadrCapacityState` parsed from active events |
| GET | `/obligations` | 2 | Pending report obligations extracted from events |
| GET | `/plan` | 3 | Active Plan or `null` |
| PUT | `/plan/objective` | 3 | Override the active `PlannerObjective` |
| GET | `/plan/events` | 3 | SSE stream of `PlannerEvent`s (`SolvingStarted`/`SolvingProgress`/`PlanReady`) |
| GET | `/ledger` | 4 | Per-asset cumulative energy / cost / CO₂ ledger |
| GET | `/user-requests` | 5 | All active user energy task requests |
| POST | `/user-requests` | 5 | Create user request → `EvSession` / `HeaterTarget` / `ShiftableLoad` |
| DELETE | `/user-requests/:id` | 5 | Cancel request → marks it `Cancelled` |
| GET | `/flexibility` | 5 | `SiteFlexibilityEnvelope` derived from live asset state (refreshed every dispatcher tick, independent of the active plan) |
| GET / POST / DELETE | `/ev-session` | 5 | Read / create / end the active `EvSession` |
| GET / PUT | `/ev-settings` | 5 | Opportunistic surplus-EV-charging overlay toggle |
| GET / POST / DELETE | `/heater-target` | 5 | Read / create / clear the active `HeaterTarget` |
| GET / POST | `/shiftable-loads` | 5 | List / create shiftable loads |
| DELETE | `/shiftable-loads/:id` | 5 | Remove a shiftable load |
| GET / POST / DELETE | `/baseline-override` | 5 | Read / create / clear additive baseline-load adjustments |

### 4.8 Trace

| Method | Path | Description |
|---|---|---|
| GET | `/trace/events` | `ControllerEvent` log (ring buffer, capacity 500), newest first; optional `?limit=N` |
| GET | `/trace/history` | Per-asset recent history slice |

### 4.9 Recorded History — Storage Model Summary

| Endpoint | What it records | Storage | Max history |
|---|---|---|---|
| `GET /trace/events` | Controller-level decisions (rate/capacity changes, plan cycles, request transitions) | In-memory ring buffer (500 entries) | Variable — depends on event frequency, not a fixed duration |
| `GET /ledger` | Cumulative totals per asset since startup | In-memory, 1 s updates | Since restart |
| `GET /reports` | Discrete report snapshots sent to VTN | Stored at VTN | Indefinite |
| `GET /timeline/:asset_id` / `/timeline/all` | Per-asset physics history merged with future plan slots | In-memory ring buffer (3600 points ≈ 1 h at 1 s tick) + full plan horizon | ≈ 1 h past + plan horizon future |

`/timeline` is the closest thing to a continuous power time series today (measured watts
in the past window, planned watts in the future window).

---

## 5. Time-Series Alignment

The system deals with multiple time series that originate from different sources and carry
different natural periods. They rarely align on a common grid:

| Series | Typical period | Origin | Type |
|---|---|---|---|
| Asset power (sim) | 1 s | Simulator tick | Continuous physical |
| Planning grid slots | 60–300 s (configurable) | Planner | Derived |
| PRICE / GHG events | 1 h (day-ahead) | VTN OpenADR | Piecewise-constant |
| Capacity limit events | 3–6 h | VTN OpenADR | Piecewise-constant |
| SIMPLE / alert events | Variable | VTN OpenADR | Piecewise-constant |
| Report obligations | 15–30 min (typical) | VTN event `reportDescriptors` | Aggregation target |
| UI chart buckets | Variable (display width) | Browser | Downsampled |

### 5.1 Interpolation Semantics by Signal Type

Different signal types require different interpolation rules. Mixing rules is a source of
silent bugs (e.g. linearly interpolating a tariff implies a continuous ramp, which is wrong).

| Signal type | Examples | Correct rule | Wrong |
|---|---|---|---|
| **Piecewise-constant** | Tariff (€/kWh), capacity limit (kW), SIMPLE level | **Step / LOCF** — value holds until the next breakpoint | Linear interpolation |
| **Continuous physical** | Power (kW), temperature (°C), SOC (%) | **Linear** between measured points | Carrying last value flat |
| **Cumulative** | Energy (kWh), cost (€) | **Sum within bucket** — never interpolate | Any interpolation |

**LOCF** = Last Observation Carried Forward — the value at time `t` is the most recent value
at or before `t`. Correct for tariffs and any signal that "takes effect and stays in effect".

### 5.2 Implementation — `common::TimeSeries`

A single reusable abstraction (`VEN/src/common/mod.rs`) backs all three time-series
consumers — tariffs, obligation reports, and timeline resampling — so there is one
interpolation/aggregation implementation, not one per consumer.

```rust
struct TimeSeries {
    samples:       Vec<(DateTime<Utc>, f64)>,
    interpolation: Interpolation,  // Step | Linear
}

enum Interpolation {
    Step,    // LOCF — used for tariffs, capacity limits
    Linear,  // used for power, temperature, SOC
}

impl TimeSeries {
    fn interpolate_at(&self, ts: DateTime<Utc>) -> Option<f64>;
    fn time_weighted_mean(&self, start: DateTime<Utc>, end: DateTime<Utc>) -> Option<f64>;
    fn resample_to_grid(&self, timestamps: &[DateTime<Utc>]) -> TimeSeries;
    fn resample_uniform(&self, width: Duration, agg: Aggregation) -> TimeSeries; // Mean | Min | Max
}
```

**Consumers today:**
- **Tariffs** (`entities/tariff_snapshot.rs::TariffTimeSeries`): three Step-interpolated
  `TimeSeries` (import, export, CO₂) built from `TariffSnapshot`s at the OpenADR interface
  boundary.
- **Obligation reports** (`controller/reporter.rs`): `resample_uniform` buckets net site
  power onto the obligation's `intervalPeriod` grid; SoC is sampled at each interval end.
- **Timeline** (`controller/timeline.rs`): uniform-grid resampling with LOCF time-weighted
  averaging for the UI chart.

**Event merge** (`openadr_interface.rs`) remains last-write-wins when multiple events
define the same interval; the OpenADR `priority` field is parsed but not used in
ordering — a higher-priority event processed first can be silently overwritten by a
lower-priority one processed later. Not tracked as a numbered item yet; add to
`docs/reference/TECHNICAL_DEBTS.md` if picked up.

❌ **GAP** (`docs/reference/TECHNICAL_DEBTS.md` R-16): the MILP planner still samples
each slot's tariff at its **start** timestamp only (`milp_planner/inputs.rs`,
`interpolate_at(slot_t)`), not the time-weighted mean across the slot. A slot straddling
a tariff boundary is priced entirely at the pre-boundary rate. `TimeSeries::time_weighted_mean`
already exists and would fix this in one call.

### 5.3 OpenADR Spec Position

The spec defines interval structure but leaves VEN-side alignment to the implementer:
- Mixed `intervalPeriod` granularities within a single event (or across events) are legal.
- Reports may use `dataQuality = ESTIMATED` for interpolated/inferred values — acknowledged but unspecified.
- Event `priority` is defined but conflict resolution for overlapping same-type payloads is not specified; priority-based ordering before merge is the correct interpretation.

---

## 6. Design Decisions

### D-01: MILP Planner (replaces greedy scheduler)

**Decision:** 3-tier MILP solver via HiGHS.
**Rationale:** The greedy scheduler was replaced when more assets and tighter constraints were
added. HiGHS solves the residential-scale problem (24–48 h, 3–15 assets) in 5–10 s on Pi4,
which is acceptable for a 20–300 s replan interval. The adoption gate filters noise replans.
See `docs/architecture/ven_milp_planner.md` for full design rationale.

### D-02: In-Memory Ledger

**Decision:** The per-asset ledger (`state::AssetLedgerEntry`) is in-memory only; resets on restart.
**Rationale:** The ledger is a running total for the current session. Persistent billing-period
data is stored at the VTN as reports. Local persistence adds complexity for little benefit in
a lab context.

### D-03: Reactor Removed (spec kit 001)

See §3.3. Controller is the single control authority.

### D-04: Generic Asset Model (spec kit 002)

**Decision:** `SimState.assets: Vec<AssetEntry>` with enum dispatch.
**Rationale:** The hardcoded named-field model required touching every layer when adding an
asset type. The generic model isolates new asset types to their own module.

### D-05: OadrEventSnapshot Unification

**Decision:** `TariffSnapshot` holds all time-varying signals
(price, CO₂, capacity limits) in one struct per poll tick.
**Rationale:** All fields are co-valid at the same timestamp. A unified struct eliminates
temporal alignment bugs that arise when price and capacity signals are stored separately.
See REQUIREMENTS §3.2.2.

### D-06: POST /sim/inject is Full-Replace

**Decision:** `POST /sim/inject` replaces the entire injection-override struct (see §4.5
for the endpoint's four behaviour classes; it superseded the earlier `/sim/override`).
**Rationale:** Partial-patch semantics (PATCH) require null-vs-absent disambiguation.
Full-replace is simpler and explicit. Callers must set all fields they want active.

### D-07: Event Poll Interval — Configurable, Not Fixed

**Decision:** Event polling defaults to 30 s (`POLL_EVENTS_SECS` env var, default 30;
`POLL_PROGRAMS_SECS`/`POLL_REPORTS_SECS` default 30/60) rather than a hardcoded constant.
**Rationale:** Balances VTN load against response latency; making it env-configurable
lets a deployment tune this per VTN without a rebuild. Configurable jitter is not
implemented in the lab.
