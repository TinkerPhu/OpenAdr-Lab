# Speckit Call 2 — VEN Controller Reform

**Speckit feature name**: `ven-controller-reform`
**Depends on**: `ven-simulator-reform` (spec_kit_call_1) must be complete and all BDD scenarios passing
**Must be complete before**: spec_kit_call_3 (timeline & UI)

## How to invoke

```
/speckit.specify <paste the Feature Description section below>
```

When prompted for a feature path / name, use: `ven-controller-reform`

---

## Feature Description

Refactor the VEN controller layer: remove the reactor, simplify the dispatcher, reform the trace system, reorganise the controller module, and split VTN reporting into dual-mode (timer-driven + event-driven). This is a **backend + API refactor** — the UI is not changed. All controller BDD scenarios (UC-01–UC-12) must continue to pass; reactor/force-override scenarios are explicitly deleted.

### Prerequisites

Speckit 1 (`ven-simulator-reform`) must be complete. The following types and modules it introduces are used here:
- `Vec<AssetEntry>` with `AssetEntry.setpoint: f64` and `AssetEntry.energy: EnergyCounter`
- `AssetState` enum with `default_setpoint()` and `capabilities()` methods
- `AssetHistoryBuffer` columnar ring buffer in `controller/trace.rs` (data structure only, wiring done here)
- `SimSnapshot` with `assets: HashMap<String, AssetSnapshot>`

### Scope

#### 1. Reactor removal

Delete `VEN/src/reactor/` entirely: `mod.rs`, `fsm.rs`, `arbitration.rs`, `interval.rs`, `trace.rs`.

The reactor read VTN events directly and produced setpoints independently of the controller — a parallel control path that silently conflicted with the planner. The controller's `openadr_interface` already handles the same events. The reactor is redundant and must be removed.

The new single control path:
```
VTN events
    └──► openadr_interface  →  rates + capacity constraints
                                        │
User requests ──────────────────────────┤
                                        ▼
                                   planner  (reactive: triggers on event change, request, completion)
                                        │
                                        ▼
                              dispatcher  →  setpoints  →  simulator
```

FSM states (Idle, Delaying, Ramping, Holding, RampingBack), reactor arbitration logic, and the reactor tick interval are all deleted. Transition smoothing, if ever needed, moves into the dispatcher execution layer.

Remove all references to `reactor` from `main.rs`, `state.rs`, and route handlers.

#### 2. Force-override removal

Remove `UserOverrides` force-override fields: `ev_force_kw`, `heater_force_kw`, `battery_force_kw`, `pv_force_export_limit_kw`.

Rationale: force overrides bypass the planner entirely — the planner produces plans that conflict with what the simulator executes, and can never replan because it was never notified.

Replacement: user intent is expressed through user requests with high-priority value curves. The planner schedules these and the dispatcher executes them. The planner remains the single authority.

`POST /sim/override` body becomes a generic `HashMap<String, f64>` keyed by control schema keys (defined by `AssetState::control_schema()`). Only non-force environmental/device-spec overrides remain (e.g. `ambient_temp_c`, `irradiance_override`). The endpoint stays but its accepted keys change.

#### 3. Trace reform — wire up ControllerEventLog and AssetHistoryBuffer

The `AssetHistoryBuffer` struct was created in speckit 1. Wire it up:

**`controller/trace.rs`** — add `ControllerEventLog`:
```rust
enum ControllerEvent {
    OpenAdrArrived   { ts, event_name, signal_type, value, interval },
    OpenAdrExpired   { ts, event_name },
    RateChange       { ts, interval_start, import_eur_kwh, export_eur_kwh },
    CapacityChange   { ts, import_limit_kw, export_limit_kw },
    PlanCycle        { ts, trigger_reason, firm_slots, flexible_slots },
    PacketTransition { ts, packet_id, asset_id, from_status, to_status },
    RequestTransition{ ts, request_id, asset_id, from_status, to_status },
}

struct ControllerEventLog {
    entries: VecDeque<ControllerEvent>,
    capacity: usize,
}
```

