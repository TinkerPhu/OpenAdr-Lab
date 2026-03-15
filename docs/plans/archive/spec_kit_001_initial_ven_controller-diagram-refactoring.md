# VEN Controller — Diagram Refactoring & Architectural Decisions

**Date**: 2026-03-15
**Status**: Architectural decisions captured — ready for implementation planning
**Scope**: VEN UI (controller-v2), VEN API (trace/timeline endpoints), VEN backend (simulator, reactor, controller, trace)

---

## 1. Immediate UI Fixes (Asset Cell Mid Section Diagram)

### What the diagram must show

Three series per asset cell, all in the same asset color, on a shared time axis:

| Series | Line style | Y-axis | Unit |
|---|---|---|---|
| Power | solid | left | kW |
| Cost rate | dashed | right | €/h |
| CO₂eq rate | dotted | right | CO₂eq/h |

- **Time window**: fixed ±1 h centred on now (2 h total) — not auto-fit to data
- **Present marker**: vertical red dotted line at the centre of the X axis
- **Line style identifies the series** — fixed across the full time span; past vs. future is communicated by position relative to the NOW line only, not by style change
- **`isPast` field**: remove from `AssetTimePoint` — it is unused and the concept is implicit in timestamp position
- **Negative values** go below the X axis (export direction)
- **Asset name** shown as a small title in both the left and mid sections (not "Power" as a title)

### Root causes of current diagram failures

| Issue | Location | Cause |
|---|---|---|
| No past data for battery / base_load | `dataBuilders.ts: getTraceAssetPower` | `switch` falls through to `default: return null` for both |
| PV past data shows setpoint limit, not generation | `dataBuilders.ts:262` | Uses `pv_export_limit_kw` (reactor control signal) instead of actual `pv.current_kw` |
| X-axis domain is auto-fit | `AssetTimelineChart.tsx:32` | `domain={["auto","auto"]}` — NOW reference line can fall outside visible area |
| `nowMs` not memoized | `ControllerV2.tsx:13` | `Date.now()` called at render time — invalidates all memos on every render |

**Rule**: do not substitute wrong data (e.g. setpoints) when real data is unavailable. Leave the gap visually empty and indicate the root cause in a code comment so it is traceable.

---

## 2. Architectural Decision: Reactor Removal

### Decision
The **reactor is removed**. It is replaced by the controller as the single control authority.

### Rationale
The reactor and controller are architecturally redundant:

- The reactor reads VTN OpenADR events directly and produces immediate setpoints (old path, built in Phase 15).
- The controller's `openadr_interface` parses the same events into rates and capacity constraints and feeds them to the planner (new path, built in Phases 20–23).
- Both paths write to the same `Setpoints` struct. The dispatcher silently overwrites the reactor for any asset it has a plan allocation for — making the reactor's work for those assets discarded.
- The FSM (ramp, delay, hold, ramp-back states) and arbitration logic in the reactor are removed along with it. Transition smoothing, if needed, moves into the dispatcher execution layer.

### New single control path
```
VTN events
    │
    └──► openadr_interface  →  rates + capacity constraints
                                        │
User requests ──────────────────────────┤
                                        ▼
                                   planner  (reactive: triggers on event change, request, completion)
                                        │
                                        ▼
                                  dispatcher  →  setpoints  →  simulator
```

---

## 3. Architectural Decision: Force-Override Removal

### Decision
`UserOverrides` force-override fields (`ev_force_kw`, `heater_force_kw`, `battery_force_kw`, `pv_force_export_limit_kw`) are **removed**.

### Rationale
Force-overrides bypass both the reactor and the controller/planner. The planner never sees them, so it produces plans that conflict with what the simulator is actually executing. A force-override to 0 kW means a packet accumulates no energy while the plan still believes the asset is delivering — the planner cannot replan because it was never notified.

### Replacement
User intent is expressed through **user requests with high-priority value curves** (high willingness-to-pay bids, tight deadlines). The planner schedules these immediately and the dispatcher executes them. The planner remains the single authority.

---

## 4. Architectural Decision: Trace Reform

### Decision
The single reactor-centric trace is replaced by two separate trace structures:

#### 4a. Controller Event Log (sparse, event-driven)
Records *why the controller made decisions*. Entries are added on state changes, not every tick. One ring buffer.

```
ControllerEvent {
    ts
    kind: OpenAdrArrived   { event_name, signal_type, value, interval }
        | OpenAdrExpired    { event_name }
        | RateChange        { interval_start, import_eur_kwh, export_eur_kwh }
        | CapacityChange    { import_limit_kw, export_limit_kw }
        | PlanCycle         { trigger_reason, firm_slots, flexible_slots }
        | PacketTransition  { packet_id, asset_id, from_status, to_status }
        | RequestTransition { request_id, asset_id, from_status, to_status }
}
```

#### 4b. Asset History Trace (dense, one entry per sim tick, per asset)
Records measured physical values and controller-derived economic values. Stored as one ring buffer per asset: `HashMap<AssetId, RingBuffer<AssetHistoryPoint>>`.

```
AssetHistoryPoint {
    ts:     DateTime<Utc>,
    values: HashMap<String, f64>,   // all values — no distinction between physical and derived
}
```

**Well-known keys:**

| Key | Applies to |
|---|---|
| `power_kw` | all assets, grid |
| `cost_rate_eur_h` | all assets, grid |
| `co2_rate_g_h` | all assets, grid |
| `soc_pct` | EV, Battery |
| `temp_c` | Heater |
| `irradiance` | PV |
| `import_price_eur_kwh` | grid |
| `export_price_eur_kwh` | grid |
| `import_limit_kw` | grid |
| `export_limit_kw` | grid |

