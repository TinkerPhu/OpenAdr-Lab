# Research: VEN Controller Reform (004)

## Decision 1: ControllerEventLog placement

**Decision**: Add `ControllerEventLog` to `controller/trace.rs` alongside the existing `AssetHistoryBuffer`.

**Rationale**: `trace.rs` already holds the `AssetHistoryBuffer` data structure stub created in speckit 1. Placing `ControllerEventLog` in the same file keeps all ring-buffer observability types co-located. The module is already named for this purpose.

**Alternatives considered**:
- Separate `event_log.rs` module — rejected; two small types in one file is simpler than a new module.

---

## Decision 2: AssetHistoryBuffer storage location in AppState

**Decision**: Add a new `controller_trace: ControllerTrace` field to `InnerState`, where `ControllerTrace` holds both `ControllerEventLog` and `HashMap<String, AssetHistoryBuffer>`.

**Rationale**: Both buffers need to be shared between the tick loop (writer) and HTTP handlers (reader). Using the existing `AppState` `Arc<RwLock<InnerState>>` pattern is consistent with all other state fields. Grouping them in a single `ControllerTrace` struct avoids cluttering `InnerState` with two extra fields and makes the relationship explicit.

**Alternatives considered**:
- Separate Arc<RwLock<>> for history buffers — rejected; adds concurrency complexity with no benefit.
- Store history in the `Mutex<SimState>` — rejected; sim and controller are separate concerns.

---

## Decision 3: Reporter relocation and split

**Decision**: Move `VEN/src/reporter.rs` to `VEN/src/controller/reporter.rs`. Split into `build_measurement_report` (timer-driven) and `build_status_report` (event-driven). Remove `reactor_mode` parameter and the dependency on `reactor::interval::find_active_intervals`.

**Rationale**: After reactor removal, `reporter.rs` no longer has any dependency on the reactor module. The `reactor_mode` string was only meaningful relative to the FSM state; with the FSM gone it becomes meaningless. Moving it into the `controller` module makes the dependency graph match the logical architecture.

**Implementation note**: `build_measurement_report` reads `AssetHistoryBuffer` snapshots. `build_status_report` reads the most recent history row per asset to describe current state in the VTN-facing report.

---

## Decision 4: `update_packets` relocation

**Decision**: Remove `update_packets` from `dispatcher.rs` and implement equivalent logic in `monitor.record_tick`. The monitor runs *after* `sim.tick()` and has access to the actual `SimSnapshot` with measured power — giving it authoritative values for energy attribution.

**Rationale**: The dispatcher currently runs *before* `sim.tick()` to set setpoints, and then `update_packets` runs after. Since monitor already runs post-tick, combining packet energy accounting into `monitor.record_tick` eliminates the need to call `dispatcher.update_packets` separately, simplifies the tick sequence, and ensures energy is attributed from measured (not commanded) values.

**Alternatives considered**:
- Keep `update_packets` in dispatcher but call it after sim.tick — still requires two dispatcher calls per tick; no benefit over moving to monitor.

---

## Decision 5: `POST /sim/override` body format after force-override removal

**Decision**: Change the body from a typed `UserOverrides` struct to `HashMap<String, f64>`, serialized as a JSON object with string keys and float values. Valid keys are the union of: environmental inputs (`pv_irradiance`, `ambient_temp_c`) and device preferences (`ev_desired_kw`, `ev_plugged` as 0/1, `ev_max_charge_kw`, `ev_soc_target`, `heater_max_kw`, `heater_temp_min_c`, `heater_temp_max_c`, `pv_rated_kw`, `base_load_w`). Unknown keys are silently ignored.

**Rationale**: The architecture document specifies `HashMap<String, f64>` keyed by control schema keys. This makes the endpoint schema-driven and forward-compatible with new asset types. The `GET /sim/schema` endpoint already exposes the available control schema keys per asset.

**Note**: `ev_plugged` (bool) is represented as `0.0` (false) or `1.0` (true) in the float map. The handler converts it.

---

## Decision 6: Tick loop sequence after refactor

**Decision**: New tick loop sequence in `main.rs`:
```
1. dispatcher.build_setpoints(plan, assets, capacity_limits, now)  → HashMap<String, f64>
2. sim.tick(dt_s, setpoints, now, &overrides)
3. monitor.record_tick(sim_snapshot, rates, packets, dt_s, now)    → Option<PlanTrigger>
4. reporter.maybe_send_measurement(history, now, vtn)              → timer check
```
Event-driven status reports are fired from the planning loop / controller event log on `PlanCycle` and `PacketTransition` events, not from the tick loop.

**Rationale**: Matches the architecture document exactly. The old reactor evaluation step is dropped entirely. The dispatcher is reduced to a pure setpoint builder with no side-effects.

---

## Decision 7: State field migration

**Decision**: Replace the existing `state.rs` fields:
- `trace: Vec<TraceEntry>` → replaced by `controller_trace: ControllerTrace`
- `planned_rates: Vec<RateSnapshot>` → `planned_tariffs: Vec<TariffSnapshot>`

And add new AppState accessor methods: `controller_trace()`, `set_controller_trace()`, `planned_tariffs()`, `set_planned_tariffs()`.

**Rationale**: The old `trace` field held reactor `TraceEntry` structs — which come from the reactor module being deleted. After reactor removal there is no longer a source for these entries. The `ControllerEventLog` replaces this role. `RateSnapshot` is renamed to `TariffSnapshot` per FR-015.

**Breaking change to `/trace` endpoint**: The old `GET /trace` returned `Vec<TraceEntry>` (reactor decision log). It is replaced by `GET /trace/events` and `GET /trace/history`.

---

## Decision 8: BDD scenario file cleanup strategy

**Decision**: Delete the following feature files / scenario blocks:
- All scenarios in any `.feature` file referencing FSM states (`Idle`, `Delaying`, `Ramping`, `Holding`, `RampingBack`), reactor arbitration, or `GET /trace` (old format).
- All scenarios POSTing `ev_force_kw`, `heater_force_kw`, `battery_force_kw` to `/sim/override`.
- Rewrite `GET /trace` → `GET /trace/events` or `GET /trace/history?asset=<id>` in any surviving scenario.
- Rewrite `GET /rates` → `GET /tariffs` in all feature files.

**Discovery needed**: Scan `tests/features/` for: `force_kw`, `GET /trace`, `GET /rates`, FSM state names. Files to check: `ven_dispatcher.feature`, `ven_rate_system.feature`, any UC feature referencing trace.

---

## Decision 9: `ControllerTrace` struct in `AppState`

**Decision**: Store `ControllerTrace` as a non-serialized (`#[serde(skip)]`) field in `InnerState`, since it holds in-memory ring buffers that do not need to survive restarts.

**Rationale**: Consistent with how `active_packets`, `active_plan`, and `asset_ledger` are already marked `#[serde(skip)]`.
