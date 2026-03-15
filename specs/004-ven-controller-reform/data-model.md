# Data Model: VEN Controller Reform (004)

## New Types

### `ControllerEvent` (enum) — `controller/trace.rs`

Tagged union representing a significant controller decision or state change.

```
ControllerEvent::OpenAdrArrived {
    ts: DateTime<Utc>,
    event_name: String,
    signal_type: String,     // "IMPORT_CAPACITY_LIMIT", "PRICE", etc.
    value: f64,
    interval: u32,
}

ControllerEvent::OpenAdrExpired {
    ts: DateTime<Utc>,
    event_name: String,
}

ControllerEvent::RateChange {
    ts: DateTime<Utc>,
    interval_start: DateTime<Utc>,
    import_eur_kwh: f64,
    export_eur_kwh: f64,
}

ControllerEvent::CapacityChange {
    ts: DateTime<Utc>,
    import_limit_kw: Option<f64>,
    export_limit_kw: Option<f64>,
}

ControllerEvent::PlanCycle {
    ts: DateTime<Utc>,
    trigger_reason: String,   // from PlanTrigger enum
    firm_slots: usize,
    flexible_slots: usize,
}

ControllerEvent::PacketTransition {
    ts: DateTime<Utc>,
    packet_id: Uuid,
    asset_id: String,
    from_status: String,
    to_status: String,
}

ControllerEvent::RequestTransition {
    ts: DateTime<Utc>,
    request_id: Uuid,
    asset_id: String,
    from_status: String,
    to_status: String,
}
```

**Serialized as**: Tagged JSON enum (`serde(tag = "type")`). The `ts` field is ISO 8601.

---

### `ControllerEventLog` (struct) — `controller/trace.rs`

Ring buffer of `ControllerEvent` entries.

```
ControllerEventLog {
    entries: VecDeque<ControllerEvent>,
    capacity: usize,           // default: 500
}
```

**Methods**: `push(event)`, `entries() -> Vec<ControllerEvent>`, `len() -> usize`.

---

### `ControllerTrace` (struct) — `controller/trace.rs`

Combined holder for both observability buffers. Stored in `InnerState` as a single field.

```
ControllerTrace {
    event_log: ControllerEventLog,
    asset_history: HashMap<String, AssetHistoryBuffer>,
}
```

**Methods**: `push_event(event)`, `push_asset_row(asset_id, ts, values)`, `events() -> Vec<ControllerEvent>`, `asset_history(asset_id) -> Option<&AssetHistoryBuffer>`.

---

## Modified Types

### `TariffSnapshot` — `entities/tariff_snapshot.rs` (renamed from `rate_snapshot.rs`)

No structural change; only renamed:
- `RateSnapshot` → `TariffSnapshot`
- `PlannedRates` → `PlannedTariffs`
- `PastRates` → `PastTariffs`
- `RateHeuristic` → `TariffHeuristic`

All fields remain identical.

---

### `UserOverrides` — `state.rs`

Remove force-override fields:
```
// REMOVED:
pub ev_force_kw: Option<f64>,
pub heater_force_kw: Option<f64>,
pub battery_force_kw: Option<f64>,
pub pv_force_export_limit_kw: Option<f64>,
```

Remaining fields (unchanged):
```
pub pv_irradiance: Option<f64>,
pub ambient_temp_c: Option<f64>,
pub ev_desired_kw: Option<f64>,
pub ev_plugged: Option<bool>,
pub ev_max_charge_kw: Option<f64>,
pub ev_soc_target: Option<f64>,
pub heater_max_kw: Option<f64>,
pub heater_temp_min_c: Option<f64>,
pub heater_temp_max_c: Option<f64>,
pub pv_rated_kw: Option<f64>,
pub base_load_w: Option<f64>,
```

The `POST /sim/override` endpoint accepts a `HashMap<String, f64>` body; the handler maps keys to `UserOverrides` fields. Unknown keys are ignored. `ev_plugged` maps from `0.0`/`1.0`.

---

### `InnerState` — `state.rs`

Changes:
- Remove: `trace: Vec<TraceEntry>` (reactor trace — gone with reactor)
- Add: `controller_trace: ControllerTrace` (`#[serde(skip)]`)
- Rename: `planned_rates: Vec<RateSnapshot>` → `planned_tariffs: Vec<TariffSnapshot>`

New AppState accessor methods:
- `controller_trace() -> ControllerTrace`
- `set_controller_trace(t: ControllerTrace)`
- `push_controller_event(event: ControllerEvent)` — acquires write lock, pushes to log
- `push_asset_history_row(asset_id, ts, values)` — acquires write lock, pushes row
- `planned_tariffs() -> Vec<TariffSnapshot>`
- `set_planned_tariffs(Vec<TariffSnapshot>)`

Remove: `update_sim(sim, trace)` method (reactor trace parameter gone) → replace with `update_sim(sim: SimSnapshot)`.

---

### `DispatcherSetpoints` — `controller/dispatcher.rs`

**Remove** the old `DispatcherSetpoints` struct (named fields). Replace with:
```
fn build_setpoints(
    plan: &Plan,
    assets: &[AssetEntry],
    capacity: &OadrCapacityState,
    now: DateTime<Utc>,
) -> HashMap<String, f64>
```

The function:
1. Scans FIRM and FLEXIBLE slots for the slot covering `now`.
2. Inserts `alloc.power_kw` for each `alloc.asset_id` in the matching slot.
3. Fills gaps via `asset.state.default_setpoint()` for every asset not covered by a slot.
4. If an active `ExportCapLimit` is present in `capacity`, overrides the `pv` key with `capacity.export_limit_kw`.

Returns `HashMap<String, f64>` — fed directly to `sim.tick()`.