- No `is_forecast` flag — past vs. future is implicit in `ts` vs. `now`
- No separate "derived" map — `cost_rate_eur_h` and `co2_rate_g_h` are on the same generic level as `power_kw`
- `cost_rate_eur_h` = `power_kw` × current tariff — computed by `controller/monitor.rs` which has access to both sim snapshot and rate schedule at tick time
- The join between physics and economics happens **once at write time** in the backend, not on every render in the UI

#### Recording location
Both traces are written from `controller/monitor.rs`, which runs post-tick with access to `SimSnapshot` and `RateSnapshot[]`. The controller event log entries are additionally written from `openadr_interface`, `planner`, `user_request`, and `dispatcher` at their respective transition points.

---

## 5. Architectural Decision: Backend-Assembled Timeline Endpoint

### Decision
The UI no longer assembles asset timelines from multiple sources. The backend provides a **unified timeline per asset** via a new endpoint.

### Endpoint
`GET /timeline/{asset_id}?hours_back=1&hours_forward=3`

Returns `Vec<AssetTimelinePoint>` — a flat sorted array covering the full requested time window:
- Past entries: from `AssetHistoryTrace` ring buffer (real measured values)
- Future entries: from current `Plan` allocations for this asset + rate schedule for derived economic values

```
AssetTimelinePoint {
    ts:     DateTime<Utc>,
    values: HashMap<String, f64>,   // same structure as AssetHistoryPoint
}
```

`GET /timeline/grid` returns the same structure for net power, tariffs, and capacity limits.

### Motivation
The current `buildAssetTimeline`, `buildTariffTimeline`, and `buildStackedAreaData` in `dataBuilders.ts` exist solely to join `/trace` + `/plan` + `/rates` in the frontend. This logic is already partially broken (battery/base_load missing, PV wrong). Moving assembly to the backend — where all data is available and correct — eliminates the problem class. The UI calls one endpoint per asset and renders a flat array.

### New backend module
`controller/timeline.rs` — a pure function:
```
fn build_asset_timeline(
    asset_id: &str,
    history: &RingBuffer<AssetHistoryPoint>,
    plan: &Plan,
    rates: &[RateSnapshot],
    window: TimeWindow,
) -> Vec<AssetTimelinePoint>
```

No side effects, fully unit-testable. `dataBuilders.ts` in the frontend effectively becomes the specification for this function.

---

## 6. Architectural Decision: Simulator Modularisation

### Directory structure
```
simulator/
  mod.rs          — SimState (Vec<AssetEntry> + GridMeter), tick(), to_sim_snapshot()
  assets/
    mod.rs        — AssetState enum + dispatch (update, predict, state_values, control_schema)
    ev.rs         — EvCharger, EvConfig, all EV physics + prediction
    heater.rs     — Heater, HeaterConfig, thermal model + prediction
    pv.rs         — PvInverter, PvConfig, irradiance model + prediction
    battery.rs    — Battery, BatteryConfig, SoC model + prediction
    base_load.rs  — BaseLoad, BaseLoadConfig
  energy.rs       — EnergyCounter (utility, used per-asset and by GridMeter)
  persist.rs      — save/load SimState (utility)
  power_model.rs  — compute net power at grid boundary (utility)
```

### AssetEntry — fully self-contained
Each asset owns its setpoint, its energy counter, and its physics state:

```
AssetEntry {
    id:       String,
    state:    AssetState,          // physics state (enum dispatch)
    setpoint: f64,                 // last commanded value written by dispatcher
    energy:   EnergyCounter,       // cumulative kWh for this asset
}
```

### AssetState enum — single integration point for new types
```
enum AssetState {
    Ev(EvCharger),
    Heater(Heater),
    Pv(PvInverter),
    Battery(Battery),
    BaseLoad(BaseLoad),
}
```

Adding a new asset type requires:
1. One new file `simulator/assets/new_type.rs`
2. One new variant in `AssetState` — compiler enforces all match arms

The enum update is a deliberate constraint: the compiler guides the developer to every integration point and prevents missed cases. It is not a limitation but a safety mechanism.

### AssetState interface (methods on the enum, delegated to each type)
- `update(dt_s, env: &TickEnvironment) -> f64` — one tick, returns actual power_kw
- `predict(setpoint, horizon, env) -> Vec<AssetTimelinePoint>` — forward projection using own physics model
- `state_values() -> HashMap<String, f64>` — current asset-specific state (soc_pct, temp_c, etc.)
- `control_schema() -> Vec<ControlDescriptor>` — describes what the UI can control (see §7)

### TickEnvironment — dynamic auxiliary inputs
Environmental inputs are a generic map, not named fields:
```
TickEnvironment: HashMap<String, f64>
// e.g. {"hour_of_day": 13.5, "ambient_temp_c": 8.0, "irradiance_override": 0.7}
```
Each asset reads what it needs and ignores the rest. Adding new environmental inputs requires no struct change.

### GridMeter — separate from Vec<AssetEntry>
`net_power_w`, `import_w`, `export_w`, `voltage_v`, `import_kwh`, `export_kwh` are the state of a simulated utility meter at the grid connection point. They are **derived** (computed from summing asset outputs after each tick) and are not controlled. Modelling them as an asset would create tick-ordering dependency (grid meter needs all other assets to have ticked first) and break the uniform asset interface.

`GridMeter` lives alongside `Vec<AssetEntry>` in `SimState` but is not part of it. It gets its own timeline entry keyed `"grid"` with the same generic `values` map structure.

Note: `GridMeter.import_kwh/export_kwh` differs from the sum of per-asset energy counters because of self-consumption (e.g. PV powering a load directly without crossing the grid boundary). Both levels are necessary and mean different things.

**Finding — simulation vs. reality inversion**: In simulation, `GridMeter` is derived from assets (sum). In a real installation the relationship is reversed: the grid meter is the measured truth and `BaseLoad` is the residual (`GridMeter − sum(known assets)`). The current architecture is simulation-only and correctly derives `GridMeter` from assets. When implementing, do not bake in the assumption that `GridMeter` is always derived — keep the derivation logic confined to `simulator/mod.rs` tick function so that a future real-device mode can invert the direction without touching the controller, timeline, or UI. Do not implement a real-sensor constructor now (it would be unused code); just avoid closing the door architecturally.

