//! WP5.2 (BL-14) accessors — split out of `mod.rs` to keep it under the
//! file-size cap; behaves as an ordinary `impl AppState` block.
use std::collections::HashMap;

use crate::entities::design_vocabulary::AssetHeuristics;

use super::AppState;

impl AppState {
    pub async fn asset_heuristics(&self) -> HashMap<String, AssetHeuristics> {
        self.hems.read().await.asset_heuristics.clone()
    }

    pub async fn set_asset_heuristics(&self, heuristics: HashMap<String, AssetHeuristics>) {
        self.hems.write().await.asset_heuristics = heuristics;
    }
}