**Writing to the event log**: entries are added from:
- `openadr_interface` on event arrival/expiry and rate/capacity changes
- `planner` on each plan cycle
- `dispatcher` / `user_request` on packet/request status transitions

**Writing to AssetHistoryBuffer**: `controller/monitor.rs` appends one row per asset per tick after `sim.tick()`. Per-asset row contains:
- `power_kw` — actual measured power from `SimSnapshot`
- All `state_values()` keys (e.g. `soc_pct`, `temp_c`)
- `cost_rate_eur_h` = `|power_kw|` × applicable tariff (import or export price)
- `co2_rate_g_h` = `power_kw` × co2_g_kwh
- Grid row additionally: `import_price_eur_kwh`, `export_price_eur_kwh`, `import_limit_kw`, `export_limit_kw`

Replace old `GET /trace` with two new endpoints:
- `GET /trace/events` — returns `Vec<ControllerEvent>` from `ControllerEventLog`
- `GET /trace/history?asset=<id>&limit=N` — returns `Vec<AssetTimelinePoint>` from `AssetHistoryBuffer` for the named asset

#### 4. Dispatcher redesign

Simplify `controller/dispatcher.rs` to a single concern: translate the current plan into per-asset setpoints for this tick.

**Remove** `update_packets()` from dispatcher — packet energy accounting moves to `monitor.rs`.

**Plan lookup — current-tick scan**:
```rust
fn build_setpoints(plan: &Plan, assets: &[AssetEntry], now: DateTime<Utc>) -> HashMap<String, f64> {
    let mut setpoints = HashMap::new();
    // Scan all FIRM and FLEXIBLE slots for the one covering now
    for slot in plan.firm_slots.iter().chain(plan.flexible_slots.iter()) {
        if slot.start <= now && now < slot.end {
            for alloc in &slot.allocations {
                setpoints.insert(alloc.asset_id.clone(), alloc.power_kw);
            }
        }
    }
    // Fill gaps with asset-owned defaults
    for asset in assets {
        setpoints.entry(asset.id.clone()).or_insert_with(|| asset.state.default_setpoint());
    }
    setpoints
}
```

FLEXIBLE slots are treated identically to FIRM — the planner already made this distinction with full context.

**PV export limit** — enforced directly: if an active `ExportCapLimit` capacity constraint is present, dispatcher writes `pv_export_limit_kw` to the PV asset setpoint regardless of plan content. This is a hard compliance constraint, not a planning decision.

**Tick loop sequence in `main.rs`**:
```
1. dispatcher.write_setpoints(plan, assets, capacity_limits, now)
2. sim.tick(dt, env)
3. monitor.record_tick(sim_snapshot, rates, now)
4. reporter.maybe_send_measurement(asset_history, now)   // timer check
```

Event-driven status reports (step 4 variant) are fired from `controller/mod.rs` wherever `ControllerEvent` entries are emitted — not in the tick loop.

#### 5. Monitor — packet energy accounting

`controller/monitor.rs` takes over packet energy accounting from the dispatcher:

```rust
fn record_tick(&mut self, snapshot: &SimSnapshot, rates: &[RateSnapshot], dt_s: f64, now: DateTime<Utc>) {
    for (asset_id, asset_snap) in &snapshot.assets {
        // 1. Write to AssetHistoryBuffer
        self.history.push(asset_id, now, &asset_snap.values);
        // 2. Attribute energy delta to active packet
        let energy_kwh = asset_snap.power_kw.abs() * (dt_s / 3600.0);
        self.update_packet_energy(asset_id, energy_kwh, now);
        // 3. Update AssetLedger
        self.update_ledger(asset_id, asset_snap, rates, now);
    }
}
```

#### 6. VTN reporting — dual-mode

Move `VEN/src/reporter.rs` to `VEN/src/controller/reporter.rs`.

Remove `reactor_mode: &str` parameter from all report builders.

Add two builder functions:

