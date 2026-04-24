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

### Step 3 — Tick loop: start WM when plan says so

> **Implementation location: `loops.rs` (`spawn_sim_tick`), NOT `dispatcher.rs`.**
>
> `build_setpoints()` is a **pure function** with no `state` parameter — it cannot call
> `state.shiftable_runtimes().await`. The start/complete logic must live in `spawn_sim_tick`
> which already has both `state: AppState` and `trigger_tx` in scope.

Add this block in `spawn_sim_tick` in `loops.rs`, just AFTER the `build_setpoints()` / `sim_guard.tick()` call and BEFORE `state.update_sim()`:

```rust
// Detect shiftable loads that the current plan slot wants to run and start them.
let known_sim_ids: std::collections::HashSet<&str> =
    sim_guard.assets.iter().map(|a| a.id.as_str()).collect();
if let Some(ref plan) = plan_snap {
    if let Some(slot) = plan.slots.iter().find(|s| s.start <= now && now < s.end) {
        let runtimes = state.shiftable_runtimes().await;
        let loads = state.shiftable_loads().await;
        for alloc in &slot.allocations {
            if known_sim_ids.contains(alloc.asset_id.as_str()) { continue; }
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
}
```

### Step 4 — Tick loop: complete expired WM loads

In the same tick loop block in `loops.rs`, also BEFORE `state.update_sim()`:

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

> **Implementation location: `loops.rs` (`spawn_sim_tick`), NOT `routes/sim.rs`.**
>
> `GET /sim` (routes/sim.rs:192) simply returns the cached `AppState.sim` snapshot —
> it does not build one. Injecting runtimes only in the route handler would mean WM power
> is **excluded from ledger accounting** (`monitor::record_tick` runs on the same snapshot
> in the tick loop). Augment the snapshot in the tick loop AFTER `to_sim_snapshot()` and
> BEFORE `state.update_sim()` and `record_tick()`.

```rust
// In spawn_sim_tick, after: let sim_snap = sim_guard.to_sim_snapshot();
let runtimes = state.shiftable_runtimes().await;
for rt in &runtimes {
    if rt.is_running(now) {
        sim_snap.assets.insert(rt.asset_id.clone(), AssetSnapshot {
            power_kw: rt.power_kw,
            values: {
                let mut m = std::collections::HashMap::new();
                m.insert("running".into(), 1.0);
                m.insert("ends_at_unix".into(), rt.ends_at.timestamp() as f64);
                m
            },
        });
    }
}
// Then: state.update_sim(sim_snap.clone()).await;  ← WM now in stored snapshot AND ledger
```

This makes WM appear in `GET /sim` and in the asset ledger during its run window.

### Step 6 — UI: WM in sim asset list and power chart

In `VEN/ui/src/components/controller/dataBuilders.ts`:
- `deriveAssetSummaries` is **hardcoded** — it has explicit branches for `ev`, `heater`,
  `pv`, `battery`, `base_load` (lines 124–152). WM will NOT appear automatically.
- Either add an explicit WM branch (simplest), or refactor to iterate `sim.assets` dynamically
  for all assets not in the hardcoded set. The latter is more future-proof.
- Add a label map entry: `"wm" → "Washing Machine"` (and update `ASSET_COLORS` if it exists).

In the power chart (Plan A chart, or existing sim chart):
- WM power is in `sim.assets["wm"].power_kw` — add as a chart series (orange).
- In plan slots: WM scheduled power is in `planned_kw_by_asset["wm"]` (from Plan A) or
  `slot.allocations.find(a => a.asset_id === "wm")?.power_kw`.

---

## Files to Modify

| File | Change |
|---|---|
| `VEN/src/entities/device_session.rs` | Add `ShiftableLoadRuntime` struct |
| `VEN/src/state.rs` | Add `shiftable_runtimes` field + accessor methods; change `add_shiftable_load` to return `Result` for duplicate rejection; update `remove_shiftable_load` to also remove matching runtime |
| `VEN/src/loops.rs` | Steps 3+4+5: start/complete runtimes and augment `SimSnapshot` in `spawn_sim_tick` |
| `VEN/src/routes/hems.rs` | Handle 409 from `add_shiftable_load` (duplicate `asset_id`); ensure delete handler removes runtime too |
| `VEN/ui/src/api/types.ts` | No change needed — `AssetSnapshot` is already a generic map |
| `VEN/ui/src/components/controller/dataBuilders.ts` | Add explicit WM branch (not automatic); add WM label and color |

> **Do NOT modify `dispatcher.rs` or `routes/sim.rs` for this feature.** The pure
> `build_setpoints()` function stays pure; `GET /sim` stays a simple cache read.

---

## Edge Cases

- **WM starts mid-slot**: The dispatcher runs every second. The first tick where the plan slot
  includes a WM allocation triggers the start. Acceptable — within 1s of slot boundary.
- **WM load deleted while running**: `remove_shiftable_load()` should also remove any matching
  runtime. Add this logic to the state method.
- **Container restart**: All `#[serde(skip)]` state is lost. WM load and runtime are both lost.
  The MILP will replan — if the WM is still within `earliest_start..latest_end`, it will be
  rescheduled. Acceptable for now (persistence is a separate concern).
- **Multiple WMs**: `asset_id` must be unique per active shiftable load. Change
  `add_shiftable_load` to return `Result<(), &'static str>` (or `bool`) and check for
  duplicate `asset_id` before pushing. Update `post_shiftable_load` in `routes/hems.rs`
  to return HTTP 409 on conflict. Current signature `async fn add_shiftable_load(&self, load: ShiftableLoad)` returns `()` and must change.
