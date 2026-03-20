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
│  │  ┌──────────────┐   ┌──────────────┐   ┌───────────────────────────┐   │ │
│  │  │  OpenADR     │   │    User      │   │     Monitor               │   │ │
│  │  │  Interface   │   │   Request    │   │     (Deviation Detector)  │   │ │
│  │  └──────┬───────┘   └──────┬───────┘   └────────────┬──────────────┘   │ │
│  │         │                  │                        │                   │ │
│  │         └──────────────────┤◄───────────────────────┘                   │ │
│  │                            ▼                                            │ │
│  │                   ┌──────────────┐                                      │ │
│  │                   │   Planner    │ ← PlanTrigger channel                 │ │
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
│  │  ┌──────────────────────────────────────────────────────────────────┐  │ │
│  │  │  AssetInterface: current() · forecast(horizon) · past(window)    │  │ │
│  │  └──────────────────────────────────────────────────────────────────┘  │ │
│  │          ▲                                            ▲                 │ │
│  │  ┌───────┴────────┐                       ┌──────────┴──────────┐      │ │
│  │  │ SimulatedAsset │  ← physics models     │  MeasuredAsset      │      │ │
│  │  │ PV · Battery   │    per asset type     │  (future: real HW   │      │ │
│  │  │ EV · Heater    │                       │   / external API)   │      │ │
│  │  │ BaseLoad       │                       └─────────────────────┘      │ │
│  │  └───────┬────────┘                                                     │ │
│  │          │ UI only                                                      │ │
│  │  ┌───────▼────────┐                                                     │ │
│  │  │ /sim endpoints │  ← simulation params, overrides, schema, reset      │ │
│  │  └────────────────┘                                                     │ │
│  └─────────────────────────────────────────────────────────────────────────┘ │
│                                                                              │
│  REST API (Axum, port 8080 internal / 821x host)                            │
└──────────────────────────────────────────────────────────────────────────────┘
                        │
                        │ OpenADR 3 REST (OAuth2 + polling at 30 s)
                        ▼
                   ┌──────────┐
                   │   VTN    │
                   └──────────┘
```

**Source layout (current):**
```
VEN/src/
  main.rs              — Axum router + all handler functions
  controller/          — dispatcher, monitor, openadr_interface, planner, user_request
  entities/            — asset, capacity, energy_packet, plan, rate_snapshot, site_meter, user_request
  simulator/           — mod.rs, assets/{ev,heater,pv,battery,base_load}, energy, persist, power_model
  reactor/             — REMOVED (see §3.3)
  reporter.rs          — report-building logic (no HTTP endpoints)
```

**Source layout (target — post asset-interface + common refactor):**
```
VEN/src/
  main.rs
  common/              — shared abstractions; no VEN-specific logic
    mod.rs
    timeseries.rs      — TimeSeries<T>, Interpolation, bucket(), resample()
    interval.rs        — overlap(), union_of_breakpoints(), time-weighted average
  controller/
  entities/
  assets/              — one module per asset type; each owns physics + forecast + history
    mod.rs             — AssetInterface trait + AssetEntry
    pv/                — PvAsset: irradiation model, forecast(), history(), /sim params
    battery/           — BatteryAsset: SOC model, forecast(), history(), /sim params
    ev/                — EvAsset
    heater/            — HeaterAsset: thermal model
    base_load/         — BaseLoadAsset
  reporter.rs
