# Plan: Option A — Snapshot-and-Release Sim Mutex

## Problem

`spawn_sim_tick` in `VEN/src/loops.rs` holds `Arc<Mutex<SimState>>` while doing multiple async
`.await` operations in every tick cycle (~1 second). This causes all HTTP handlers
(`GET /forecast`, `GET /history`, `GET /capability`, etc.) to block until the tick completes,
producing 10 s `ReadTimeout` failures in BDD tests (RC1) and Playwright timeouts (RC3).

Three lock sites hold across awaits:
1. **Phase 1–5 main block**: `apply_sim_injections` cleared_fields awaits + entire `publish_sim_tick_result().await` (sensor update, shiftable logic, ledger, envelope — all async state calls)
2. **Phase 8 persist**: `persist::save(&sim_guard, &data_dir).await` — file I/O while holding the lock
3. (Phase 7 is already correct — drops guard before the await loop)

## Approach

**Snapshot-and-release**: do all sim-mutating work synchronously inside the lock block, extract
all needed data into owned values, drop the lock, then do all async state publishes outside the lock.

No new types needed — use existing `SensorSnapshot`, `SimSnapshot`, `SiteEnvelope` return values.

## Files Changed

- `VEN/src/loops.rs` — the only file; two targeted changes:
  - `spawn_sim_tick` lock block restructure
  - `publish_sim_tick_result` signature change (drop `&mut SimState` parameter)

## Tasks

### T1 — Restructure the main lock block in `spawn_sim_tick`

**Current** (inside `let sim_snapshot = { let mut sim_guard = sim.lock().await; ... }` block):
- Phase 1: `apply_sim_injections` → collect `cleared_fields`
- Phase 1 await: `state.clear_inject_field(field).await` × N  ← **WRONG — inside lock**
- Phase 2: `build_tick_setpoints` (sync)
- Phase 3: `apply_deviation_correction` (sync)
- Phase 4: `sim_guard.tick(...)` (sync)
- One-shot clears: `state.clear_inject_field("pv_irradiance").await` ← **WRONG**
- Phase 5: `publish_sim_tick_result(&mut *sim_guard, ...).await` ← **WRONG**

**New lock block** (pure sync, no awaits):
```rust
let (sensor, sim_snap, envelope, cleared_fields, pv_clear, base_clear) = {
    let mut sim_guard = sim.lock().await;
    // Phase 1: collect fields to clear (sync only)
    let cleared_fields = apply_sim_injections(&inject, &mut *sim_guard);
    // Phase 2: setpoints (sync)
    let sp_map = build_tick_setpoints(...);
    // Phase 3: deviation correction (sync)
    apply_deviation_correction(...);
    // Phase 4: tick (sync)
    sim_guard.tick(...);
    let pv_clear = inject.pv_irradiance.is_some();
    let base_clear = inject.base_load_kw.is_some();
    // Extract data while guard is held
    let sensor = sim_guard.to_sensor_snapshot();
    let sim_snap = sim_guard.to_sim_snapshot();
    // History push (mutates sim_guard — must be in-lock)
    for entry in &mut sim_guard.assets {
        entry.history.push(HistoryPoint { ts: now, power_kw: entry.last_power_kw, state: entry.state.clone() });
    }
    // Grid asset update (mutates sim_guard — must be in-lock)
    sim_guard.grid_asset.update(net_power_kw, import_limit_kw, export_limit_kw_signed, now);
    // Envelope compute (reads sim_guard, returns owned value)
    let envelope = controller::envelope::compute_envelope(&*sim_guard, now);
    (sensor, sim_snap, envelope, cleared_fields, pv_clear, base_clear)
    // ← sim_guard DROPPED HERE
};

// Post-lock: inject field clears (now safe async)
for field in cleared_fields { state.clear_inject_field(field).await; }
if pv_clear { state.clear_inject_field("pv_irradiance").await; }
if base_clear { state.clear_inject_field("base_load_kw").await; }

// Post-lock Phase 5: all async state updates
let sim_snapshot = publish_sim_tick_result(sensor, sim_snap, envelope, plan_snap.as_ref(), ...).await;
```

### T2 — Refactor `publish_sim_tick_result` signature

**Remove**: `sim_guard: &mut SimState` parameter (and all `sim_guard` usage inside)  
**Add**: `sensor: SensorSnapshot`, `mut sim_snap: SimSnapshot`, `envelope: SiteEnvelope`  
**Remove**: the in-function history push, grid asset update, and envelope compute blocks (moved to T1)  
**Keep**: all the async state calls exactly as they are  
**Change**: `known_sim_ids` derivation → use `sim_snap.assets.contains_key(...)` instead of `sim_guard.assets.iter()`

### T3 — Fix Phase 8 persist

**Current** (inside loop, after Phase 7):
```rust
let sim_guard = sim.lock().await;
if let Err(e) = crate::simulator::persist::save(&sim_guard, &data_dir).await { ... }
```

**New**:
```rust
let sim_clone = { sim.lock().await.clone() };
if let Err(e) = crate::simulator::persist::save(&sim_clone, &data_dir).await { ... }
```

### T4 — cargo build

Verify no compile errors. Run on Pi4 via Docker build (Windows can't build `highs-sys`).

### T5 — BDD regression on Pi4

Push, rebuild VEN Docker image, run full BDD suite. Expect RC1/RC3 failures to disappear.

## Expected outcome

- Lock hold time: ~1 ms (pure CPU math) vs current ~1 s (full tick cycle with async)
- HTTP handler block probability: ~0.1% vs current ~50%
- BDD failures: 0 (down from 16)
