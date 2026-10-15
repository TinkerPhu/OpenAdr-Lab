//! `SimState::peek_base_load_kw` — split into its own file to keep
//! `simulator/mod.rs` under the file-size cap; behaves as an ordinary
//! `impl SimState` method. Mirrors `pv_preview.rs`'s structure exactly.

use chrono::{DateTime, Utc};

use crate::assets::BaseLoad;

use super::SimState;

impl SimState {
    /// Preview this tick's base-load output *before* `tick()` mutates state
    /// (same natural-profile + Behaviour B offset-decay formula as `tick()` in
    /// `mod.rs`; read-only), for the base load `asset_id`. `None` if it is not a base load.
    ///
    /// Closes the base-load half of the one-tick lag `peek_pv_kw` already
    /// closes for PV — the deviation arbiter needs both uncontrollable
    /// inputs to be this tick's value, not last tick's `AssetSnapshot.power_kw`
    /// (see `docs/reference/KEY_LEARNINGS.md`'s Deviation Absorber section).
    /// Shares its arithmetic with the live tick rather than mirroring it by
    /// hand: `BaseLoad::natural_base_kw` for the measured/heuristic/profile
    /// precedence, and `BaseLoadSmoothingState::next_offset_kw` (the pure half
    /// of what `tick()` calls as `update`) for the offset.
    /// `peek_base_load_kw_matches_tick_output_for_same_now` in
    /// `simulator/tests/peek_base_load_kw_tests.rs` guards against re-drift.
    pub fn peek_base_load_kw(
        &self,
        asset_id: &str,
        now: DateTime<Utc>,
        dt_s: f64,
        base_load_kw_override: Option<f64>,
        base_load_alpha: f64,
        feeds: &super::AssetTickFeeds,
    ) -> Option<f64> {
        let (_, cfg) = self.find_asset(asset_id)?;
        let bl_cfg = cfg.as_any().downcast_ref::<BaseLoad>()?;

        // Reproduce exactly what `tick()` is about to do, without writing it back:
        // the same `natural_base_kw` precedence and the same offset arithmetic.
        let measured_kw = feeds.measured_kw.get(asset_id).copied();
        let natural_base_kw =
            bl_cfg.natural_base_kw(measured_kw, feeds.heuristic_kw(asset_id, now), now);
        let smoothing = self
            .base_load_smoothing
            .get(asset_id)
            .cloned()
            .unwrap_or_default();
        let offset_kw = smoothing.next_offset_kw(
            base_load_kw_override,
            natural_base_kw,
            dt_s,
            base_load_alpha,
        );
        Some(super::BaseLoadSmoothingState::baseline_kw(
            natural_base_kw,
            offset_kw,
        ))
    }
}
