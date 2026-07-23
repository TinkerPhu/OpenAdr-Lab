use chrono::{DateTime, Duration, Utc};

use super::{
    Asset, AssetCapability, AssetFlexibilityFloor, AssetHistoryBuffer, AssetState, GridState,
    HistoryPoint,
};

/// Grid virtual asset.
///
/// Not controllable — represents the site boundary. Derives its state from
/// the sum of all other asset powers (written each tick by the simulator) and
/// active VTN capacity limits.
///
/// Implements the full `Asset` trait so the Grid participates in trait-object
/// queries (timelines, flexibility envelope). It is NOT added to `AssetConfig`
/// because it has no physics configuration and is not part of the controllable
/// asset dispatch loop.
///
/// ## `simulate_forward` limitation
/// Per spec §10.2, `simulate_forward()` for Grid should compute net = Σ other assets.
/// The `Asset::simulate_forward()` signature takes only `(initial_state, setpoints)` —
/// it has no access to the other assets. The inherited default impl (setpoint passthrough)
/// is used. Full multi-asset net simulation would require a `SiteSimulator` abstraction.
#[derive(Debug, Clone)]
pub struct Grid {
    /// Current live state: net power + VTN capacity limits.
    pub state: GridState,
    /// Per-tick history ring buffer. Capacity = 3600 entries ≈ 1 h at 1 s tick rate.
    pub history: AssetHistoryBuffer,
}

impl Grid {
    /// Construct a new Grid with default state (no limits, zero power).
    ///
    /// `import_limit_kw` defaults to `f64::MAX` (unlimited import).
    /// `export_limit_kw` defaults to `-f64::MAX` (unlimited export).
    pub fn new() -> Self {
        Self {
            state: GridState {
                net_power_kw: 0.0,
                import_limit_kw: f64::MAX,
                export_limit_kw: -f64::MAX,
            },
            history: AssetHistoryBuffer::new(3600),
        }
    }

    /// Update state from the current simulator tick. Called by `loops.rs` after each tick.
    ///
    /// `export_limit_kw_signed` must follow the sign convention: ≤ 0.
    /// Derive from `OadrCapacityState.export_limit_kw` (positive magnitude) by negating:
    /// `export_limit_kw_signed = -capacity.export_limit_kw.unwrap_or(f64::MAX)`
    pub fn update(
        &mut self,
        net_power_kw: f64,
        import_limit_kw: f64,
        export_limit_kw_signed: f64,
        ts: DateTime<Utc>,
    ) {
        self.state = GridState {
            net_power_kw,
            import_limit_kw,
            export_limit_kw: export_limit_kw_signed,
        };
        self.history.push(HistoryPoint {
            ts,
            power_kw: net_power_kw,
            state: AssetState::Grid(self.state.clone()),
        });
    }
}

impl Default for Grid {
    fn default() -> Self {
        Self::new()
    }
}

impl Asset for Grid {
    fn id(&self) -> &str {
        "grid"
    }

    fn current_state(&self) -> AssetState {
        AssetState::Grid(self.state.clone())
    }

    fn history(&self, window: Duration) -> Vec<HistoryPoint> {
        self.history.slice(window, Utc::now())
    }

    /// Grid capability reflects active VTN capacity limits.
    /// Returns `[export_limit_kw, import_limit_kw]` directly from state (already signed).
    fn capability(&self, state: &AssetState) -> AssetCapability {
        match state {
            AssetState::Grid(g) => AssetCapability {
                max_export_kw: g.export_limit_kw, // ≤ 0
                max_import_kw: g.import_limit_kw, // ≥ 0
            },
            _ => AssetCapability {
                max_export_kw: 0.0,
                max_import_kw: 0.0,
            },
        }
    }

    /// Grid is not a controllable device — it's the site boundary, driven by
    /// externally-imposed VTN capacity limits, not something the VEN dispatches.
    /// Floor equals ceiling, same reasoning as PV/base_load. Not rendered in
    /// the Flexibility & Forecast panel (Grid isn't in `AssetConfig`), but the
    /// `Asset` trait requires an explicit answer regardless.
    fn flexibility_floor(&self, state: &AssetState) -> AssetFlexibilityFloor {
        match state {
            AssetState::Grid(g) => AssetFlexibilityFloor {
                min_export_kw: g.export_limit_kw,
                min_import_kw: g.import_limit_kw,
            },
            _ => AssetFlexibilityFloor {
                min_export_kw: 0.0,
                min_import_kw: 0.0,
            },
        }
    }

