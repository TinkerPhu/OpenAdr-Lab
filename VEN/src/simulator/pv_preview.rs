//! `SimState::peek_pv_kw` — split into its own file to keep `simulator/mod.rs`
//! under the file-size cap; behaves as an ordinary `impl SimState` method.

use chrono::{DateTime, Utc};

use crate::assets::{AssetConfig, PvInverter, PvPowerInputs};

use super::SimState;

impl SimState {
    /// Preview this tick's PV output *before* `tick()` mutates state
    /// (read-only). `None` if no PV asset is configured.
    ///
    /// Lets `apply_surplus_ev_overlay` avoid a one-tick lag from reading last
    /// tick's `AssetSnapshot.power_kw` (see its doc comment for the full
    /// rationale). Shares its arithmetic with the live tick rather than
    /// mirroring it by hand: the natural curve comes from
    /// `PvInverter::natural_irradiance_at`, the offset decay from
    /// `PvSmoothingState::next_offset` (the pure half of what `tick()` calls
    /// as `update`), and the precedence/clipping rules from
    /// `PvInverter::resolve_power_kw` (the same function `step_inner` calls).
    /// `peek_pv_kw_matches_tick_output_for_same_now` in
    /// `simulator/tests/peek_pv_kw_tests.rs` guards against re-drift.
    pub fn peek_pv_kw(
        &self,
        now: DateTime<Utc>,
        dt_s: f64,
        pv_irradiance_override: Option<f64>,
        pv_alpha: f64,
        weather_pv_kw: Option<f64>,
        pv_measured_kw: Option<f64>,
    ) -> Option<f64> {
        let pv_cfg = self.asset_configs.iter().find_map(|cfg| match cfg {
            AssetConfig::Pv(pv) => Some(pv),
            _ => None,
        })?;

        // Reproduce exactly what `tick()` is about to do to the smoothing state,
        // without writing it back: same natural curve, same offset arithmetic.
        let natural_irradiance = PvInverter::natural_irradiance_at(now);
        let offset = self.pv_smoothing.next_offset(
            pv_irradiance_override,
            natural_irradiance,
            dt_s,
            pv_alpha,
        );
        Some(pv_cfg.resolve_power_kw(&PvPowerInputs {
            measured_power_kw: pv_measured_kw,
            weather_power_kw: weather_pv_kw,
            irradiance: (natural_irradiance + offset).clamp(0.0, 1.0),
            irradiance_offset: offset,
            irradiance_forced: pv_irradiance_override.is_some(),
        }))
    }
}