- **Delete mid-run (AC#5)**: `remove_shiftable_load` must also remove the matching runtime
  atomically (single write lock). Do NOT require two separate state calls — the delete handler
  would race between them. Simplest: inside `remove_shiftable_load` also call
  `shiftable_runtimes.retain(|r| r.load_id != id)` in the same write lock.

---

## Acceptance Criteria

1. After `POST /shiftable-loads` with `asset_id: "wm"`, the MILP schedules it and the plan shows WM allocations in future slots.
2. When the plan's current slot has a WM allocation, `GET /sim` returns an entry `assets.wm.power_kw == load.power_kw`.
3. After `duration_min` minutes, the WM entry disappears from `GET /sim` and from `GET /shiftable-loads`.
4. A replan is triggered on WM completion.
5. Deleting the shiftable load mid-run also removes the runtime and replans.
6. All existing BDD tests pass.

---

## Tests to Write

### Rust unit tests (fast, no Docker)

Add to `VEN/src/state.rs` (or a `#[cfg(test)]` module there):

| Test | What it verifies |
|---|---|
| `shiftable_runtime_is_running_returns_true_during_window` | `is_running()` true during `[started_at, ends_at)`, false before and after |
| `add_shiftable_load_rejects_duplicate_asset_id` | second add with same `asset_id` returns `Err`; first load still present |
| `start_and_read_shiftable_runtime` | `start_shiftable()` stores runtime; `shiftable_runtimes()` returns it |
| `complete_shiftable_removes_from_both_collections` | `complete_shiftable(load_id)` removes from `shiftable_runtimes` AND `shiftable_loads` |
| `remove_shiftable_load_also_removes_runtime` | `remove_shiftable_load(id)` atomically removes runtime with same `load_id` |

### New BDD step definitions (add to `device_session_steps.py`)

The existing `"I wait for the VEN /plan to have an EV allocation in slots"` is hardcoded.
Add a generic version:

```python
@when('I wait for the VEN /plan to have a "{asset_id}" allocation in slots')
def step_wait_for_asset_allocation(context, asset_id): ...
# poll /plan until any slot.allocations contains asset_id; timeout=90s
```

New polling/assertion steps needed:

```python
@when('I poll VEN /sim until assets contains "{asset_id}"')
# poll_until: GET /sim; assets[asset_id] exists; timeout=60s

@when('I poll VEN /sim until assets does not contain "{asset_id}"')
# poll_until: GET /sim; asset_id NOT in assets; timeout=120s (AC#3 needs 60s+ wait)

@then('the sim asset "{asset_id}" has power_kw equal to {kw:f}')
# assert context.last_response.json()["assets"][asset_id]["power_kw"] == kw

@when('I poll VEN /shiftable-loads until the list is empty')
# poll_until: GET /shiftable-loads returns []; timeout=120s
```

### New BDD scenarios

Add to `tests/features/ven_dispatcher.feature` (or a new `ven_shiftable_runtime.feature`):

```gherkin
Scenario: Shiftable load appears in plan allocations after POST  (AC#1)
  When I POST a shiftable load for asset "wm" at 2.0 kW for 30 minutes within 4 hours
  And I wait for the VEN /plan to have a "wm" allocation in slots
  Then at least one slot has an allocation for asset "wm"

Scenario: Shiftable load appears in GET /sim when current slot is active  (AC#2)
  # earliest_start=now forces slot 0 → dispatcher starts it within one plan+tick cycle
  When I POST a shiftable load for asset "wm" at 2.0 kW for 5 minutes within 1 hour
  And I poll VEN /sim until assets contains "wm"
  Then the sim asset "wm" has power_kw equal to 2.0

Scenario: Shiftable load auto-completes and disappears after duration  (AC#3)
  # Use duration_min=1 (60s). Test takes ~80-90s — set poll timeout=120s.
  When I POST a shiftable load for asset "wm" at 2.0 kW for 1 minutes within 1 hour
  And I poll VEN /sim until assets contains "wm"
  And I poll VEN /sim until assets does not contain "wm"
  Then I poll VEN /shiftable-loads until the list is empty

Scenario: Deleting a running shiftable load removes it from GET /sim immediately  (AC#5)
  When I POST a shiftable load for asset "wm" at 2.0 kW for 5 minutes within 1 hour
  And I poll VEN /sim until assets contains "wm"
  And I DELETE shiftable load with saved id
  Then the response status is 204
  And GET /sim does not contain asset "wm"
  And GET /shiftable-loads returns an empty list

Scenario: POST /shiftable-loads rejects duplicate asset_id with 409
  Given I POST a shiftable load for asset "wm" at 2.0 kW for 60 minutes within 4 hours
  When I POST a shiftable load for asset "wm" at 1.0 kW for 30 minutes within 2 hours
  Then the response status is 409
```

> **Note on AC#4 (replan on completion)**: AC#3 implicitly covers this — if the replan
> did NOT run after completion, the plan would still show WM allocations and the planner
> would try to re-start it. A dedicated test (poll plan.id change) is optional; if added,
> record plan.id before the WM completes and assert it changes within 30s after.

---

## Dependency

Can be implemented independently of Plan A and Plan C.
Plan A's `planned_kw_by_asset` makes WM visible in the future plan chart;
Plan B makes it visible in the live sim chart. They complement each other.