```

`common/` is a plain Rust module (`mod common;` in `main.rs`), not a separate crate.
When a VTN controller is built, `common/` can be extracted into a shared workspace crate
at that point — its public API will not need to change.

See `docs/BACKLOG.md §Refactoring` for the migration plan.

---

## 2. HEMS Controller

### 2.1 Components & Responsibilities

| Component | Module | Cycle / Trigger | Owns |
|---|---|---|---|
| **OpenADR Interface** | `controller/openadr_interface` | 30 s poll + event-driven | `OadrEventCache`, `OadrEventSnapshots` [RENAME], `OadrCapacityState`, `OadrReportObligations`, `PlannedRates` |
| **User Request Manager** | `controller/user_request` | Event-driven (API call) | `UserRequest`, `EnergyPacket` (creation) |
| **Monitor** | `controller/monitor` | 1 s tick | `AssetLedger`, `PastEnergySum`, deviation detection |
| **Planner** | `controller/planner` | Watch channel + 20 s periodic | `Plan`, `FlexibilityEnvelopes`, `PlanWarnings` |
| **Dispatcher** | `controller/dispatcher` | 1 s tick | `DispatchCommands` → Simulator setpoints |
| **Entities** | `entities/` | Shared state | `EnergyPacket`, `Plan`, `RateSnapshot`, `AssetLedger`, `UserRequest` |

#### OpenADR Interface

Translates between VTN REST JSON and the internal domain model. The only component that
knows about OpenADR HTTP, OAuth, and event payload formats.

**VTN → internal translation:**

| OpenADR EventType | Internal target |
|---|---|
| `PRICE` | `OadrEventSnapshot.ImportPrice` |
| `EXPORT_PRICE` | `OadrEventSnapshot.ExportPrice` |
| `GHG` | `OadrEventSnapshot.ImportCO2` |
| `IMPORT_CAPACITY_LIMIT` | `OadrEventSnapshot.ImportCapacityLimit` (per interval) |
| `EXPORT_CAPACITY_LIMIT` | `OadrEventSnapshot.ExportCapacityLimit` (per interval) |
| `IMPORT_CAPACITY_SUBSCRIPTION` | `OadrCapacityState.ImportSubscription_kW` |
| `EXPORT_CAPACITY_SUBSCRIPTION` | `OadrCapacityState.ExportSubscription_kW` |
| `IMPORT_CAPACITY_RESERVATION` | `OadrCapacityState.ImportReservation_kW` |
| `EXPORT_CAPACITY_RESERVATION` | `OadrCapacityState.ExportReservation_kW` |
| `ALERT_GRID_EMERGENCY` / `ALERT_BLACK_START` | High-priority synthetic EnergyPacket via Planner |
| `ALERT_FLEX_ALERT` | `PlanTrigger.ALERT` |
| `DISPATCH_SETPOINT` | Direct Dispatcher override (bypasses Planner) |
| `CHARGE_STATE_SETPOINT` | Creates/modifies EnergyPacket |

**Internal → VTN report generation:**

| Report obligation | Source |
|---|---|
| `USAGE` | `PastEnergySum` per resource per interval (from SiteMeter) |
| `DEMAND` | `AssetState.ActualPower` per resource |
| `STORAGE_CHARGE_LEVEL` | `AssetState.SoC` per storage resource |
| `OPERATING_STATE` | Derived from `DeviceResponsiveness` + `EnergyPacketStatus` |
| `USAGE_FORECAST` | FIRM slots: point forecast; FLEXIBLE slots: range [0, MaxPower] in window |
| `IMPORT_CAPACITY_RESERVATION` | `GetImportFlexibility()` + Σ `FlexibilityEnvelope.MaxPower` |
| `EXPORT_CAPACITY_RESERVATION` | `GetExportFlexibility()` |

#### User Request Manager

Translates user-facing energy requests (from `POST /user-requests`) into `EnergyPacket`
entities, then emits `PlanTrigger.USER_REQUEST` to the Planner watch channel.

- Resolves natural-language time shortcuts (UI concern) to concrete RFC 3339 timestamps
- Applies default `CompletionPolicy` per asset type (see REQUIREMENTS §3.2.1)
- Calculates `TargetEnergy_kWh` from SOC delta × battery capacity

#### Monitor (Deviation Detector)

Runs every 1 s in the Dispatcher loop. Responsibilities:
- Updates `AssetLedger` (cumulative energy/cost/CO₂ per asset)
- Updates `PastEnergySum` for report obligations
- Checks penalty thresholds (`MeasurementWindow`)
- Detects deviation: `|ActualPower − PlannedPower| > threshold` → emits `PlanTrigger.DEVICE_DEVIATION`
- Derives `SITE_RESIDUAL`: `P_residual = P_utility − Σ P_modelled_assets`

#### Dispatcher

1 s tick loop. Translates the current `PlanTimeSlot` into device setpoints:

1. Reads `ActivePlan.currentSlot` from Planner
2. For each `PacketAllocation` in the slot: computes `DispatchCommand` for the target asset
3. For auto-follow assets: distributes `NetDeviation = Σ(ActualPower) − Σ(PlannedPower)` across auto-follow assets
4. Writes commands to the Simulator
5. Updates `EnergyPacket.PastPowerProfile` and accumulated cost/CO₂
6. Transitions packet status (`PENDING → ACTIVE → COMPLETED`)

### 2.2 Two-Speed Loop

The controller operates at two timescales:

| Loop | Period | Driver | Purpose |
|---|---|---|---|
| **Fast** (Dispatcher + Monitor) | 1 s | Tokio interval | Execute current plan slot; accumulate ledger; detect deviations |
| **Slow** (Planner) | 20 s periodic + watch channel | `PlanTrigger` watch channel | Produce new Plan from current rates, packets, and asset state |

The watch channel (`PlanTrigger`) decouples triggering from execution: any component can emit
a trigger; the Planner processes them in order. This prevents redundant replanning while ensuring
every relevant event causes exactly one new plan.

### 2.3 Planning Algorithm (Summary)

The Planner runs an **8-phase priority-based greedy scheduler**. It is not a full LP/MILP
optimizer — deliberate choice for residential scale (24–48 h horizon, 3–15 assets, millisecond
runtime requirement, frequent replanning).

For the complete algorithm see `docs/VEN_Controller/Step4_Algorithm.md` [ARCHIVED].

```
Phase 1 — PREPARE
  Build planning grid (slots × tariffs × limits)
  Classify slots: FIRM (near-horizon) vs FLEXIBLE (far-horizon)
  Populate baseline by calling asset.forecast(horizon) for each asset — NO inline formulas
  Classify assets and packets

