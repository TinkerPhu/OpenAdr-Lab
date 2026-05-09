/// `MockSimulatorPort` — deterministic simulator stub for controller unit tests.
///
/// Usage:
/// ```rust
/// let port = MockSimulatorPort::with_snapshot(make_snapshot());
/// let snap = port.snapshot().expect("mock should succeed");
/// ```
use std::sync::Mutex;

use crate::controller::simulator_port::{
    AssetSnapshot, GridSnapshot, SimInjectState, SimSnapshot, SimulatorPort, SnapshotError,
};

pub struct MockSimulatorPort {
    snapshot: Result<SimSnapshot, SnapshotError>,
    injected: Mutex<Vec<SimInjectState>>,
}

impl MockSimulatorPort {
    /// Pre-load a successful snapshot response.
    pub fn with_snapshot(snapshot: SimSnapshot) -> Self {
        Self {
            snapshot: Ok(snapshot),
            injected: Mutex::new(vec![]),
        }
    }

    /// Pre-load an error response.
    pub fn with_error(err: SnapshotError) -> Self {
        Self {
            snapshot: Err(err),
            injected: Mutex::new(vec![]),
        }
    }

    /// Return all `inject()` calls recorded so far.
    pub fn injected_calls(&self) -> Vec<SimInjectState> {
        self.injected.lock().unwrap().clone()
    }

    /// Build a minimal empty `SimSnapshot` for tests that don't need asset data.
    pub fn empty_snapshot() -> SimSnapshot {
        use chrono::Utc;
        use std::collections::HashMap;
        SimSnapshot {
            ts: Utc::now(),
            grid: GridSnapshot {
                net_power_w: 0.0,
                voltage_v: 230.0,
                import_kwh: 0.0,
                export_kwh: 0.0,
            },
            assets: HashMap::new(),
        }
    }

    /// Build a single-asset `SimSnapshot` for tests needing one asset.
    pub fn snapshot_with_asset(id: &str, snap: AssetSnapshot) -> SimSnapshot {
        use chrono::Utc;
        use std::collections::HashMap;
        let mut assets = HashMap::new();
        assets.insert(id.to_string(), snap);
        SimSnapshot {
            ts: Utc::now(),
            grid: GridSnapshot {
                net_power_w: 0.0,
                voltage_v: 230.0,
                import_kwh: 0.0,
                export_kwh: 0.0,
            },
            assets,
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn with_snapshot_returns_ok() {
        let port = MockSimulatorPort::with_snapshot(MockSimulatorPort::empty_snapshot());
        assert!(port.snapshot().is_ok());
    }

    #[test]
    fn with_error_returns_err() {
        let port = MockSimulatorPort::with_error(SnapshotError::Uninitialized);
        assert!(port.snapshot().is_err());
    }

    #[test]
    fn inject_calls_are_recorded() {
        let port = MockSimulatorPort::with_snapshot(MockSimulatorPort::empty_snapshot());
        port.inject(SimInjectState {
            ambient_temp_c_override: Some(20.0),
            pv_irradiance_override: None,
            base_load_kw_override: None,
            ev_plugged_override: None,
            ev_soc_target_override: None,
            pv_alpha: 0.1,
            base_load_alpha: 0.1,
        });
        let calls = port.injected_calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].ambient_temp_c_override, Some(20.0));
    }

    #[tokio::test]
    async fn concurrent_snapshot_and_inject() {
        let port = Arc::new(MockSimulatorPort::with_snapshot(
            MockSimulatorPort::empty_snapshot(),
        ));
        let n = 4;
        let mut handles = vec![];
        for _ in 0..n {
            let p = port.clone();
            handles.push(tokio::task::spawn(async move {
                p.snapshot().expect("snapshot should not fail");
                p.inject(SimInjectState {
                    ambient_temp_c_override: None,
                    pv_irradiance_override: None,
                    base_load_kw_override: None,
                    ev_plugged_override: None,
                    ev_soc_target_override: None,
                    pv_alpha: 0.1,
                    base_load_alpha: 0.1,
                });
            }));
        }
        for h in handles {
            h.await.expect("task should not panic");
        }
        assert_eq!(port.injected_calls().len(), n);
    }
}