### SimSnapshot — consistent with AssetTimelinePoint
```
SimSnapshot {
    ts, net_power_w, import_w, export_w, voltage_v, import_kwh, export_kwh
    assets: HashMap<String, AssetSnapshot {
        power_kw: f64,
        values:   HashMap<String, f64>   // same keys as AssetHistoryPoint
    }>
}
```
No separate typed snapshot structs (EvSnapshot, HeaterSnapshot, etc.).

### Setpoints — generic map
Replaces the hardcoded `Setpoints` struct:
```
HashMap<String, f64>   // asset_id → commanded value
```
Each asset's `update()` interprets the value according to its own semantics (EV: charge target kW; PV: export ceiling kW; battery: signed charge/discharge kW).

---

## 7. Architectural Decision: Dynamic UI Control Schema

### Decision
The UI asset cell right section (simulation controls) is rendered dynamically from a schema provided by each asset type. No hardcoded per-asset-type control logic in the frontend.

### Schema structure
```
AssetControlSchema {
    asset_id: String,
    controls: Vec<ControlDescriptor {
        key:   String,          // parameter name, used as key in override POST body
        label: String,          // display label
        kind:  Slider | Switch | NumberInput,
        min:   Option<f64>,
        max:   Option<f64>,
        unit:  String,
    }>
}
```

Served via `GET /sim/schema` or embedded in `GET /sim` response.

The UI reads the schema and renders the correct controls for whatever assets are configured. A new asset type automatically gets the right controls in the UI without any frontend code change.

User control actions POST a generic `HashMap<String, f64>` body to `POST /sim/override` (or a new request endpoint), keyed by the schema's `key` fields.

---

## 8. Architectural Decision: Asset History Buffer — Columnar Internal Layout

### Decision
The `AssetHistoryBuffer` ring buffer is stored **columnar internally** and **transposed to row-oriented on serialisation**.

### Rationale
`VecDeque<AssetHistoryPoint>` where each `AssetHistoryPoint` contains a `HashMap<String, f64>` is valid Rust — `HashMap` has a fixed stack size (pointer + capacity + length, 3 words) regardless of how many entries it holds, so all ring buffer slots are the same size. The variable-length key-value data lives on the heap per entry.

However, a per-entry `HashMap` is wasteful for a dense time series:
- 1000 entries × 5 assets = 5000 small heap-allocated HashMaps
- Plotting reads all `power_kw` values sequentially — per-entry maps are cache-unfriendly for this access pattern

### Columnar layout (internal)
```rust
struct AssetHistoryBuffer {
    timestamps: VecDeque<DateTime<Utc>>,
    columns:    HashMap<String, VecDeque<f64>>,  // one deque per value key, across all entries
}
```

- One `HashMap` at the buffer level, not per entry
- Each column (`power_kw`, `soc_pct`, `cost_rate_eur_h`, ...) is a contiguous `VecDeque<f64>` — memory-efficient and cache-friendly for sequential reads
- Adding a new value key (e.g. `irradiance` for a new asset type) adds one new column deque — no struct change
- Missing values for a given entry represented as `f64::NAN` or `Option<f64>` per column

### Row-oriented on the wire
The internal columnar layout is an implementation detail. The HTTP response transposes to row-oriented `Vec<AssetTimelinePoint>` where each point carries `{ ts, values: HashMap<String, f64> }`. The UI always sees the same flat row structure regardless of internal storage.

### Where this lives
`controller/trace.rs` — `AssetHistoryBuffer` struct with `push(ts, key, value)` and `to_timeline(window) -> Vec<AssetTimelinePoint>` methods.

---

## 10. Summary of API Changes

| Current endpoint | Change |
|---|---|
| `GET /trace` | Replaced by `GET /trace/events` (controller event log) and `GET /trace/history?asset=<id>` (asset history) |
| `GET /sim` | Response structure changes to generic `assets: HashMap<String, AssetSnapshot>` |
| `POST /sim/override` | Body becomes generic `HashMap<String, f64>` keyed by control schema keys; force-override fields removed |
| *(new)* `GET /timeline/{asset_id}` | Unified past+future timeline per asset |
| *(new)* `GET /timeline/all` | All-asset timelines in one call — `HashMap<asset_id, Vec<AssetTimelinePoint>>` — used by stacked area view and to prefetch all cells |
| *(new)* `GET /timeline/grid` | Unified past+future grid timeline (tariffs, capacity, net power) |
| *(new)* `GET /sim/schema` | Asset control schemas (or embedded in GET /sim) |
| `GET /rates` | **Renamed** to `GET /tariffs` (see §18) |
| *(new)* `POST /sim/reset/{asset_id}` | One-shot asset state initialisation (replaces `ev_initial_soc`, `battery_initial_soc` stubs) |
| *(new)* `PUT /sim/config/{asset_id}` | Runtime device-spec update (replaces `battery_capacity_kwh` stub) |

---

## 11. Summary of VEN Backend Changes

