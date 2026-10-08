//! What the sim records and what is injected into it from outside a tick: the per-asset history
//! points, the grid virtual asset's update, and the base-load asset's observed window. Each
//! was done by hand in a task (`tasks/sim_tick/finalize.rs`, `tasks/base_load_window.rs`),
//! reaching into `entry.history` / downcasting to `BaseLoad`; the sim owns its roster, so the
//! sim does it and the tasks only say when.

use chrono::{DateTime, Utc};

use super::{GridMeter, SimState};
use crate::assets::{HistoryPoint, LoadWindowStats};
use crate::ids::ASSET_BASE_LOAD;

impl GridMeter {
    /// Net site power in kW (positive = import). The meter stores watts; this is the one
    /// conversion for the sim's own tail (the other readers are tracked in R-111).
    pub fn net_power_kw(&self) -> f64 {
        self.net_power_w / 1000.0
    }
}

impl SimState {
    /// Record this tick: one `HistoryPoint` per asset into its ring buffer, and the grid
    /// virtual asset updated with the meter's net power and the VTN capacity limits.
    /// `export_limit_kw_signed` follows the grid asset's sign convention (<= 0). Done after
    /// `tick()` rather than inside it because the capacity limits come from the caller.
    pub fn record_history(
        &mut self,
        now: DateTime<Utc>,
        import_limit_kw: f64,
        export_limit_kw_signed: f64,
    ) {
        for entry in &mut self.assets {
            entry.history.push(HistoryPoint {
                ts: now,
                power_kw: entry.last_power_kw,
                state: entry.state.clone(),
            });
        }
        let net_power_kw = self.grid.net_power_kw();
        self.grid_asset
            .update(net_power_kw, import_limit_kw, export_limit_kw_signed, now);
    }

    /// Hand the base-load asset the summary of its trailing window of recorded power (`None`
    /// while the history holds no record). A no-op when the roster has no base load.
    pub fn set_base_load_observed_window(&mut self, window: Option<LoadWindowStats>) {
        if let Some((_, asset)) = self.find_asset_mut(ASSET_BASE_LOAD) {
            asset.set_observed_window(window);
        }
    }
}