Remove function: `get_setpoints` (old FIRM-only version).
Remove function: `update_packets` (moved to monitor).

---

### `monitor.rs` — `controller/monitor.rs`

Add new function signature:
```
fn record_tick(
    trace: &mut ControllerTrace,
    ledger: &mut HashMap<String, AssetLedgerEntry>,
    packets: &mut Vec<EnergyPacket>,
    snapshot: &SimSnapshot,
    tariffs: &[TariffSnapshot],
    dt_s: f64,
    now: DateTime<Utc>,
) -> Option<PlanTrigger>
```

The function:
1. Writes one row per asset to `AssetHistoryBuffer` via `trace.push_asset_row(...)`:
   - Columns: `power_kw`, all `state_values()` keys from `AssetSnapshot`, `cost_rate_eur_h`, `co2_rate_g_h`.
   - Grid asset additionally: `import_price_eur_kwh`, `export_price_eur_kwh`, `import_limit_kw`, `export_limit_kw`.
2. Attributes energy: `|power_kw| * (dt_s / 3600.0)` to the `EnergyPacket` whose `asset_id` matches and whose status is Active. Handles `Scheduled → Active` transition when energy starts flowing.
3. Checks deadline/completion on each packet; transitions to `Completed` or `PartialCompleted` as needed. Returns a `PlanTrigger::DeviceDeviation` if any packet completed.
4. Updates `AssetLedgerEntry` in ledger (existing ledger logic moved here from the old `update_ledger` function).

Remove function: `update_ledger` (folded into `record_tick`).

---

### `reporter.rs` — moved to `controller/reporter.rs`

Two public functions:

```
pub fn build_measurement_report(
    asset_history: &HashMap<String, AssetHistoryBuffer>,
    ven_name: &str,
    now: DateTime<Utc>,
) -> Option<OpenAdrReport>
```
Content: TELEMETRY_USAGE / READING — per-asset power, cumulative energy, SoC where available. Built from last N rows of each `AssetHistoryBuffer`.

```
pub fn build_status_report(
    event: &ControllerEvent,
    asset_history: &HashMap<String, AssetHistoryBuffer>,
    ven_name: &str,
    now: DateTime<Utc>,
) -> Option<OpenAdrReport>
```
Content: TELEMETRY_STATUS — VEN's current response status to a specific plan/packet transition.

**Remove**: `reactor_mode: &str` parameter from all functions. **Remove**: `find_active_intervals` import from `reactor::interval`.

---

### `controller/mod.rs`

Replace flat module list with three grouped sections:
```rust
// ── VTN protocol adapter ──────────────────────────────────────────────
pub mod openadr_interface;
pub mod reporter;

// ── Control logic ─────────────────────────────────────────────────────
pub mod planner;
pub mod dispatcher;
pub mod user_request;

// ── Observability ─────────────────────────────────────────────────────
pub mod trace;
pub mod monitor;
pub mod timeline;
```

---

### `controller/timeline.rs` (new stub)

```rust
use crate::controller::trace::AssetHistoryBuffer;
use std::collections::HashMap;

/// Stub — fully implemented in speckit 3.
pub fn build_asset_timeline(
    _history: &HashMap<String, AssetHistoryBuffer>,
) -> serde_json::Value {
    serde_json::Value::Null
}
```

---

## Deleted Types

| Type | Location | Reason |
|------|----------|--------|
| `Reactor` | `reactor/mod.rs` | Entire reactor module deleted |
| `ReactorFsm` | `reactor/fsm.rs` | Part of deleted reactor |
| `ControlIntent` | `reactor/arbitration.rs` | Part of deleted reactor |
| `ReactorMode` | `reactor/arbitration.rs` | Part of deleted reactor |
| `DecisionTrace` | `reactor/trace.rs` | Part of deleted reactor |
| `TraceEntry` | `reactor/trace.rs` | Part of deleted reactor |
| `Setpoints` | `reactor/mod.rs` | Replaced by `HashMap<String, f64>` from dispatcher |
| `DispatcherSetpoints` | `controller/dispatcher.rs` | Replaced by `HashMap<String, f64>` |
| `RateSnapshot` | `entities/rate_snapshot.rs` | Renamed to `TariffSnapshot` |
| `PlannedRates` | `entities/rate_snapshot.rs` | Renamed to `PlannedTariffs` |
| `PastRates` | `entities/rate_snapshot.rs` | Renamed to `PastTariffs` |

---

## File-Level Changes Summary

| File | Action |
|------|--------|
| `VEN/src/reactor/` (all 5 files) | **Delete** |
| `VEN/src/reporter.rs` | **Move** to `VEN/src/controller/reporter.rs` |
| `VEN/src/entities/rate_snapshot.rs` | **Rename** to `tariff_snapshot.rs`; rename types inside |
| `VEN/src/entities/mod.rs` | Update `rate_snapshot` → `tariff_snapshot` |
| `VEN/src/controller/trace.rs` | **Add** `ControllerEvent`, `ControllerEventLog`, `ControllerTrace` |
| `VEN/src/controller/dispatcher.rs` | **Rewrite** `get_setpoints` → `build_setpoints`; remove `update_packets` |
| `VEN/src/controller/monitor.rs` | **Expand** `update_ledger` → `record_tick` with history + packet accounting |
| `VEN/src/controller/reporter.rs` | **New** (moved + refactored from `reporter.rs`) |
| `VEN/src/controller/timeline.rs` | **New** (stub) |
| `VEN/src/controller/mod.rs` | **Update** module list with grouped comments |
| `VEN/src/state.rs` | Remove `trace` field; add `controller_trace`; rename `planned_rates` |
| `VEN/src/main.rs` | **Major refactor**: remove reactor init, rewrite tick loop, update routes |
