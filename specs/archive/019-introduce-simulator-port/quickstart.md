# Quickstart: Introduce SimulatorPort trait (Phase 2 — AB-03)

Quick reference for a developer picking up this feature.

## What this phase does

Introduces a `SimulatorPort` trait so that controller logic (dispatcher, absorber, planner, monitor, envelope) and two routes (sim, timeline) no longer import `SimState` directly. After this phase, all seven modules receive `&dyn SimulatorPort` and can be unit-tested with a mock.

## Prerequisites

1. Phase 1 complete (`loops.rs` split into `tasks/`) — or at minimum, `absorber::apply(...)` and `controller::escalate_if_needed(...)` extracted as named function calls in `sim_tick.rs`.
2. `cargo test` baseline is green before starting.

## Key files to touch

| File | Action |
|------|--------|
| `VEN/src/controller/simulator_port.rs` | **Create** — trait, `SnapshotError`, `SimSnapshot`, `GridSnapshot`, `AssetSnapshot`, `SimInjectState` |
| `VEN/src/controller/mod.rs` | **Edit** — `pub mod simulator_port; pub use simulator_port::SimulatorPort;` |
| `VEN/src/simulator/mod.rs` | **Edit** — add `impl SimulatorPort for SimState { ... }` |
| `VEN/src/assets/mod.rs` | **Edit** — move `AssetHistoryBuffer` here from `simulator/mod.rs` |
| `VEN/src/controller/dispatcher.rs` | **Edit** — replace `S_MOD` import with `&dyn SimulatorPort` parameter |
| `VEN/src/controller/absorber.rs` | **Edit** — same |
| `VEN/src/controller/milp_planner.rs` | **Edit** — same |
| `VEN/src/controller/monitor.rs` | **Edit** — same |
| `VEN/src/controller/envelope.rs` | **Edit** — same |
| `VEN/src/routes/sim.rs` | **Edit** — same (temporary; Phase 5 will move to service) |
| `VEN/src/routes/timeline.rs` | **Edit** — same (temporary) |
| `VEN/src/services/test_support/mock_simulator_port.rs` | **Create** — `MockSimulatorPort` |
| `VEN/src/services/test_support/mod.rs` | **Edit** — `pub mod mock_simulator_port;` |

## Trait definition (copy-paste start)

```rust
// VEN/src/controller/simulator_port.rs

use std::collections::HashMap;
use chrono::{DateTime, Utc};

pub trait SimulatorPort: Send + Sync {
    fn snapshot(&self) -> Result<SimSnapshot, SnapshotError>;
    fn inject(&self, state: SimInjectState);
}

#[derive(Debug, Clone)]
pub struct SimSnapshot {
    pub ts: DateTime<Utc>,
    pub grid: GridSnapshot,
    pub assets: HashMap<String, AssetSnapshot>,
}

#[derive(Debug, Clone)]
pub struct GridSnapshot {
    pub net_power_w: f64,
    pub voltage_v: f64,
    pub import_kwh: f64,
    pub export_kwh: f64,
}

#[derive(Debug, Clone)]
pub struct AssetSnapshot {
    pub power_kw: f64,
    pub values: HashMap<String, f64>,
}

#[derive(Debug, Clone)]
pub struct SimInjectState {
    pub ambient_temp_c_override: Option<f64>,
    pub pv_irradiance_override: Option<f64>,
    pub base_load_kw_override: Option<f64>,
    pub ev_plugged_override: Option<bool>,
    pub ev_soc_target_override: Option<f64>,
    pub pv_alpha: f64,
    pub base_load_alpha: f64,
}

#[derive(Debug, thiserror::Error)]
pub enum SnapshotError {
    #[error("simulator not yet initialized")]
    Uninitialized,
    #[error("transient error; retry on next poll")]
    Transient,
    #[error("fatal simulator error")]
    Fatal,
}
```

## Mock skeleton (copy-paste start)

```rust
// VEN/src/services/test_support/mock_simulator_port.rs

use std::sync::Mutex;
use crate::controller::simulator_port::{
    SimulatorPort, SimSnapshot, SimInjectState, SnapshotError,
};

pub struct MockSimulatorPort {
    snapshot: Result<SimSnapshot, SnapshotError>,
    injected: Mutex<Vec<SimInjectState>>,
}

impl MockSimulatorPort {
    pub fn with_snapshot(snapshot: SimSnapshot) -> Self {
        Self { snapshot: Ok(snapshot), injected: Mutex::new(vec![]) }
    }

    pub fn with_error(err: SnapshotError) -> Self {
        Self { snapshot: Err(err), injected: Mutex::new(vec![]) }
    }

    pub fn injected_calls(&self) -> Vec<SimInjectState> {
        self.injected.lock().unwrap().clone()
    }
}

impl SimulatorPort for MockSimulatorPort {
    fn snapshot(&self) -> Result<SimSnapshot, SnapshotError> {
        self.snapshot.clone()
    }

    fn inject(&self, state: SimInjectState) {
        self.injected.lock().unwrap().push(state);
    }
}
```

## Verification checklist

```bash
# 1. Compile clean
cargo build -p ven 2>&1 | grep error

# 2. Unit tests pass
cargo test -p ven 2>&1 | tail -5

# 3. No direct S_MOD imports in listed modules
grep -r "use crate::simulator" VEN/src/controller VEN/src/routes/sim.rs VEN/src/routes/timeline.rs

# 4. Existing integration tests still green (run on Pi4)
docker compose -f tests/docker-compose.test.yml run --build --rm test-runner
```
