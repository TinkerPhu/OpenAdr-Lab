# VEN API Overview

All HTTP endpoints are defined in `VEN/src/main.rs`. Routes are registered via Axum's `Router::new().route()` chain (lines 443–469). All handlers are async functions in the same file. CORS is enabled for all methods from any origin. Every handler receives `State(ctx)` providing access to `AppState`, `VtnClient`, metrics, the plan-trigger channel, and the loaded device profile.

---

## Endpoints

### Infrastructure

| Method | Path | Handler line | Description |
|--------|------|-------------|-------------|
| GET | `/health` | ~498 | Returns `"ok"` |
| GET | `/metrics` | ~502 | Prometheus metrics in text format |

---

### OpenADR Proxy

Forwards queries to the VTN via the internal `VtnClient`.

| Method | Path | Handler line | Description |
|--------|------|-------------|-------------|
| GET | `/events` | ~511 | Active OpenADR events from VTN; optional `limit` query param |
| GET | `/programs` | ~523 | Available programs from VTN |

---

### Sensors

Manual sensor snapshot — used by UI and tests to inject readings.

| Method | Path | Handler line | Description |
|--------|------|-------------|-------------|
| GET | `/sensors` | ~527 | Returns current sensor snapshot (temperature, power, voltage) |
| POST | `/sensors` | ~531 | Create/update sensor snapshot |

---

### Reports

VTN report submission — the VEN creates reports on behalf of itself.

| Method | Path | Handler line | Description |
|--------|------|-------------|-------------|
| GET | `/reports` | ~547 | Reports already submitted to VTN by this VEN |
| POST | `/reports` | ~551 | Submit a new report to VTN (proxied) |
| PUT | `/reports/:id` | ~571 | Update an existing report at VTN (proxied) |

---

### Simulator

Physics-based device simulation (PV, EV, battery, heater).
Business logic lives in `VEN/src/simulator/` and `VEN/src/reactor/`.

| Method | Path | Handler line | Description |
|--------|------|-------------|-------------|
| GET | `/sim` | ~592 | Full simulator state — device states, power flows, energy counters |
| GET | `/sim/override` | ~603 | Current user override settings |
| POST | `/sim/override` | ~607 | **Full-replace** override (EV charge limit, battery force kW, heater target, PV curtailment, initial SOC) |
| GET | `/trace` | ~620 | Reactor decision trace log (newest first); optional `limit` query param |

---

### HEMS Controller (Stages 1–5)

Home Energy Management System — tariff-aware planner and dispatcher.
Business logic lives in `VEN/src/controller/` and `VEN/src/entities/`.

| Method | Path | Handler line | Stage | Description |
|--------|------|-------------|-------|-------------|
| GET | `/rates` | ~648 | 2 | Rate/tariff snapshots parsed from active events |
| GET | `/capacity` | ~653 | 2 | `OadrCapacityState` parsed from active events |
| GET | `/obligations` | ~658 | 2 | Pending report obligations extracted from events |
| GET | `/packets` | ~635 | 3 | All EnergyPackets (FIRM + FLEXIBLE + terminal) |
| GET | `/plan` | ~640 | 3 | Active Plan or `null` |
| POST | `/packets` | ~673 | 4 | Create EnergyPacket and trigger reactive replanning |
| GET | `/ledger` | ~735 | 4 | Per-asset cumulative energy / cost / CO₂ ledger |
| GET | `/user-requests` | ~742 | 5 | All active user energy task requests |
| POST | `/user-requests` | ~747 | 5 | Create user request with multi-tier deadline + budget constraints |
| DELETE | `/user-requests/:id` | ~782 | 5 | Cancel request → marks associated packet ABANDONED |
| GET | `/flexibility` | ~802 | 5 | `FlexibilityEnvelopes` derived from active plan |

---

## Recorded History of Simulated Values

### Which endpoints carry historical/recorded data?

Four endpoints expose data derived from the simulation's past. They differ fundamentally in granularity, scope, and storage model.

---

