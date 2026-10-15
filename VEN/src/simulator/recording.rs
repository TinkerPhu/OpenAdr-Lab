//! What the sim records and what is injected into it from outside a tick: the per-asset history
//! points, the grid virtual asset's update, and the base-load asset's observed window. Each
//! was done by hand in a task (`tasks/sim_tick/finalize.rs`, `tasks/base_load_window.rs`),
//! reaching into `entry.history` / downcasting to `BaseLoad`; the sim owns its roster, so the
//! sim does it and the tasks only say when.

use chrono::{DateTime, Utc};

use super::SimState;
use crate::assets::{HistoryPoint, LoadWindowStats};

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
        self.grid_asset.update(
            self.grid.net_power_kw,
            import_limit_kw,
            export_limit_kw_signed,
            now,
        );
    }

    /// The assets that keep a trailing-window summary (`Asset::learns_observed_window`), in
    /// roster order.
    pub fn observed_window_asset_ids(&self) -> Vec<String> {
        self.iter_assets()
            .filter(|(_, cfg)| cfg.learns_observed_window())
            .map(|(entry, _)| entry.id.clone())
            .collect()
    }

    /// Hand `asset_id` the summary of its trailing window of recorded power (`None` while the
    /// history holds no record). A no-op for an asset not in the roster.
    pub fn set_observed_window(&mut self, asset_id: &str, window: Option<LoadWindowStats>) {
        if let Some((_, asset)) = self.find_asset_mut(asset_id) {
            asset.set_observed_window(window);
        }
    }
}
