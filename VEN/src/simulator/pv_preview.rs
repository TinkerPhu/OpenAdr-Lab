//! `SimState::peek_pv_kw` — split into its own file to keep `simulator/mod.rs`
//! under the file-size cap; behaves as an ordinary `impl SimState` method.

use chrono::{DateTime, Utc};

use crate::assets::{AssetConfig, PvInverter};

use super::SimState;

impl SimState {
    /// Preview this tick's PV output *before* `tick()` mutates state (same
    /// irradiance formula, and the same weather/manual-override precedence,
    /// as `tick()` in `mod.rs`; read-only). `None` if no PV asset is
    /// configured.
    ///
    /// Lets `apply_surplus_ev_overlay` avoid a one-tick lag from reading last
    /// tick's `AssetSnapshot.power_kw` (see its doc comment for the full
    /// rationale). Must stay in lockstep with `tick()`'s irradiance formula —
    /// `peek_pv_kw_matches_tick_output_for_same_now` in `simulator/tests.rs`
    /// guards against drift.
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

        // Mirrors SimState::tick's / PvInverter::step_inner's precedence exactly: a
        // forced override (posted this exact tick) wins outright; otherwise weather
        // (if present) is the base with the decaying offset blended additively on
        // top; otherwise the sin model + offset. Kept in lockstep by the
        // peek_pv_kw_matches_tick_output_for_same_now equivalence test.
        let raw_kw = if let Some(forced) = pv_irradiance_override {
            -(pv_cfg.rated_kw * forced.clamp(0.0, 1.0))
        } else {
            const PLAN_STEP_S: f64 = 300.0;
            let per_tick_factor = (1.0 - pv_alpha).powf(dt_s / PLAN_STEP_S);
            let mut offset = self.pv_smoothing.irradiance_offset * per_tick_factor;
            if offset.abs() < 0.005 {
                offset = 0.0;
            }
            match pv_measured_kw.or(weather_pv_kw) {
                Some(base_kw) => -(base_kw.max(0.0) + offset * pv_cfg.rated_kw).max(0.0),
                None => {
                    let natural_irradiance = PvInverter::natural_irradiance_at(now);
                    let irradiance = (natural_irradiance + offset).clamp(0.0, 1.0);
                    -(pv_cfg.rated_kw * irradiance)
                }
            }
        };
        Some(
            pv_cfg
                .generation_limit_kw
                .map(|lim| raw_kw.max(lim))
                .unwrap_or(raw_kw),
        )
    }
}