    /// Grid is not actuated — passthrough. Setpoint is ignored; returns current net power.
    fn step(&self, state: &AssetState, _setpoint_kw: f64, _dt: Duration) -> (AssetState, f64) {
        let power_kw = state.actual_power_kw();
        (state.clone(), power_kw)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn make_grid_state(net_kw: f64, import_kw: f64, export_kw_signed: f64) -> AssetState {
        AssetState::Grid(GridState {
            net_power_kw: net_kw,
            import_limit_kw: import_kw,
            export_limit_kw: export_kw_signed,
        })
    }

    #[test]
    fn new_has_correct_defaults() {
        let g = Grid::new();
        assert_eq!(g.id(), "grid");
        assert_eq!(g.state.net_power_kw, 0.0);
        assert_eq!(g.state.import_limit_kw, f64::MAX);
        assert_eq!(g.state.export_limit_kw, -f64::MAX);
    }

    #[test]
    fn id_returns_grid() {
        let g = Grid::new();
        assert_eq!(g.id(), "grid");
    }

    #[test]
    fn current_state_reflects_live_state() {
        let mut g = Grid::new();
        g.state.net_power_kw = 3.5;
        g.state.import_limit_kw = 8.0;
        g.state.export_limit_kw = -3.0;
        match g.current_state() {
            AssetState::Grid(s) => {
                assert!((s.net_power_kw - 3.5).abs() < 1e-9);
                assert!((s.import_limit_kw - 8.0).abs() < 1e-9);
                assert!((s.export_limit_kw + 3.0).abs() < 1e-9);
            }
            _ => panic!("expected Grid state"),
        }
    }

    #[test]
    fn capability_returns_limit_values() {
        let g = Grid::new();
        let state = make_grid_state(0.0, 10.0, -4.0);
        let cap = g.capability(&state);
        assert!((cap.max_import_kw - 10.0).abs() < 1e-9);
        assert!((cap.max_export_kw + 4.0).abs() < 1e-9); // -4.0
    }

    #[test]
    fn capability_non_grid_state_returns_zero() {
        let g = Grid::new();
        let state = AssetState::Battery(super::super::BatteryState {
            soc: 0.5,
            actual_power_kw: 0.0,
        });
        let cap = g.capability(&state);
        assert_eq!(cap.max_import_kw, 0.0);
        assert_eq!(cap.max_export_kw, 0.0);
    }

    #[test]
    fn step_passthrough_returns_current_net_power() {
        let g = Grid::new();
        let state = make_grid_state(5.0, 10.0, -3.0);
        let (new_state, power_kw) = g.step(&state, 99.0, Duration::seconds(60));
        assert!((power_kw - 5.0).abs() < 1e-9);
        // state is unchanged
        match new_state {
            AssetState::Grid(s) => assert!((s.net_power_kw - 5.0).abs() < 1e-9),
            _ => panic!("expected Grid state"),
        }
    }

    #[test]
    fn update_sets_state_and_pushes_history() {
        let mut g = Grid::new();
        let now = Utc::now();
        g.update(4.2, 8.0, -2.0, now);
        assert!((g.state.net_power_kw - 4.2).abs() < 1e-9);
        assert!((g.state.import_limit_kw - 8.0).abs() < 1e-9);
        assert!((g.state.export_limit_kw + 2.0).abs() < 1e-9);
        let hist = g.history.slice(Duration::seconds(60), now);
        assert_eq!(hist.len(), 1);
        assert!((hist[0].power_kw - 4.2).abs() < 1e-9);
    }

    #[test]
    fn history_returns_points_in_window() {
        let mut g = Grid::new();
        let now = Utc::now();
        g.update(1.0, f64::MAX, -f64::MAX, now - Duration::seconds(30));
        g.update(2.0, f64::MAX, -f64::MAX, now);
        let hist = g.history(Duration::seconds(60));
        assert_eq!(hist.len(), 2);
        // Most recent push should be the last in ascending order
        assert!((hist[1].power_kw - 2.0).abs() < 1e-9);
    }
}