| Component | Change |
|---|---|
| `reactor/` | **Removed entirely** (mod, fsm, arbitration, interval, trace) |
| `state.rs: UserOverrides` | Force-override fields removed; only environment inputs and device-spec overrides remain (or moved into asset control schema mechanism) |
| `profiles/ven-N.yaml` | `assets:` becomes a typed list (see §20); named device fields removed |
| `profile.rs: DeviceConfig` | Replaced by `Vec<AssetConfig>` with serde tag dispatch |
| `simulator/actors.rs` | Split into `simulator/assets/{ev,heater,pv,battery,base_load}.rs` |
| `simulator/mod.rs` | `SimState` uses `Vec<AssetEntry>`; `SimSnapshot` uses generic maps |
| `reactor/trace.rs` | Replaced by `controller/trace.rs` with `ControllerEventLog` and `AssetHistoryBuffer` (columnar ring buffer) |
| `controller/monitor.rs` | Extended: writes `AssetHistoryBuffer` each tick AND handles all packet energy accounting (replaces dispatcher's `update_packets`) |
| `controller/dispatcher.rs` | Simplified: current-tick plan scan only; writes `HashMap<String, f64>` setpoints; packet accounting removed (moved to monitor) |
| *(new)* `controller/timeline.rs` | Assembles `Vec<AssetTimelinePoint>` from history + plan + rates on demand; also serves `/timeline/all` |
| `reporter.rs` | Moved into `controller/reporter.rs`; split into `build_measurement_report` and `build_status_report`; `reactor_mode` removed |
| `entities/rate_snapshot.rs` | **Renamed** to `entities/tariff_snapshot.rs`; `RateSnapshot` → `TariffSnapshot`, `PlannedRates` → `PlannedTariffs`, `PastRates` → `PastTariffs` |
| `state.rs: UserOverrides` stubs | `ev_initial_soc`, `battery_initial_soc`, `battery_capacity_kwh` removed; replaced by `POST /sim/reset/{asset_id}` and `PUT /sim/config/{asset_id}` |

---

## 12. Summary of UI Changes (VEN controller-v2)

| Component | Change |
|---|---|
| `dataBuilders.ts` | `buildAssetTimeline`, `buildTariffTimeline`, `buildStackedAreaData` removed — replaced by backend timeline endpoint calls |
| `AssetTimelineChart.tsx` | X-axis: fixed ±1h domain by default; `isPast` logic removed; no style change across now line; per-cell extended-view toggle (icon in cell header) switches to a longer horizon (see §13) |
| `AssetMidSection.tsx` | Pass asset name as title |
| `AssetRightSection.tsx` | Controls rendered dynamically from `AssetControlSchema`; no hardcoded per-asset logic |
| `GridAccumulatedCell` | Uses `GET /timeline/all` response — no local data assembly; one request covers all assets |
| `types.ts` | `AssetTimePoint.isPast` removed; new `AssetTimelinePoint` with flat `values` map |
| API hooks | `useRates()` renamed to `useTariffs()`; add `useTimeline(assetId, hoursBack, hoursForward)` and `useAllTimelines(hoursBack, hoursForward)` hooks; remove direct `/trace` + `/plan` + `/tariffs` joins in dataBuilders |

---

## 13. Extended Time Window (Per-Cell Toggle)

### Decision
Each cell's mid section diagram has a toggle icon in the cell header. One click switches between the **default short window** and an **extended window**. State is per-cell, session-scoped. No checkbox — a single icon button that visually indicates the active mode.

### Window parameters per cell type

| Cell | Default | Extended | Past in extended view |
|---|---|---|---|
| Tariff | ±1h | 24h forward | No — past tariffs are less useful than the forward schedule; `hours_back=0, hours_forward=24` |
| Capacity limits | ±1h | 24h forward | No — same rationale as tariff |
| EV | ±1h | Until SoC target reached (or 24h cap) | Yes, ±short past for context |
| Battery | ±1h | 24h | Yes, short past for context |
| Plan overview / stacked chart | ±1h | Full plan horizon (24h) | Yes |
| Other assets (heater, PV, base load) | ±1h | No extended view needed | — |

### Implementation
The `useTimeline(assetId, hoursBack, hoursForward)` hook passes the window as query parameters to `GET /timeline/{asset_id}?hours_back=N&hours_forward=M`. The toggle in the cell header updates these parameters. The backend returns exactly the requested window — no frontend trimming needed.

---

## 14. Architectural Decision: VTN Reporting — Dual-Mode

### Decision
Reporting to the VTN operates in two modes with different triggers and different content. Both replace the current single timer-driven reporter that depends on `reactor_mode`.

#### Timer-driven: measurement reports
- **Trigger**: periodic timer (`report_interval_s`), unchanged from current behaviour
- **Content**: TELEMETRY_USAGE / READING — measured values from `AssetHistoryBuffer` (per-asset power, cumulative energy, SoC where available)
- **Source**: `AssetHistoryBuffer` replaces `SimSnapshot + reactor_mode`
- **Function**: `build_measurement_report(asset_history, now) -> OpenAdrReport`

#### Event-driven: status reports
- **Trigger**: controller events — OpenADR event arrival/expiry, plan cycle completion, packet status transition
- **Content**: TELEMETRY_STATUS — confirms the VEN's current response status to the VTN (e.g. "responding to ImportCapLimit event X, currently delivering at Y kW")
- **Source**: `ControllerEventLog` latest relevant entry
- **Function**: `build_status_report(event: &ControllerEvent, asset_history, now) -> OpenAdrReport`

### reporter.rs changes
`reactor_mode: &str` parameter removed from all report builders. Two builder functions replace the current single function. The event-driven builder is called from `controller/mod.rs` wherever controller events are emitted, not from the tick loop.

---

## 15. Architectural Decision: Asset Capability Interface for the Planner

### Decision
Each asset exposes a `capabilities()` method. The planner queries this rather than embedding per-asset-type knowledge.

```
AssetCapabilities {
    asset_id:      String,
    max_import_kw: f64,
    max_export_kw: f64,         // 0.0 for unidirectional assets
    is_flexible:   bool,        // true = planner may create power allocations for this asset
    energy_state:  Option<EnergyState {
        current_kwh: f64,       // e.g. SoC expressed as kWh remaining
        min_kwh:     f64,
        max_kwh:     f64,
    }>,
    availability:  Option<TimeWindow>,  // e.g. EV latest departure time
}
```

### Flexible vs. non-flexible assets

| Asset | `is_flexible` | Rationale |
|---|---|---|
| EV | true | Planner schedules charge/discharge allocations |
| Battery | true | Planner schedules charge/discharge allocations |
| Heater | true | Planner reduces load during high-price periods |
| PV | false | Output follows physics; planner does not command power |
| BaseLoad | false | Uncontrolled residual; planner uses forecast as background |

Non-flexible assets are included in the plan's net power accounting via their `predict()` output — the planner needs to know "PV will generate 3 kW at noon" to correctly size flexible allocations within capacity limits. They simply never receive a power allocation in a plan slot.

### PV export limit nuance
`is_flexible: false` means the planner does not schedule power allocations for PV. It does not mean PV has no controllable parameters. `export_limit_kw` is a grid compliance constraint written by the dispatcher in direct response to active ExportCapLimit events — it is not a planning decision. The asset control schema (§7) manages this independently of `is_flexible`.

### Heater prediction accuracy
The heater is `is_flexible: true` but has autonomous thermostat behaviour (overrides when below `temp_min_c`). The heater's `predict()` must incorporate the thermal model so the planner receives realistic forecasts. If the planner allocates zero heater power during a cold night, the thermostat will override it — the planner must be aware of this rather than treating the heater as a simple on/off load.

---

## 16. Architectural Decision: Idle Setpoints — Asset-Owned Defaults (Option A)

### Decision
Each asset defines its own `default_setpoint() -> f64` — its natural operating point when no plan allocation is active. The dispatcher owns idle fallback: if no FIRM slot covers an asset for the current tick, it uses `asset.default_setpoint()`.

### Rationale
- Matches the real nature of assets: a physical EV charger has a default behaviour when no DR signal is present; a real heater has a thermostat setpoint
- The planner only produces slots for flexible assets with active packets or requests — no "idle filler" slots needed, simpler plan structure
- Graceful degradation: if the planner fails or produces no output, all assets continue with their defaults rather than going to zero
- One code path in the dispatcher: always `plan_setpoint.unwrap_or(asset.default_setpoint())`

### Planner still accounts for defaults
When checking whether a proposed plan stays within capacity limits, the planner sums:
- Flexible asset allocations (from plan slots)
- Non-flexible asset predictions (PV, base_load via `predict()`)
- Flexible asset defaults for time periods not yet covered by a plan slot

This ensures capacity constraint checks are accurate even for partially planned horizons.

### No central Setpoints struct
The current `Setpoints { ev_charge_kw, heater_kw, pv_export_limit_kw, battery_kw }` struct is removed along with the reactor. Setpoints are a `HashMap<String, f64>` assembled by the dispatcher each tick from plan allocations and asset defaults.

---

## 17. Architectural Decision: Simulation Initialisation Endpoints (Stub Replacement)

### Decision
Three `UserOverrides` stub fields are replaced with proper first-class endpoints. Implemented as part of speckit 1 (simulator reform) alongside profile generalisation.

### Stubs to replace

| Stub field | Current behaviour | Replacement |
|---|---|---|
| `ev_initial_soc: Option<f64>` | One-shot SoC jump written via `POST /sim/override` | `POST /sim/reset/{asset_id}` with `{ "soc": 0.8 }` — writes directly into `AssetEntry.state` |
| `battery_initial_soc: Option<f64>` | Same | Same pattern |
| `battery_capacity_kwh: Option<f64>` | Override battery capacity at runtime | `PUT /sim/config/{asset_id}` with `{ "capacity_kwh": 20.0 }` — updates the asset's config struct in place |

### Rationale
Force-overrides for `initial_soc` bypass the normal physics model (setting SoC mid-simulation is a test/reset action, not a control signal). `battery_capacity_kwh` is a device specification, not an operational setpoint. Both belong in explicit reset/config endpoints rather than the `POST /sim/override` body.

### Implementation
- `POST /sim/reset/{asset_id}` — writes initial state values (e.g. `soc`) directly into the matching `AssetEntry.state` via a `reset(values: HashMap<String, f64>)` method on `AssetState`
- `PUT /sim/config/{asset_id}` — updates the asset's config (e.g. `capacity_kwh`) in the matching `AssetEntry.state` via a `update_config(values: HashMap<String, f64>)` method
- Both endpoints replace the stub fields in `UserOverrides`; the stub fields are removed from `UserOverrides` and from `POST /sim/override`
- Persist changes to `sim_state.json` after each call

---

## 18. Architectural Decision: API Rename — `/rates` → `/tariffs`

### Decision
Rename the VEN API endpoint and all associated types to match the project nomenclature: **tariff = X/kWh** (price per unit of energy), **rate = X/h** (instantaneous flow). Implemented across speckit 2 (backend + API) and speckit 3 (UI).

### Nomenclature conflict
`GET /rates` and `RateSnapshot` use "rate" to mean a per-kWh tariff value. This conflicts throughout the codebase wherever "rate" is used for X/h values (`cost_rate_eur_h`, `co2_rate_g_h`). Having both meanings in active use creates persistent ambiguity in code review and debugging.

### Rename scope

| Old name | New name | Location |
|---|---|---|
| `GET /rates` | `GET /tariffs` | `VEN/src/main.rs` route |
| `RateSnapshot` | `TariffSnapshot` | `VEN/src/entities/rate_snapshot.rs` → `tariff_snapshot.rs` |
| `PlannedRates` | `PlannedTariffs` | same file |
| `PastRates` | `PastTariffs` | same file |
| `useRates()` | `useTariffs()` | `VEN/ui/src/api/hooks.ts` |
| `rates` prop/variable | `tariffs` | all UI components that pass tariff data |

### Speckit assignment
- **Speckit 2**: backend rename — entity file, route, all Rust callers
- **Speckit 3**: UI rename — hooks, component props, TypeScript types

### Note on existing code comments
`dataBuilders.ts` already contains a comment `// Note: RateSnapshot is the API type name; the data it holds is tariff (per-kWh)`. This annotation becomes unnecessary after the rename — remove it.

---

## 19. Architectural Decision: Dispatcher Redesign + Tick Loop

### Dispatcher responsibilities (post-reactor-removal)

The dispatcher is simplified to a single concern: **translate the current plan into per-asset setpoints for this tick**.

Packet energy accounting is moved to `monitor.rs` (B2). The dispatcher no longer calls `update_packets()`.

#### Plan lookup — current-tick scan (A1)

On each tick, the dispatcher scans the plan's FIRM and FLEXIBLE slots for the slot whose time range covers `now` and extracts allocations:

```rust
fn build_setpoints(plan: &Plan, assets: &[AssetEntry], now: DateTime<Utc>) -> HashMap<String, f64> {
    let mut setpoints = HashMap::new();
    let all_slots = plan.firm_slots.iter().chain(plan.flexible_slots.iter());
    for slot in all_slots {
        if slot.start <= now && now < slot.end {
            for alloc in &slot.allocations {
                setpoints.insert(alloc.asset_id.clone(), alloc.power_kw);
            }
        }
    }
    // Fill gaps with asset-owned defaults (§16)
    for asset in assets {
        setpoints.entry(asset.id.clone()).or_insert_with(|| asset.state.default_setpoint());
    }
    setpoints
}
```

The plan is short (≤24h, ≤dozens of slots); scanning it every tick is negligible. No pre-computed cache needed.

#### FLEXIBLE slots treated same as FIRM (C1)

The planner already made the FIRM/FLEXIBLE distinction with full context at planning time. The dispatcher executes both at their stated allocations. No runtime scaling or headroom check — that is the planner's job.

#### PV export limit — direct dispatch, not a plan allocation

`ExportCapLimit` compliance is enforced by the dispatcher directly: if an active `ExportCapLimit` capacity constraint is present, the dispatcher writes `pv_export_limit_kw` to the PV asset's setpoint regardless of plan content. This is a hard constraint, not a planning decision (see §15).

### Tick loop sequence (D)

```
1. dispatcher.write_setpoints(plan, assets, capacity_limits, now)
       → HashMap<asset_id, f64> written into each AssetEntry.setpoint
2. sim.tick(dt, env)
       → each AssetEntry.state.update(dt, env) using its current setpoint
       → returns actual power_kw per asset
       → GridMeter derived from sum of asset outputs
3. monitor.record_tick(sim_snapshot, rates, now)
       → appends to AssetHistoryBuffer (power + state_values + cost/CO₂)
       → updates packet energy counters (energy delta × dt attribution)
       → updates AssetLedger
4. reporter.maybe_send_measurement(asset_history, now)
       → timer check; if due, builds and POSTs measurement report to VTN
```

Step 4 fires only on the measurement timer interval. Event-driven status reports are fired from `controller/mod.rs` wherever `ControllerEvent` entries are emitted (not in the tick loop).

### monitor.rs — packet energy accounting (B2)

After each tick, monitor reads the actual power delivered per asset from `SimSnapshot` and attributes the energy delta to the asset's active packet:

```rust
fn record_tick(&mut self, snapshot: &SimSnapshot, rates: &[RateSnapshot], dt_s: f64, now: DateTime<Utc>) {
    for (asset_id, asset_snap) in &snapshot.assets {
        // 1. Write to history buffer
        self.history.push(asset_id, now, &asset_snap.values);
        // 2. Attribute energy to active packet
        let energy_kwh = asset_snap.power_kw * (dt_s / 3600.0);
        self.update_packet_energy(asset_id, energy_kwh, now);
        // 3. Update ledger
        self.update_ledger(asset_id, asset_snap, rates, now);
    }
}
```

Centralising accounting in monitor avoids the dispatcher needing to reach back into sim internals after the tick.

---

## 18. Architectural Decision: Profile YAML Generalisation

### Decision
Asset configuration in `ven-N.yaml` changes from named device fields to a typed list (P1). Each entry carries a `type` discriminant for serde tag dispatch.

### New YAML format

```yaml
assets:
  - type: ev
    id: ev
    max_charge_kw: 11.0
    max_discharge_kw: 0.0
    battery_kwh: 60.0
    initial_soc: 0.8
    default_charge_kw: 7.0

  - type: heater
    id: heater
    max_kw: 2.0
    temp_min_c: 18.0
    temp_max_c: 22.0
    default_setpoint_c: 20.0

  - type: pv
    id: pv
    peak_kw: 5.0

  - type: battery
    id: battery
    max_charge_kw: 5.0
    max_discharge_kw: 5.0
    capacity_kwh: 10.0
    initial_soc: 0.5

  - type: base_load
    id: base_load
    baseline_kw: 0.4
```

### Rust profile types

```rust
#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum AssetConfig {
    Ev(EvConfig),
    Heater(HeaterConfig),
    Pv(PvConfig),
    Battery(BatteryConfig),
    BaseLoad(BaseLoadConfig),
}

#[derive(Deserialize)]
struct ProfileConfig {
    assets: Vec<AssetConfig>,
    // ... other profile fields (ven_name, report_interval_s, etc.)
}
```

Each `AssetConfig` variant owns its own config struct (defined in the corresponding `simulator/assets/*.rs` file). Adding a new asset type requires one new variant in the enum — the compiler enforces that all match arms are handled everywhere `AssetConfig` is used.

### Migration
Existing `ven-1.yaml`, `ven-2.yaml`, `ven-3.yaml`, and `test.yaml` are rewritten to the new list format. The old `DeviceConfig` struct in `profile.rs` is replaced by `Vec<AssetConfig>`.

---

## 19. Architectural Decision: Stacked Area / GridAccumulatedCell Data Source

### Decision
`GridAccumulatedCell` (stacked area chart) uses `GET /timeline/all` (S2). This is a single endpoint that returns timelines for all configured assets in one call.

### Endpoint

```
GET /timeline/all?hours_back=1&hours_forward=3
```

Response:
```json
{
  "ev":        [{ "ts": "...", "values": { "power_kw": 7.2, ... } }, ...],
  "heater":    [...],
  "pv":        [...],
  "battery":   [...],
  "base_load": [...],
  "grid":      [...]
}
```

`HashMap<String, Vec<AssetTimelinePoint>>` — the same `AssetTimelinePoint` structure used by `/timeline/{asset_id}`.

### Why one endpoint covers both use cases

- **Individual asset cells**: each cell can use its own slice from the `/timeline/all` response, keyed by `asset_id`. This replaces N parallel `/timeline/{id}` calls on page load with a single fetch.
- **Stacked area**: the UI zips the per-asset `power_kw` series by timestamp. Because all assets are sampled on the same 1s tick clock, timestamps align exactly — no interpolation needed.

### Backend implementation

`controller/timeline.rs` already iterates over all assets to serve `/timeline/{asset_id}`. Serving `/timeline/all` is the same loop without the asset filter:

```rust
fn build_all_timelines(
    history: &HashMap<String, AssetHistoryBuffer>,
    plan: &Plan,
    rates: &[RateSnapshot],
    window: TimeWindow,
) -> HashMap<String, Vec<AssetTimelinePoint>>
```

### UI hook

```ts
useAllTimelines(hoursBack: number, hoursForward: number)
// → { data: Record<AssetId | "grid", AssetTimelinePoint[]> }
```

`GridAccumulatedCell` calls `useAllTimelines`. Individual `AssetCell` components can either call `useTimeline(assetId, ...)` independently or receive their slice from a parent that called `useAllTimelines` — to be decided at implementation time based on which avoids duplicate fetches.

---

## 20. Architectural Decision: controller/ Module Organisation

### Decision
All controller modules stay in `controller/`. No `openadr/` sub-module is split off.

### Rationale
The two "VTN-facing" files (`openadr_interface.rs` and `reporter.rs`) are tightly coupled to `controller/trace.rs`:
- `openadr_interface.rs` feeds parsed rates directly into planner state
- `reporter.rs` reads from `AssetHistoryBuffer` and `ControllerEventLog` to build report payloads

Moving them to an `openadr/` module would require `openadr/` to import `controller/trace.rs`, making the adapter layer depend on the core. The coupling is inherent and the split adds plumbing without a genuine boundary.

The VTN HTTP transport (polling `/events`, POSTing reports) already lives outside `controller/` in the VEN API client layer. What remains in `controller/openadr_interface.rs` is interpretation — converting API types to entity types — which correctly belongs in the controller.

### Internal module grouping

The logical structure is documented as grouped declarations in `controller/mod.rs`:

```rust
// ── VTN protocol adapter (inbound + outbound) ──────────────────────────
mod openadr_interface;   // VTN events → rates, capacity constraints
mod reporter;            // AssetHistoryBuffer + ControllerEventLog → VTN reports

// ── Control logic ──────────────────────────────────────────────────────
mod planner;             // 8-phase greedy scheduler
mod dispatcher;          // plan → setpoints per tick
mod user_request;        // user request lifecycle

// ── Observability + data assembly ──────────────────────────────────────
mod trace;               // ControllerEventLog + AssetHistoryBuffer
mod monitor;             // tick accounting, history writes, packet energy
mod timeline;            // Vec<AssetTimelinePoint> assembly on demand
```

If a second DR protocol is ever added, the extraction path is clear: pull `openadr_interface.rs` and `reporter.rs` into `openadr/`, introduce a trait, write a second implementation. No speculative split needed now.

---

## 21. BDD Test Suite — Scope of Impact (T2)

### Decision
Document the affected test surface before implementation begins to avoid discovering broken assumptions mid-way.

### Categories affected

| Category | Impact | Action |
|---|---|---|
| Reactor behaviour scenarios | **Deleted** — reactor is removed | Remove feature files / scenarios that test FSM states, reactor arbitration, reactor trace |
| `UserOverrides` force-override scenarios | **Deleted** — force fields removed | Remove scenarios that set `ev_force_kw`, `heater_force_kw`, `battery_force_kw` in POST /sim/override |
| `POST /sim/override` generic schema | **Rewritten** — body format changes | Update steps to use new generic `HashMap<String, f64>` body keyed by control schema |
| `GET /trace` scenarios | **Rewritten** — endpoint replaced | Update to `GET /trace/events` and `GET /trace/history?asset=<id>` |
| Asset timeline / diagram data scenarios | **Rewritten** — data source changes | Update to use `GET /timeline/{asset_id}` and `GET /timeline/all` |
| Simulator asset state scenarios | **Rewritten** — `GET /sim` response format changes | Update field paths from named snapshots to `assets.<id>.values.<key>` |
| Controller UC-01–UC-12 scenarios | **Largely preserved** — planner/dispatcher/request flow unchanged | Verify pass after other changes; minor fixture updates expected |
| VTN reporting scenarios | **Rewritten** — dual-mode reporter, `reactor_mode` removed | Update report content assertions; add event-driven status report scenarios |

### Approach
Run the full BDD suite against the unchanged codebase first to establish a baseline. Then, as each implementation phase completes, re-run the suite. Deleted scenarios are removed before the relevant phase begins (not at the end) so failures are never masked.


# How to use it with speckit
  ---
  Why not one speckit

  The document spans three distinct implementation layers with strict dependencies. A single speckit would generate 100+ tasks across  
  Rust backend, API, and React frontend — too large to implement coherently in one go. More critically, speckit.tasks generates a flat 
  task list; it can't represent "don't touch the UI until the backend compiles" as a structural constraint.

  ---
  Recommended split: 3 speckit features, strictly sequential

  Speckit 1 — ven-simulator-reform

  Pure Rust refactor — no API or UI changes, zero behavior change

  Sections: §6, §8, §15, §16, §18

  ┌─────────┬────────────────────────────────────────────────────────────────────────────────────────────────────────┐
  │ Section │                                              What changes                                              │
  ├─────────┼────────────────────────────────────────────────────────────────────────────────────────────────────────┤
  │ §6      │ simulator/actors.rs → simulator/assets/{ev,heater,pv,battery,base_load}.rs; SimState → Vec<AssetEntry> │
  ├─────────┼────────────────────────────────────────────────────────────────────────────────────────────────────────┤
  │ §8      │ AssetHistoryBuffer columnar ring buffer data structure (used in Speckit 2)                             │
  ├─────────┼────────────────────────────────────────────────────────────────────────────────────────────────────────┤
  │ §15     │ AssetCapabilities trait on each asset type                                                             │
  ├─────────┼────────────────────────────────────────────────────────────────────────────────────────────────────────┤
  │ §16     │ default_setpoint() on each asset; AssetEntry owns setpoint + energy counter                            │
  ├─────────┼────────────────────────────────────────────────────────────────────────────────────────────────────────┤
  │ §18     │ ven-N.yaml typed list format; profile.rs: DeviceConfig → Vec<AssetConfig>                              │
  └─────────┴────────────────────────────────────────────────────────────────────────────────────────────────────────┘

  This is safe to implement and test before touching the controller. The existing BDD simulator/profile scenarios validate it compiles 
  and behaves identically.

  ---
  Speckit 2 — ven-controller-reform

  Backend refactor + API changes — no UI changes yet

  Sections: §2, §3, §4, §14, §17, §20, plus BDD cleanup from §21

  ┌─────────┬────────────────────────────────────────────────────────────────────────────┐
  │ Section │                                What changes                                │
  ├─────────┼────────────────────────────────────────────────────────────────────────────┤
  │ §2      │ reactor/ deleted; single control path                                      │
  ├─────────┼────────────────────────────────────────────────────────────────────────────┤
  │ §3      │ UserOverrides force fields removed                                         │
  ├─────────┼────────────────────────────────────────────────────────────────────────────┤
  │ §4      │ controller/trace.rs with ControllerEventLog + AssetHistoryBuffer writes    │
  ├─────────┼────────────────────────────────────────────────────────────────────────────┤
  │ §17     │ Dispatcher simplified; monitor owns packet accounting; tick loop finalised │
  ├─────────┼────────────────────────────────────────────────────────────────────────────┤
  │ §14     │ reporter.rs → controller/reporter.rs; dual-mode report builders            │
  ├─────────┼────────────────────────────────────────────────────────────────────────────┤
  │ §20     │ controller/mod.rs grouped declarations                                     │
  ├─────────┼────────────────────────────────────────────────────────────────────────────┤
  │ §21     │ Delete/rewrite reactor + force-override + GET /trace BDD scenarios         │
  └─────────┴────────────────────────────────────────────────────────────────────────────┘

  Depends on: Speckit 1 (needs AssetEntry, AssetHistoryBuffer, AssetCapabilities)

  ---
  Speckit 3 — ven-timeline-ui

  New API endpoints + full UI rebuild

  Sections: §1, §5, §7, §13, §19, plus UI rows from §12, plus remaining §21 BDD updates

  ┌─────────┬────────────────────────────────────────────────────────────────────────────────────┐
  │ Section │                                    What changes                                    │
  ├─────────┼────────────────────────────────────────────────────────────────────────────────────┤
  │ §5      │ GET /timeline/{asset_id} in controller/timeline.rs                                 │
  ├─────────┼────────────────────────────────────────────────────────────────────────────────────┤
  │ §19     │ GET /timeline/all same module                                                      │
  ├─────────┼────────────────────────────────────────────────────────────────────────────────────┤
  │ §7      │ GET /sim/schema + AssetControlSchema + dynamic UI controls                         │
  ├─────────┼────────────────────────────────────────────────────────────────────────────────────┤
  │ §13     │ Per-cell extended window toggle; useTimeline hook with query params                │
  ├─────────┼────────────────────────────────────────────────────────────────────────────────────┤
  │ §1      │ UI fixes now correct (battery/PV data from timeline, fixed X-axis, memoized nowMs) │
  ├─────────┼────────────────────────────────────────────────────────────────────────────────────┤
  │ §21     │ Rewrite timeline + sim response + UI BDD scenarios                                 │
  └─────────┴────────────────────────────────────────────────────────────────────────────────────┘

  Depends on: Speckit 1 + 2 (needs correct data flowing through new endpoints)

  ---
  Summary

  ┌───────────────────────┬────────────┬─────────────────────┬───────────────┬─────────────────┐
  │        Speckit        │ Rust files │     API changes     │  UI changes   │ Estimated tasks │
  ├───────────────────────┼────────────┼─────────────────────┼───────────────┼─────────────────┤
  │ 1 — simulator-reform  │ ~10        │ None                │ None          │ ~35             │
  ├───────────────────────┼────────────┼─────────────────────┼───────────────┼─────────────────┤
  │ 2 — controller-reform │ ~12        │ GET /trace replaced │ None          │ ~45             │
  ├───────────────────────┼────────────┼─────────────────────┼───────────────┼─────────────────┤
  │ 3 — timeline-ui       │ ~5         │ 3 new endpoints     │ ~8 components │ ~35             │
  └───────────────────────┴────────────┴─────────────────────┴───────────────┴─────────────────┘

  The summaries §10/§11/§12 are reference material — paste the relevant rows into each speckit's speckit.specify description rather    
  than treating them as separate features.

  Verdict: split into 3, feed each speckit only its own sections. The ordering is fixed by the dependencies. Start with Speckit 1 —    
  it's a pure refactor with a clear definition of done (all existing simulator BDD scenarios still pass, zero behavior change).        