#### 1. `GET /trace` — Reactor decision log (primary time series)
**Source**: `VEN/src/reactor/trace.rs` — `DecisionTrace` ring buffer
**Storage**: in-memory `VecDeque`, capped at **1 000 entries**, not persisted across restarts
**Cadence**: one entry per reactor tick (1 s interval)
**Default response**: last 50 entries, newest first; use `?limit=N` for more

Each entry (`TraceEntry`) contains:

| Field | Type | Description |
|-------|------|-------------|
| `ts` | `DateTime<Utc>` | Timestamp of the reactor tick |
| `mode` | `String` | Reactor arbitration mode (`IMPORT_CAP`, `PRICE`, `IDLE`, …) |
| `fsm_state` | `String` | FSM state (`Idle`, `Ramping`, `Holding`, …) |
| `active_events` | `Vec<String>` | Event IDs active at this tick |
| `winning_intent` | `Option<String>` | Event ID that won arbitration, if any |
| `setpoints.ev_charge_kw` | `f32` (2 dp) | Commanded EV charge power (kW) |
| `setpoints.heater_kw` | `f32` (2 dp) | Commanded heater power (kW) |
| `setpoints.pv_export_limit_kw` | `Option<f32>` | Active PV export cap (kW); `null` = no limit |
| `setpoints.mode` | `String` | Setpoint mode label |
| `constraints` | `Vec<String>` | Hard constraints active this tick |
| `reason` | `String` | Human-readable explanation for the decision |

**What this records**: the control decisions (commanded setpoints), not measured power. At 1 s cadence the commanded and actual values are effectively identical for these modelled devices, so the trace is the closest thing to a raw power time series in the system. Maximum history window at 1 s cadence: ~16.7 minutes.

---

#### 2. `GET /packets` — Per-packet actual power profile (HEMS task history)
**Source**: `VEN/src/entities/energy_packet.rs` — `EnergyPacket.past_power_profile`
**Storage**: in-memory, survives as long as the packet is in state (including terminal packets)
**Cadence**: one `EnergySnapshot` per dispatcher tick while packet is `ACTIVE`
**Scope**: only covers HEMS-managed energy tasks (EV charging sessions, battery dispatch jobs)

Each `EnergyPacket` (returned by `GET /packets`) embeds:

| Field | Type | Description |
|-------|------|-------------|
| `past_power_profile[].ts` | `DateTime<Utc>` | Measurement timestamp |
| `past_power_profile[].power_kw` | `f64` | Instantaneous power at this step |
| `past_power_profile[].cumulative_energy_kwh` | `f64` | Cumulative energy delivered since packet start |
| `accumulated_cost_eur` | `f64` | Running cost tally (Σ power × tariff × dt) |
| `accumulated_co2_g` | `f64` | Running CO₂ tally |
| `planned_power_profile[]` | `Vec<EnergySnapshot>` | Planner's forward schedule (future, not past) |

**What this records**: actual execution history for a specific energy task. Persists until the VEN restarts (not written to disk). Terminal packets (`COMPLETED`, `ABANDONED`, etc.) remain in the response so their history is accessible after the task ends.

---

#### 3. `GET /ledger` — Per-asset cumulative accumulators (billing period totals)
**Source**: `VEN/src/entities/asset.rs` — `AssetLedger`
**Storage**: in-memory, updated every dispatcher tick (1 s)
**Cadence**: aggregate only — no time series, just running totals since VEN startup

Each `AssetLedger` entry contains:

| Field | Type | Description |
|-------|------|-------------|
| `asset_id` | `String` | Which device |
| `period_start` | `Option<DateTime<Utc>>` | Start of the accounting period |
| `total_consumption_kwh` | `f64` | All energy consumed this period |
| `total_production_kwh` | `f64` | All energy produced this period |
| `total_import_cost_eur` | `f64` | Cost of imported energy attributed to this asset |
| `total_export_revenue_eur` | `f64` | Revenue from exported energy |
| `total_co2_g` | `f64` | CO₂ attributed to this asset |
| `tracked_by_packets_kwh` | `f64` | Energy covered by EnergyPackets |
| `untracked_energy_kwh` | `f64` | Standby / uncontrolled consumption |

