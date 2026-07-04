# contracts/simulator-port.md

## SimulatorPort trait

```rust
pub trait SimulatorPort: Send + Sync {
    fn snapshot(&self) -> Result<SimSnapshot, SnapshotError>;
    fn inject(&self, state: SimInjectState);
}
```

### `snapshot(&self) -> Result<SimSnapshot, SnapshotError>`

Returns the most recent simulator state snapshot suitable for read-only controller logic.

- Must NOT include historical buffers; history is accessed via `assets/` read routes.
- Callers must not hold the result across ticks; snapshots are point-in-time values.
- Implementations should minimize lock hold time — snapshot and release.

### `inject(&self, state: SimInjectState)`

Best-effort override: mutates simulator parameters for the next tick.

- Return type is `()` — fire-and-forget. Callers cannot recover from a failed inject.
- If the simulator is uninitialized, the inject is silently dropped.
- The simulator self-corrects on the next tick regardless.

---

## Snapshot shape

```rust
pub struct SimSnapshot {
    pub ts: DateTime<Utc>,
    pub grid: GridSnapshot,
    pub assets: HashMap<String, AssetSnapshot>,
}

pub struct GridSnapshot {
    pub net_power_w: f64,
    pub voltage_v: f64,
    pub import_kwh: f64,
    pub export_kwh: f64,
}

pub struct AssetSnapshot {
    pub power_kw: f64,
    pub values: HashMap<String, f64>,  // asset-specific numeric fields (e.g., soc, temp)
}
```

---

## Error semantics (SnapshotError)

```rust
pub enum SnapshotError {
    Uninitialized,  // simulator has not produced a snapshot yet; caller should wait and retry
    Transient,      // temporary condition (e.g., sim locked under long solve); caller may retry
    Fatal,          // unrecoverable state; caller should abort and alert operator
}
```

---

## Inject shape

```rust
pub struct SimInjectState {
    pub ambient_temp_c_override: Option<f64>,
    pub pv_irradiance_override: Option<f64>,
    pub base_load_kw_override: Option<f64>,
    pub ev_plugged_override: Option<bool>,
    pub ev_soc_target_override: Option<f64>,
    pub pv_alpha: f64,
    pub base_load_alpha: f64,
}
```

---

## Concurrency

- Implementations must be `Send + Sync`.
- Interior mutability (`Mutex`/`RwLock`) is allowed but lock hold times must be minimised — take snapshot inside the lock, return the owned value outside.
- The trait is object-safe; callers receive `&dyn SimulatorPort` or `Arc<dyn SimulatorPort>`.

---

## Mocking

`MockSimulatorPort` lives at `VEN/src/services/test_support/mock_simulator_port.rs`.

It must:
- Accept a pre-built `SimSnapshot` at construction time (or a sequence for multi-tick tests).
- Return `Ok(snapshot)` by default; allow configuration of `Err(SnapshotError::Uninitialized)` etc.
- Record all `inject()` calls in an ordered `Vec<SimInjectState>` for assertion.
- Be constructible without running a physics simulation.