**Timer-driven measurement reports** (replaces existing timer logic):
```rust
fn build_measurement_report(asset_history: &HashMap<String, AssetHistoryBuffer>, now: DateTime<Utc>) -> OpenAdrReport
```
Content: TELEMETRY_USAGE / READING — measured values from `AssetHistoryBuffer` (per-asset power, cumulative energy, SoC where available).

**Event-driven status reports** (new):
```rust
fn build_status_report(event: &ControllerEvent, asset_history: &HashMap<String, AssetHistoryBuffer>, now: DateTime<Utc>) -> OpenAdrReport
```
Content: TELEMETRY_STATUS — confirms the VEN's current response status to the VTN (e.g. "responding to ImportCapLimit event X, currently delivering at Y kW"). Called from `controller/mod.rs` on `PlanCycle` and `PacketTransition` events.

#### 7. Controller module organisation

In `controller/mod.rs`, declare sub-modules in three logical groups:

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
mod timeline;            // (stub — fully implemented in speckit 3)
```

`controller/timeline.rs` is created as a stub (empty module with `pub fn build_asset_timeline(...)` signature) so speckit 3 has a clean insertion point.

#### 8. BDD test suite updates (reactor + force-override scenarios)

Per the pre-agreed scope in the architecture document:

**Delete** (reactor removed):
- All feature files / scenarios testing FSM states (Idle, Delaying, Ramping, Holding, RampingBack)
- Scenarios testing reactor arbitration
- Scenarios testing reactor trace output (`GET /trace` old format)

**Delete** (force overrides removed):
- All scenarios that POST `ev_force_kw`, `heater_force_kw`, `battery_force_kw` to `/sim/override`

**Rewrite**:
- `GET /trace` → `GET /trace/events` and `GET /trace/history?asset=<id>` where applicable
- `POST /sim/override` body shape — update to new generic key format

**Preserve** (verify pass with no change):
- UC-01 through UC-12 controller scenarios — planner/dispatcher/request flow is unchanged

Run the full BDD suite after each major deletion/rewrite phase to catch regressions immediately.

#### 9. API rename — `/rates` → `/tariffs` (backend half)

Rename all Rust identifiers to match the project nomenclature (tariff = X/kWh, rate = X/h):

| Old | New |
|---|---|
| `VEN/src/entities/rate_snapshot.rs` | `VEN/src/entities/tariff_snapshot.rs` |
| `RateSnapshot` struct | `TariffSnapshot` |
| `PlannedRates` | `PlannedTariffs` |
| `PastRates` | `PastTariffs` |
| `GET /rates` route | `GET /tariffs` |
| All Rust callers of the above | Updated to new names |

The UI rename (`useRates` → `useTariffs`, TypeScript types) is done in speckit 3.

### API changes in this speckit

| Endpoint | Change |
|---|---|
| `GET /trace` | **Removed** — replaced by `GET /trace/events` and `GET /trace/history?asset=<id>` |
| `GET /rates` | **Renamed** to `GET /tariffs`; response type renamed `TariffSnapshot` |
| `POST /sim/override` | Body changes: force-override fields removed; remaining keys are generic schema keys |
| `GET /sim` | No change in this speckit (format changed in speckit 1) |

### Acceptance criteria

1. `reactor/` directory does not exist.
2. `UserOverrides` has no force-override fields.
3. `GET /trace/events` returns `ControllerEventLog` entries.
4. `GET /trace/history?asset=ev` returns recent `AssetHistoryBuffer` rows for the EV asset.
5. `monitor.rs` writes to `AssetHistoryBuffer` on every tick and handles packet energy accounting.
6. `reporter.rs` lives in `controller/reporter.rs` with two builder functions; `reactor_mode` parameter gone.
7. All UC-01–UC-12 BDD scenarios pass.
8. Deleted reactor/force-override BDD scenarios are removed from the test suite (not just skipped).
9. `GET /tariffs` returns the same data as the old `GET /rates`; no `RateSnapshot` type name remains in Rust source.

### Files NOT in scope

- `GET /timeline/*` endpoints — speckit 3
- `GET /sim/schema` wiring to UI — speckit 3
- Any UI component — speckit 3