Phase 2 — SCORE (FIRM slots only)
  For each (packet, slot) pair:
    Compute CalcCache: slot cost, comfort bid, time pressure, eligibility

Phase 3 — ALLOCATE CONSUMPTION (FIRM slots only)
  Sort eligible (packet, slot) pairs by EffectivePriority
  Greedy allocation respecting hard constraints (capacity limits, SOC bounds)

Phase 4 — ALLOCATE STORAGE (FIRM slots only)
  Identify charge/discharge opportunities
  Apply round-trip efficiency test

Phase 5 — ALLOCATE RESIDUAL PV SURPLUS (FIRM slots only)
  Export unclaimed surplus (up to ExportCapacityLimit)
  Handle PV curtailment if export cap is zero

Phase 6 — PENALTY CHECK (FIRM slots only)
  Evaluate discrete penalty thresholds (MeasurementWindow)
  Reschedule if avoidance cost < penalty cost

Phase 7 — BUILD FLEXIBILITY ENVELOPES (far horizon)
  For each packet with unallocated energy:
    Characterize flexible demand window
    Compute rate range, budget remaining, estimated cost

Phase 8 — FINALIZE
  Write FIRM PacketAllocations
  Write FlexibilityEnvelopes
  Compute slot summaries and completion estimates
  Emit PlanWarnings
