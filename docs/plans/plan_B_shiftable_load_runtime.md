# Plan B — Shiftable Load Runtime (WM Dispatcher Sim)

## Goal

Give shiftable loads (washing machine, dishwasher, etc.) a live runtime state so they:
1. Appear in `GET /sim` with real-time power draw during their scheduled window.
2. Appear as a named series in the power chart (not silently merged into `net_import_kw`).
3. Automatically complete and clean themselves up when their run finishes.
4. Trigger a replan when they complete (frees up the scheduled slot).

---

## Background and Problem

`ShiftableLoad` (entities/device_session.rs:45) describes a scheduling request.
The MILP planner picks an optimal start slot and puts `AssetAllocation` entries for the WM in
`PlanTimeSlot.allocations`. The dispatcher reads these allocations and calls
`setpoints.insert(asset_id, power_kw)` for each.

But nothing acts on that setpoint — there is no sim asset for WM, so:
- `GET /sim` does not include WM power.
- The live power chart shows no WM series.
- WM energy consumption is invisible (it's implicitly in `net_import_kw` but not broken out).
- WM never completes — the `ShiftableLoad` stays in `active_requests` forever.
- There is no feedback loop: the MILP keeps replanning WM in the same slot even after it started.

WM does **not** need a physics simulation (no thermal model, no SoC). It needs only a
**state machine**: `Idle → Running (countdown) → Done`.

---

## Key Code References

### Backend — entities
- `ShiftableLoad` struct: `VEN/src/entities/device_session.rs:45–59`
  - `id: Uuid`, `asset_id: String`, `power_kw: f64`, `duration_min: u32`
  - `earliest_start: DateTime<Utc>`, `latest_end: DateTime<Utc>`
  - `created_at`, `updated_at`
- `EvSession` (reference, same file:11): has `departure_time` as its completion condition.

### Backend — state
- `AppState` inner struct: `VEN/src/state.rs:87–127`
  - `shiftable_loads: Vec<ShiftableLoad>` at line ~124, `#[serde(skip)]`
  - All session fields are `#[serde(skip)]` — none are persisted.
  - Accessor: `shiftable_loads()`, `add_shiftable_load()`, `remove_shiftable_load()`

### Backend — dispatcher
- `build_setpoints()`: `VEN/src/controller/dispatcher.rs:23–97`
  - Finds current plan slot: `plan.slots.iter().find(|s| s.start <= now && now < s.end)` (~line 42)
  - Applies allocations as setpoints: iterates `slot.allocations`, calls `setpoints.insert(alloc.asset_id.clone(), alloc.power_kw)` (~line 60)
  - WM would receive this setpoint call but nothing consumes it.

### Backend — simulator
- `SimState` struct: `VEN/src/simulator/mod.rs:78–96`
  - `assets: Vec<AssetEntry>` — physics assets only (EV, heater, PV, battery, base_load)
- `AssetState` enum: `VEN/src/assets/mod.rs:97–105`
  - variants: `Battery`, `Ev`, `Heater`, `Pv`, `BaseLoad`, `Grid`
- `AssetEntry`: `VEN/src/simulator/mod.rs:48–63`
  - `id`, `state: AssetState`, `setpoint_kw`, `last_power_kw`, `energy: EnergyCounter`, `history`
- `GET /sim` returns `SimSnapshot` with `assets: HashMap<String, AssetSnapshot>`
  - `AssetSnapshot.power_kw` is what the UI charts.

### Backend — loops / dispatch cycle
- Main dispatch loop tick: look for where `build_setpoints()` is called and where `sim.tick()` runs.
  Check `VEN/src/loops.rs` and `VEN/src/controller/dispatcher.rs` for the 1-second tick structure.

---

## Design: ShiftableLoadRuntime

Add a separate in-memory runtime tracker alongside `shiftable_loads` in `AppState`.
Do NOT add WM to the physics `SimState` — it has no physics, and the sim architecture is
asset-config-driven (loaded from profile YAML). WM loads are dynamic user requests.

### New struct (entities/device_session.rs or new file entities/shiftable_runtime.rs)

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShiftableLoadRuntime {
    pub load_id: Uuid,           // FK → ShiftableLoad.id
    pub asset_id: String,        // e.g. "wm"
    pub power_kw: f64,
    pub started_at: DateTime<Utc>,
    pub ends_at: DateTime<Utc>,  // started_at + duration_min
}

impl ShiftableLoadRuntime {
    pub fn is_running(&self, now: DateTime<Utc>) -> bool {
        now >= self.started_at && now < self.ends_at
    }
}
```

### AppState changes (state.rs)

Add alongside `shiftable_loads`:

```rust
#[serde(skip)]
shiftable_runtimes: Vec<ShiftableLoadRuntime>,
```

Add accessor methods:
- `shiftable_runtimes() -> Vec<ShiftableLoadRuntime>` (read)
- `start_shiftable(load_id, power_kw, asset_id, started_at, ends_at)`
- `complete_shiftable(load_id) -> Option<ShiftableLoad>` (removes runtime + removes load from shiftable_loads)

---

## Step-by-Step Implementation

### Step 1 — Add `ShiftableLoadRuntime` struct

Add to `VEN/src/entities/device_session.rs` (after `ShiftableLoad`):

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShiftableLoadRuntime {
    pub load_id: Uuid,
    pub asset_id: String,
    pub power_kw: f64,
    pub started_at: DateTime<Utc>,
    pub ends_at: DateTime<Utc>,
}
```

### Step 2 — Add to AppState (state.rs)

In the inner state struct, add:
```rust
#[serde(skip)]
shiftable_runtimes: Vec<ShiftableLoadRuntime>,
```

Add methods:
```rust
pub async fn start_shiftable(&self, runtime: ShiftableLoadRuntime) { ... }
pub async fn shiftable_runtimes(&self) -> Vec<ShiftableLoadRuntime> { ... }
pub async fn complete_shiftable(&self, load_id: Uuid) { ... }
// complete_shiftable: removes from shiftable_runtimes AND shiftable_loads
```

### Step 3 — Dispatcher: start WM when plan says so

In `build_setpoints()` (`dispatcher.rs`), after the existing allocation loop, for each
allocation whose `asset_id` is NOT a known sim asset (i.e., not in sim's asset list):

```rust
// In the allocation loop, after setpoints.insert():
let known_sim_ids: HashSet<&str> = sim_assets.iter().map(|a| a.id.as_str()).collect();
for alloc in &slot.allocations {
    if !known_sim_ids.contains(alloc.asset_id.as_str()) {
        // Shiftable load — start it if not already running
        let runtimes = state.shiftable_runtimes().await;
        let loads = state.shiftable_loads().await;
        let already_running = runtimes.iter().any(|r| r.asset_id == alloc.asset_id);
        if !already_running {
            if let Some(load) = loads.iter().find(|l| l.asset_id == alloc.asset_id) {
                let ends_at = now + chrono::Duration::minutes(load.duration_min as i64);
                state.start_shiftable(ShiftableLoadRuntime {
                    load_id: load.id,
                    asset_id: load.asset_id.clone(),
                    power_kw: load.power_kw,
                    started_at: now,
                    ends_at,
                }).await;
                tracing::info!(asset_id = %load.asset_id, ends_at = %ends_at, "shiftable load started");
            }
        }
    }
}
```

### Step 4 — Dispatcher tick: complete expired WM loads

In the same dispatcher loop (or the 1s tick loop in loops.rs), after starting:

```rust
let runtimes = state.shiftable_runtimes().await;
for rt in &runtimes {
    if now >= rt.ends_at {
        tracing::info!(asset_id = %rt.asset_id, "shiftable load completed");
        state.complete_shiftable(rt.load_id).await;
        let _ = trigger_tx.send(PlanTrigger::UserRequest); // replan: slot is now free
    }
}
```

`complete_shiftable()` in state.rs removes from both `shiftable_runtimes` and `shiftable_loads`.

### Step 5 — Surface in GET /sim response

The sim's `SimSnapshot` is built from `SimState.assets`. WM is not a sim asset.
Instead, inject active shiftable runtimes into the snapshot in the route handler
(`routes/sim.rs` or wherever `GET /sim` is handled):

```rust
// After building the normal SimSnapshot:
let runtimes = ctx.state.shiftable_runtimes().await;
for rt in &runtimes {
    if rt.is_running(Utc::now()) {
        snapshot.assets.insert(rt.asset_id.clone(), AssetSnapshot {
            power_kw: rt.power_kw,
            values: {
                let mut m = HashMap::new();
                m.insert("running".into(), 1.0);
                m.insert("ends_at_unix".into(), rt.ends_at.timestamp() as f64);
                m
            },
        });
    }
}
```

This makes WM appear in `GET /sim` with `power_kw` during its run window.

### Step 6 — UI: WM in sim asset list and power chart

In `VEN/ui/src/components/controller/dataBuilders.ts`:
- The sim snapshot now includes WM asset entries dynamically.
- The existing code that builds asset cards iterates `sim.assets` — WM will appear automatically.
- Add a display label for WM (currently asset IDs are shown raw; add a map `"wm" → "Washing Machine"`).

In the power chart (Plan A chart, or existing sim chart):
- WM power is in `sim.assets["wm"].power_kw` — add as a chart series (orange).
- In plan slots: WM scheduled power is in `planned_kw_by_asset["wm"]` (from Plan A) or
  `slot.allocations.find(a => a.asset_id === "wm")?.power_kw`.

---

## Files to Modify

| File | Change |
|---|---|
| `VEN/src/entities/device_session.rs` | Add `ShiftableLoadRuntime` struct |
| `VEN/src/state.rs` | Add `shiftable_runtimes` field + 3 accessor methods |
| `VEN/src/controller/dispatcher.rs` | Start WM on allocation, complete expired loads |
| `VEN/src/routes/sim.rs` | Inject active runtimes into `SimSnapshot` |
| `VEN/ui/src/api/types.ts` | No change needed — `AssetSnapshot` is already a generic map |
| `VEN/ui/src/components/controller/dataBuilders.ts` | Add WM label; already handles dynamic assets |

---

## Edge Cases

- **WM starts mid-slot**: The dispatcher runs every second. The first tick where the plan slot
  includes a WM allocation triggers the start. Acceptable — within 1s of slot boundary.
- **WM load deleted while running**: `remove_shiftable_load()` should also remove any matching
  runtime. Add this logic to the state method.
- **Container restart**: All `#[serde(skip)]` state is lost. WM load and runtime are both lost.
  The MILP will replan — if the WM is still within `earliest_start..latest_end`, it will be
  rescheduled. Acceptable for now (persistence is a separate concern).
- **Multiple WMs**: `asset_id` must be unique per active shiftable load — enforce this in
  `add_shiftable_load()` (reject if same `asset_id` already active).

---

## Acceptance Criteria

1. After `POST /shiftable-loads` with `asset_id: "wm"`, the MILP schedules it and the plan shows WM allocations in future slots.
2. When the plan's current slot has a WM allocation, `GET /sim` returns an entry `assets.wm.power_kw == load.power_kw`.
3. After `duration_min` minutes, the WM entry disappears from `GET /sim` and from `GET /shiftable-loads`.
4. A replan is triggered on WM completion.
5. Deleting the shiftable load mid-run also removes the runtime and replans.
6. All existing BDD tests pass.

---

## Dependency

Can be implemented independently of Plan A and Plan C.
Plan A's `planned_kw_by_asset` makes WM visible in the future plan chart;
Plan B makes it visible in the live sim chart. They complement each other.
