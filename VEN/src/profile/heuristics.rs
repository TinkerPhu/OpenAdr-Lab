//! Profile-configurable knobs for the base-load heuristics learner
//! (`services::heuristics::learn_asset_heuristics`) — split out of
//! `schema.rs` to keep that file under the file-size cap, same pattern as
//! `grid.rs`/`polling.rs`. Flat, top-level section (not per-asset): the
//! learner operates on one fixed asset today (`base_load`), and even if
//! more heuristic-eligible assets are added later these knobs tune the
//! *algorithm*, not a specific asset's physical parameters.

use serde::Deserialize;

#[derive(Debug, Clone, Copy, Deserialize)]
pub struct HeuristicsConfig {
    #[serde(default = "super::defaults::default_heuristics_rolling_window_days")]
    pub rolling_window_days: u32,
    #[serde(default = "super::defaults::default_heuristics_ewma_halflife_days")]
    pub ewma_halflife_days: f64,
    #[serde(default = "super::defaults::default_heuristics_shrinkage_k_days")]
    pub shrinkage_k_days: f64,
    #[serde(default = "super::defaults::default_heuristics_min_samples_for_confidence")]
    pub min_samples_for_confidence: usize,
}

impl HeuristicsConfig {
    /// Convert to the runtime type consumed by `learn_asset_heuristics` —
    /// mirrors `WeatherPvConfig::to_params()`/`AssetProfile::to_params()`.
    pub fn to_config(self) -> crate::services::heuristics::HeuristicsConfig {
        crate::services::heuristics::HeuristicsConfig {
            rolling_window_days: self.rolling_window_days,
            ewma_halflife_days: self.ewma_halflife_days,
            shrinkage_k_days: self.shrinkage_k_days,
            min_samples_for_confidence: self.min_samples_for_confidence,
        }
    }
}
