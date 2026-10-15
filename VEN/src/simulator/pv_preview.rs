//! `SimState::peek_pv_kw` — split into its own file to keep `simulator/mod.rs`
//! under the file-size cap; behaves as an ordinary `impl SimState` method.

use chrono::{DateTime, Utc};

use crate::assets::{PvInverter, PvPowerInputs};

use super::SimState;

/// This tick's PV inputs, resolved once, readable under any generation limit: the limit in
/// force (what the tick will produce) or a limit the caller resolved — e.g. the limit without
/// the arbiter's own tightening, for its release check (R-88).
pub struct PvPreview<'a> {
    pv: &'a PvInverter,
    inputs: PvPowerInputs,
    /// This array's share of the site generation limit (`generation_limit_share`).
    limit_share: f64,
}

impl PvPreview<'_> {
    /// Under the generation limit the inverter holds now.
    pub fn in_force_kw(&self) -> f64 {
        self.pv.resolve_power_kw(&self.inputs)
    }

    /// Under `generation_limit_kw` (≤ 0; `None` = unlimited) instead.
    /// This array's output under the site generation limit `generation_limit_kw`, of which it
    /// takes its share.
    pub fn under_limit_kw(&self, generation_limit_kw: Option<f64>) -> f64 {
        let own_limit_kw = generation_limit_kw.map(|l| l * self.limit_share);
        self.pv.power_under_limit_kw(&self.inputs, own_limit_kw)
    }
}

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
    pub fn peek_pv(
        &self,
        asset_id: &str,
        now: DateTime<Utc>,
        dt_s: f64,
        pv_irradiance_override: Option<f64>,
        pv_tau_s: f64,
        feeds: &super::AssetTickFeeds,
    ) -> Option<PvPreview<'_>> {
        let (_, cfg) = self.find_asset(asset_id)?;
        let pv_cfg = cfg.as_any().downcast_ref::<PvInverter>()?;
        let limit_share =
            super::generation_limit_share(cfg, super::total_ac_ceiling_kw(&self.asset_configs));

        // Reproduce exactly what `tick()` is about to do to the smoothing state,
        // without writing it back: same natural curve, same offset arithmetic.
        let natural_irradiance = PvInverter::natural_irradiance_at(now);
        let offset = self.pv_smoothing.next_offset(
            pv_irradiance_override,
            natural_irradiance,
            dt_s,
            pv_tau_s,
        );
        let inputs = PvPowerInputs {
            measured_power_kw: feeds.measured_kw.get(asset_id).copied(),
            weather_power_kw: feeds.weather_pv_kw.get(asset_id).copied(),
            irradiance: (natural_irradiance + offset).clamp(0.0, 1.0),
            irradiance_offset: offset,
            irradiance_forced: pv_irradiance_override.is_some(),
        };
        Some(PvPreview {
            pv: pv_cfg,
            inputs,
            limit_share,
        })
    }

    /// This tick's PV output under the generation limit in force — `peek_pv(..)` read as
    /// the tick will actually produce it. Test-only anchor: `peek_pv_kw_matches_tick_output_for_same_now`
    /// proves `peek_pv` and `tick()` cannot diverge; production reads `peek_pv` directly.
    #[cfg(test)]
    pub fn peek_pv_kw(
        &self,
        asset_id: &str,
        now: DateTime<Utc>,
        dt_s: f64,
        pv_irradiance_override: Option<f64>,
        pv_tau_s: f64,
        feeds: &super::AssetTickFeeds,
    ) -> Option<f64> {
        self.peek_pv(asset_id, now, dt_s, pv_irradiance_override, pv_tau_s, feeds)
            .map(|preview| preview.in_force_kw())
    }
}