```

**Key data structure: CalcCache** (transient, per-packet-per-slot, discarded after Phase 3)

| Field | Description |
|---|---|
| `EffectiveCost` | Surplus-aware cost for this packet in this slot. Pure grid: `ImportPrice + ImportCO₂ × CO₂Weight`. PV self-consumption: `ExportPrice` (opportunity cost). Blended: weighted average. |
| `ComfortBid` | Interpolated from `ComfortRate[]` at `ProjectedFill`. Maximum the packet will pay. |
| `TimePressure` | Urgency factor — rises as `SlotsUntilDeadline` shrinks. |
| `WithinComfortBid` | `EffectiveCost ≤ ComfortBid` — eligibility gate. |

**Slot classification:** `FirmBoundary = now + NearHorizonDuration` (configurable, default 2 h).
Slots within `[now, FirmBoundary]` are FIRM. Slots beyond are FLEXIBLE.

**Early firm-up:** If rate variance across the FLEXIBLE window is < 10% (flat rate), FLEXIBLE
slots may firm up early to simplify execution.

**StaleRatePolicy:** When VTN is unreachable, unknown future slots are handled per
`StaleRatePolicy` (default: `HEURISTIC_FORECAST`). See REQUIREMENTS §3.2.1.

### 2.4 Data Flows

See `docs/VEN_Controller/Step3_DataFlow.md` [ARCHIVED] for complete entity lifecycle tables.

**One heartbeat (5 min PlanTimeStep, steady state):**

```
t=0s     Asset Controller polls devices + grid meter
           → AssetState (power, SoC, temperature, IsConnected)
           → SiteMeter.NetImport_kW

t=0.05s  Dispatcher reads current PlanTimeSlot
           → DispatchCommand[] to Simulator
           → EnergyPacket.PastPowerProfile updated
           → AccumulatedCost/CO₂ updated

t=0.1s   Monitor
           → PastEnergySum updated
           → AssetLedger updated
           → Penalty check
           → Deviation detection → PlanTrigger? (via watch channel)

t=30s    OpenADR Interface polls VTN
           → New events → translate to OadrEventSnapshots, CapacityState
           → PlanTrigger.RATE_CHANGE or CAPACITY_CHANGE if changed

t=20s    Planner (if triggered)
           → Reads all state
           → Produces new Plan
           → Emits FlexibilityEnvelopes
           → Writes PlanWarnings → UserNotifications
```

---

## 3. Asset Layer

### 3.0 Asset Abstraction

Each asset exposes a uniform interface to the controller. The controller never calls
physics functions directly or reads simulation parameters.

```
trait AssetInterface {
    fn current(&self) -> f64;                              // kW now
    fn forecast(&self, horizon: Duration) -> Vec<(DateTime<Utc>, f64)>;  // predicted kW
    fn past(&self, window: Duration) -> Vec<(DateTime<Utc>, f64)>;       // recorded kW
}
```

Two implementations exist (or will exist):

| Implementation | Backend | Used by |
|---|---|---|
| `SimulatedAsset` | Physics model (sin, SOC, thermal) | All current VENs |
| `MeasuredAsset` | Real sensor / hardware API | Future real deployments |

From the controller's perspective these are identical. Swapping a `SimulatedAsset` for a
`MeasuredAsset` requires no changes outside that asset's module.

**Simulation parameters** (irradiation curve, initial SOC, rated power, thermal constants)
are only accessible through the `/sim` API endpoints. The controller never reads them.

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

### 3.3 Reactor (REMOVED)

> **The reactor was removed in spec kit 001 (2026-03-15).** The controller is the single
> control authority.

**Rationale:** The reactor (Phase 15) and controller (Phases 20–23) both read VTN events and
both wrote to the same `Setpoints` struct. The Dispatcher silently overwrote the reactor's
output for any asset with a plan allocation, making the reactor work redundant. The FSM
(Idle → Delaying → Ramping → Holding → RampingBack) and arbitration logic have been removed.
Transition smoothing, if needed, lives in the Dispatcher execution layer.

**New single control path:**
```
VTN events → openadr_interface → rates + capacity constraints
                                            │
User requests ──────────────────────────────┤
                                            ▼
                                        Planner
                                            │
                                        Dispatcher → Simulator setpoints