**What this records**: billing-period totals per asset. No timestamps within the period. The underlying energy integrator is the same `EnergyCounter` that drives `/sim`'s `import_kwh`/`export_kwh`.

---

#### 4. `GET /reports` — VTN-stored event reports (discrete sim snapshots)
**Source**: `VEN/src/reporter.rs` — `build_report()` constructs from `SimState` at submission time
**Storage**: stored at the VTN (proxied via `VtnClient`), not held locally
**Cadence**: event-driven — one report per active event per reactor reporting cycle

Each report submitted to (and returned from) the VTN contains:

| Payload type | Value source | When used |
|---|---|---|
| `USAGE` → `import_w` | `SimState.import_w` | For `IMPORT_CAPACITY_LIMIT` events |
| `USAGE` → `export_w` | `SimState.export_w` | For `EXPORT_CAPACITY_LIMIT` events |
| `USAGE` → `net_power_w` | `SimState.net_power_w` | For `PRICE` events |
| `SIMPLE` → `1.0` | constant | For `SIMPLE` events |
| `OPERATING_STATE` | reactor mode string | Always included |
| `STORAGE_CHARGE_LEVEL` | `ev.soc × 100` | When EV is present |

**What this records**: a coarse, event-triggered snapshot of sim power values sent to the VTN as compliance evidence. Not a continuous log.

---

### Overlap and duplication analysis

| Concern | Assessment |
|---------|-----------|
| `/trace` vs `/sim` current values | No duplication: `/trace` is historical (commanded setpoints), `/sim` is the live snapshot (actual device state). |
| `/trace` setpoints vs `/reports` power values | Partial overlap in data (both derive from sim state at the same moments), but different purpose and destination. `/trace` is local diagnostic history; `/reports` are outbound compliance messages to the VTN. |
| `/ledger` vs `/sim` `import_kwh`/`export_kwh` | The same underlying `EnergyCounter` drives both. `/sim` exposes site-level totals; `/ledger` breaks them down per asset with cost/CO₂ attribution. No functional duplication, different granularity. |
| `/packets` `past_power_profile` vs `/ledger` | Complementary: `/packets` gives a time series per task; `/ledger` gives aggregate totals per asset across all tasks and untracked consumption. |
| Gap: no raw sim power time series | **The system has no endpoint that logs a continuous time series of measured/simulated power values (e.g. a `/sim/history`).** `/trace` is the best substitute but records commanded setpoints, not measured watts, and is capped at ~1 000 ticks (~16 min at 1 s cadence). |

---

### Summary: recorded-history data by endpoint

| Endpoint | Type | Granularity | Persisted? | Max history |
|----------|------|-------------|------------|-------------|
| `GET /trace` | Control decisions (setpoints) | 1 s | No | 1 000 ticks ≈ 16 min |
| `GET /packets` `past_power_profile` | Actual power per HEMS task | 1 s (while active) | No | Lifetime of packet |
| `GET /ledger` | Cumulative totals per asset | None (aggregate) | No | Since startup |
| `GET /reports` | Discrete sim snapshots at VTN | Event-driven | At VTN | Indefinite (VTN) |

---

## Module Map

| Module | Path | Role |
|--------|------|------|
| Routes + handlers | `VEN/src/main.rs` | All route registrations and handler functions |
| Simulator actors | `VEN/src/simulator/` | PV, EV, battery, heater physics models |
| Reactor | `VEN/src/reactor/` | FSM, arbitration, interval loop, decision trace |
| HEMS controller | `VEN/src/controller/` | Dispatcher, monitor, planner, OpenADR interface, user-request manager |
| Entities | `VEN/src/entities/` | EnergyPacket, Plan, RateSnapshot, CapacityState, AssetLedger, UserRequest |
| Reporter | `VEN/src/reporter.rs` | Report-building logic (no HTTP endpoints) |
