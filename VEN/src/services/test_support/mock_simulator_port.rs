/// `MockSimulatorPort` — deterministic simulator stub for controller unit tests.
///
/// Usage:
/// ```rust
/// let port = MockSimulatorPort::with_snapshot(make_snapshot());
/// let snap = port.snapshot().expect("mock should succeed");
/// ```
use crate::controller::simulator_port::{
    AssetSnapshot, GridSnapshot, SimSnapshot, SimulatorPort, SnapshotError,
};

pub struct MockSimulatorPort {
    snapshot: Result<SimSnapshot, SnapshotError>,
}

impl MockSimulatorPort {
    /// Pre-load a successful snapshot response.
    pub fn with_snapshot(snapshot: SimSnapshot) -> Self {
        Self {
            snapshot: Ok(snapshot),
        }
    }

    /// Pre-load an error response.
    pub fn with_error(err: SnapshotError) -> Self {
        Self { snapshot: Err(err) }
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
                import_limit_kw: f64::MAX,
                export_limit_kw: -f64::MAX,
            },
            assets: HashMap::new(),
        }
    }

    /// Build a single-asset `SimSnapshot` for tests needing one asset.
    #[allow(dead_code)]
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
                import_limit_kw: f64::MAX,
                export_limit_kw: -f64::MAX,
            },
            assets,
        }
    }
}

impl SimulatorPort for MockSimulatorPort {
    fn snapshot(&self) -> Result<SimSnapshot, SnapshotError> {
        self.snapshot.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