```

**Legacy:** `GET /trace` still exists and returns the reactor decision log (ring buffer, 1 000
entries). As of spec kit 001 this records Dispatcher decisions, not reactor FSM transitions.

---

## 4. API Contract

All routes are registered in `VEN/src/main.rs`. CORS is open. All handlers receive `State(ctx)`.

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
| GET | `/sim/override` | Current user override settings |
| POST | `/sim/override` | **Full-replace** override (EV charge limit, battery force kW, heater target, PV curtailment, initial SOC) |
| GET | `/sim/schema` | JSON schema for the profile YAML |
| POST | `/sim/reset/:id` | Reset a specific asset to its profile defaults |
| PUT | `/sim/config/battery` | Update battery configuration at runtime |
| GET | `/trace` | Reactor/Dispatcher decision trace log (newest first); optional `?limit=N` |

### 4.6 HEMS Controller

| Method | Path | Stage | Description |
|---|---|---|---|
| GET | `/rates` [RENAME → `/tariffs`] | 2 | `OadrEventSnapshot` array parsed from active events |
| GET | `/capacity` | 2 | `OadrCapacityState` parsed from active events |
| GET | `/obligations` | 2 | Pending report obligations extracted from events |
| GET | `/packets` | 3 | All EnergyPackets (FIRM + FLEXIBLE + terminal) |
| GET | `/plan` | 3 | Active Plan or `null` |
| POST | `/packets` | 4 | Create EnergyPacket + trigger reactive replanning |
| GET | `/ledger` | 4 | Per-asset cumulative energy / cost / CO₂ ledger |
| GET | `/user-requests` | 5 | All active user energy task requests |
| POST | `/user-requests` | 5 | Create user request with multi-tier deadline + budget |
| DELETE | `/user-requests/:id` | 5 | Cancel request → marks associated packet `ABANDONED` |
| GET | `/flexibility` | 5 | `FlexibilityEnvelopes` derived from active plan |

### 4.7 Recorded History — Storage Model Summary

| Endpoint | What it records | Storage | Max history |
|---|---|---|---|
| `GET /trace` | Dispatcher/control decisions (setpoints) | In-memory ring buffer (1 000 entries) | ≈ 16 min at 1 s |
| `GET /packets` `.past_power_profile` | Actual power per HEMS task while ACTIVE | In-memory, per-packet | Lifetime of packet |
| `GET /ledger` | Cumulative totals per asset since startup | In-memory, 1 s updates | Since restart |
| `GET /reports` | Discrete sim snapshots sent to VTN | Stored at VTN | Indefinite |

**No continuous power time series endpoint exists.** `/trace` is the closest substitute
(commanded setpoints at 1 s cadence, not measured watts).

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

### 5.2 Current Implementation (Audit)

The codebase uses three different strategies with no shared abstraction:

**Planner — tariff lookup** (`planner.rs:540–560`, `tariff_import_at()`):
Exact-interval containment: `interval_start ≤ ts < interval_end`. Applied at the slot
**start timestamp only**. A planning slot that spans a tariff boundary gets the rate from
the first half only — the boundary is invisible to the planner.

```
Tariff:   [10:00──────── €0.20 ────────11:00)  [11:00── €0.15 ──12:00)
Slots:    [10:30──────────────────── 11:10)
Lookup:   tariff_at(10:30) = €0.20  ← 10:57–11:10 billed at wrong rate
```

**Planner — capacity** (`openadr_interface.rs:236–320`):
Treated as a scalar state (strictest-limit-wins across all active events). Per-interval
capacity changes within an event are flattened to a single value — slot-level variation lost.

**Event merge** (`openadr_interface.rs:179–213`):
Last-write-wins when multiple events define the same interval. The OpenADR `priority` field
is parsed but not used in ordering. A higher-priority event that is processed first can be
silently overwritten by a lower-priority one processed later.

**UI stacked chart** (`GridAccumulatedCell.tsx:16–80`):
Nearest-neighbour binary search with 15 s tolerance. Breaks when assets have different
effective sampling rates (e.g. one asset downsampled to 30 s strides, another at 1 s).
Currently mitigated by excluding the `grid` virtual asset from timestamp collection to
avoid false zero-spikes. Known deferred fix — see BACKLOG RF-05.

**Report generation** (`reporter.rs`):
Latest snapshot only — no per-interval aggregation. Does not align to the report obligation's
`intervalPeriod`. Produces one data point regardless of reporting interval length.

### 5.3 Target Architecture — `TimeSeries<T>`

A single reusable abstraction should replace all ad-hoc lookups. See BACKLOG RF-05 and RF-06
for the implementation plan.

```
TimeSeries<T> {
    points:        Vec<(DateTime<Utc>, T)>,
    interpolation: Interpolation,  // Step | Linear | None
}

