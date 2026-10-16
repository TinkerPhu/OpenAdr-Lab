use chrono::{DateTime, Utc};

use super::{AssetHistoryBuffer, AssetState, GridState, HistoryPoint};

/// The site meter.
///
/// Not controllable — represents the site boundary. Derives its state from
/// the sum of all other asset powers (written each tick by the simulator) and
/// active VTN capacity limits.
///
/// The site meter, not an asset: it implements no `Asset` trait and declares no `AssetKind`.
/// The simulator holds one (`SimState::grid_asset`) and reads its state and history directly.
///
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

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration, Utc};

    #[test]
    fn new_has_correct_defaults() {
        let g = Grid::new();
        assert_eq!(g.state.net_power_kw, 0.0);
        assert_eq!(g.state.import_limit_kw, f64::MAX);
        assert_eq!(g.state.export_limit_kw, -f64::MAX);
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
        let hist = g.history.slice(Duration::seconds(60), now);
        assert_eq!(hist.len(), 2);
        // Most recent push should be the last in ascending order
        assert!((hist[1].power_kw - 2.0).abs() < 1e-9);
    }
}
