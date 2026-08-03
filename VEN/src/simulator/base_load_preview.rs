//! `SimState::peek_base_load_kw` — split into its own file to keep
//! `simulator/mod.rs` under the file-size cap; behaves as an ordinary
//! `impl SimState` method. Mirrors `pv_preview.rs`'s structure exactly.

use chrono::{DateTime, Utc};

use crate::assets::AssetConfig;

use super::SimState;

impl SimState {
    /// Preview this tick's base-load output *before* `tick()` mutates state
    /// (same natural-profile + Behaviour B offset-decay formula as `tick()` in
    /// `mod.rs`; read-only). `None` if no base-load asset is configured.
    ///
    /// Closes the base-load half of the one-tick lag `peek_pv_kw` already
    /// closes for PV — the deviation arbiter needs both uncontrollable
    /// inputs to be this tick's value, not last tick's `AssetSnapshot.power_kw`
    /// (see `docs/reference/KEY_LEARNINGS.md`'s Deviation Absorber section). Must stay in
    /// lockstep with `tick()`'s formula —
    /// `peek_base_load_kw_matches_tick_output_for_same_now` in
    /// `simulator/tests/peek_base_load_kw_tests.rs` guards against drift.
    pub fn peek_base_load_kw(
        &self,
        now: DateTime<Utc>,
        dt_s: f64,
        base_load_kw_override: Option<f64>,
        base_load_alpha: f64,
        base_load_measured_kw: Option<f64>,
    ) -> Option<f64> {
        let bl_cfg = self.asset_configs.iter().find_map(|cfg| match cfg {
            AssetConfig::BaseLoad(bl) => Some(bl),
            _ => None,
        })?;

        // Mirrors SimState::tick's AssetConfig::BaseLoad branch exactly: the
        // override folds into an offset relative to `natural_base_kw` (measured
        // reading if present, else profile + simulated appliance noise), so a
        // forced value lands exactly on `forced_kw`; without an override the
        // existing offset decays via the same EMA used for PV's irradiance offset.
        const PLAN_STEP_S: f64 = 300.0;
        let natural_base_kw = base_load_measured_kw
            .unwrap_or_else(|| bl_cfg.baseline_kw_profile + bl_cfg.appliance_noise_kw(now));
        let offset_kw = if let Some(forced_kw) = base_load_kw_override {
            forced_kw - natural_base_kw
        } else {
            let per_tick_factor = (1.0 - base_load_alpha).powf(dt_s / PLAN_STEP_S);
            let mut offset = self.base_load_smoothing.load_offset_kw * per_tick_factor;
            if offset.abs() < 0.005 {
                offset = 0.0;
            }
            offset
        };
        Some((natural_base_kw + offset_kw).max(0.0))
    }
}