enum Interpolation {
    Step,    // LOCF — correct for tariffs, capacity limits, states
    Linear,  // correct for power, temperature, SOC
    None,    // cumulative values — aggregate only, never interpolate
}

impl<T> TimeSeries<T> {
    fn at(&self, ts: DateTime<Utc>) -> Option<T>
    // Evaluate at any timestamp using the declared interpolation rule.

    fn resample(&self, grid: &[DateTime<Utc>]) -> TimeSeries<T>
    // Project onto an arbitrary timestamp grid (union-of-breakpoints or fixed).

    fn merge(series: &[TimeSeries<T>]) -> TimeSeries<T>
    // Union-of-breakpoints merge: collect all breakpoints from all inputs,
    // evaluate each series at every breakpoint using its own rule.

    fn bucket(&self, width: Duration, agg: Aggregator) -> TimeSeries<T>
    // Downsample: mean (power), last (states), sum (cumulative).
}
```

**Planner slot costing (target behaviour):**
Instead of sampling tariff at `slot.start`, compute the time-weighted average across the slot:

```
effective_tariff(slot) =
  Σ( tariff_i × overlap(slot, interval_i) ) / slot.duration
```

A 5-min slot straddling a 10:57 boundary with €0.20 before and €0.15 after would give:
`(7 min × €0.20 + 3 min × €0.15) / 10 min = €0.185/kWh` — correct to three decimal places.

For capacity: `effective_limit(slot) = min(capacity_i for all intervals overlapping slot)`.

**Report generation (target behaviour):**
Evaluate `asset.history(interval_period)` bucketed to the obligation's interval grid.
Payload type determines aggregator: `USAGE → sum(kWh)`, `DEMAND → mean(kW)`,
`STORAGE_CHARGE_LEVEL → last(%)`, `BASELINE → last(kW)`.

### 5.4 OpenADR Spec Position

The spec defines interval structure but leaves VEN-side alignment to the implementer:
- Mixed `intervalPeriod` granularities within a single event (or across events) are legal.
- Reports may use `dataQuality = ESTIMATED` for interpolated/inferred values — acknowledged but unspecified.
- Event `priority` is defined but conflict resolution for overlapping same-type payloads is not specified; priority-based ordering before merge is the correct interpretation.

---

## 6. Design Decisions

### D-01: Greedy Planner (not LP/MILP)

**Decision:** Priority-based greedy scheduler.
**Rationale:** 24–48 h horizon, 3–15 assets, replanning every 20 s or on event. A greedy
approach with well-designed CalcCache produces near-optimal results and runs in milliseconds.
A full LP/MILP solver would add 100–500 ms latency and complexity without meaningful quality
gain at residential scale.

### D-02: In-Memory Ledger

**Decision:** `AssetLedger` is in-memory only; resets on restart.
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

**Decision:** `RateSnapshot` [RENAME → `OadrEventSnapshot`] holds all time-varying signals
(price, CO₂, capacity limits) in one struct per poll tick.
**Rationale:** All fields are co-valid at the same timestamp. A unified struct eliminates
temporal alignment bugs that arise when price and capacity signals are stored separately.
See REQUIREMENTS §3.2.2.

### D-06: POST /sim/override is Full-Replace

**Decision:** `POST /sim/override` replaces the entire override struct.
**Rationale:** Partial-patch semantics (PATCH) require null-vs-absent disambiguation.
Full-replace is simpler and explicit. Callers must set all fields they want active.

### D-07: 30 s Fixed Poll Interval

**Decision:** Event polling is fixed at 30 s.
**Rationale:** Balances VTN load against response latency. The 30–60 s range from system_design
was narrowed to 30 s fixed in implementation. Configurable jitter is not implemented in the lab.
