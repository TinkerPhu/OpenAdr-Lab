//! `SimState::peek_base_load_kw` — split into its own file to keep
//! `simulator/mod.rs` under the file-size cap; behaves as an ordinary
//! `impl SimState` method. Mirrors `pv_preview.rs`'s structure exactly.

use chrono::{DateTime, Utc};

use crate::assets::BaseLoad;

use super::SimState;

impl SimState {
    /// Preview this tick's base-load output *before* `tick()` mutates state
    /// (same natural-profile + Behaviour B offset-decay formula as `tick()` in
    /// `mod.rs`; read-only). `None` if no base-load asset is configured.
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
        now: DateTime<Utc>,
        dt_s: f64,
        base_load_kw_override: Option<f64>,
        base_load_alpha: f64,
        base_load_measured_kw: Option<f64>,
        base_load_heuristic_kw: Option<f64>,
    ) -> Option<f64> {
        let bl_cfg = self
            .asset_configs
            .iter()
            .find_map(|cfg| cfg.as_any().downcast_ref::<BaseLoad>())?;

        // Reproduce exactly what `tick()` is about to do, without writing it back:
        // the same `natural_base_kw` precedence and the same offset arithmetic.
        let natural_base_kw =
            bl_cfg.natural_base_kw(base_load_measured_kw, base_load_heuristic_kw, now);
        let offset_kw = self.base_load_smoothing.next_offset_kw(
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
