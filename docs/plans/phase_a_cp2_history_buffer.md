# Phase A – Checkpoint 2: AssetHistoryBuffer → AssetEntry

## Context

After Checkpoint 1, `AssetEntry` already has a `history: AssetHistoryBuffer` field
(initialized empty). Checkpoint 2 wires history writes to it and removes the
central `asset_history: HashMap<String, AssetHistoryBuffer>` from `ControllerTrace`.

Prerequisite: CP1 compiles and all BDD scenarios pass.

---

## AssetHistoryBuffer — move definition and migrate API

Currently in `controller/trace.rs`. Move to `assets/mod.rs` to match spec §1.5–1.6.

Migrate the internal point type to `HistoryPoint` (spec §1.5) which carries
a full `AssetState` snapshot alongside power:

```rust
/// One recorded tick in a per-asset history buffer.
pub struct HistoryPoint {
    pub ts:       DateTime<Utc>,
    pub power_kw: f64,        // signed: positive = import, negative = export
    pub state:    AssetState, // full state snapshot at this tick
}
```

Replace the current `TimelinePoint { ts, values: HashMap<String, f64> }` API
with the spec's `HistoryPoint` API:

```rust
pub struct AssetHistoryBuffer {
    capacity: usize,                 // default 3600 = 1 h at 1 s tick
    points:   VecDeque<HistoryPoint>,
}

impl AssetHistoryBuffer {
    pub fn new(capacity: usize) -> Self;

    /// Called every tick. Evicts oldest point when full.
    pub fn push(&mut self, point: HistoryPoint) {
        if self.points.len() == self.capacity { self.points.pop_front(); }
        self.points.push_back(point);
    }

    /// All points in [now − window, now], ordered ascending.
    pub fn slice(&self, window: Duration, now: DateTime<Utc>) -> Vec<HistoryPoint>;

    /// Most recent point.
    pub fn latest(&self) -> Option<&HistoryPoint>;

    /// Last-observation-carried-forward power at or before `t`.
    /// Returns None if no point exists at or before `t`.
    pub fn power_at(&self, t: DateTime<Utc>) -> Option<f64>;
}
```

The existing `to_timeline()` method used by `history_from_buffer()` is removed.
Callers that built a `TimeSeries` from history are updated to use
`buffer.slice(timespan, now)` and convert to `TimeSeries` inline (extracting
`(point.ts, point.power_kw)` pairs), or via a new `AssetHistoryBuffer::to_time_series()`
helper if multiple callers share the same conversion.

Update `assets/mod.rs` import from `crate::controller::trace::AssetHistoryBuffer`
to the local definition.

---

## `loops.rs` — write history rows into `sim` lock

Replace `state.push_asset_row(...)` calls with direct writes into `sim.lock()`:

```rust
// In the dispatcher loop after each tick:
let now = Utc::now();
{
    let mut sim = ctx.sim.lock().await;
    for entry in &mut sim.assets {
        entry.history.push(HistoryPoint {
            ts:       now,
            power_kw: entry.last_power_kw,
            state:    entry.state.clone(),
        });
    }
}
```

Remove all `state.push_asset_row(id, power_kw, ts)` calls.

---

## `controller/trace.rs` — remove `asset_history`

Remove from `ControllerTrace`:
- `pub asset_history: HashMap<String, AssetHistoryBuffer>` field
- `pub fn push_asset_row(...)` method
- The `AssetHistoryBuffer` definition itself (moved to `assets/mod.rs`)

`ControllerTrace` retains all other fields (decision log, plan trace, etc.).

---

## `state.rs` — delete `push_asset_row`

Delete `AppState::push_asset_row()`. All call sites now use `sim.lock()` directly
(see `loops.rs` above).

---

## `controller/reporter.rs` — accept `&SimState` instead of `&HashMap`

Change the signature from:
```rust
pub fn build_asset_reports(history: &HashMap<String, AssetHistoryBuffer>, ...) -> ...
```
to:
```rust
pub fn build_asset_reports(sim: &SimState, ...) -> ...
```

Inside the function, access per-asset buffers via:
```rust
for entry in &sim.assets {
    let points = entry.history.slice(window, now);
    // ... build report from points
}
```

---

## `controller/timeline.rs` — accept `&SimState`

Change the signature from:
```rust
pub fn build_asset_timeline(history: &HashMap<String, AssetHistoryBuffer>, ...) -> ...
```
to:
```rust
pub fn build_asset_timeline(sim: &SimState, ...) -> ...
```

Inside: iterate `sim.assets`, access `entry.history.slice(...)` per asset.

---

## `routes/assets.rs` and `routes/trace.rs` — read from `ctx.sim`

Replace any reads from `ctx.state.asset_history(id)` or similar with:
```rust
let sim = ctx.sim.lock().await;
let entry = sim.asset(&id).ok_or(...)?;
let points = entry.history.slice(window, Utc::now());
```

Routes already hold `ctx.sim` (Arc<Mutex<SimState>>), so no new dependencies needed.

---

## Files changed (Checkpoint 2 only)

| File | Change |
|---|---|
| `assets/mod.rs` | add `HistoryPoint` struct; add `AssetHistoryBuffer` definition (moved from `controller/trace.rs`) with new `HistoryPoint`-based API; remove old `to_timeline()` method; update `AssetHistoryBuffer::new(3600)` call in `from_profile()` |
| `controller/trace.rs` | remove `asset_history` field, `push_asset_row()` method, and `AssetHistoryBuffer` definition |
| `state.rs` | delete `push_asset_row()` method |
| `loops.rs` | add `sim.lock()` block to push `HistoryPoint` per asset each tick; remove `state.push_asset_row()` calls |
| `controller/reporter.rs` | accept `&SimState`; read history via `entry.history.slice(...)` |
| `controller/timeline.rs` | accept `&SimState`; read history via `entry.history.slice(...)` |
| `routes/assets.rs` | read history from `ctx.sim.lock()` |
| `routes/trace.rs` | read history from `ctx.sim.lock()` |

---

## Success criteria

- `cargo build` compiles without error
- All existing BDD scenarios pass (`docker compose run --build test-runner`)
- Single commit (combined CP1 + CP2): `refactor(ven): Phase A — config/state split, step(), capability(), per-asset history`
