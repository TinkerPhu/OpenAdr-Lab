//! The PV asset's snow-cover conclusion, read for publication (R-55).

use crate::assets::PvInverter;
use crate::entities::pv_snow::PvSnowState;

use super::SimState;

impl SimState {
    /// What the site's arrays conclude about snow on their panels (`PvInverter::observe_snow`):
    /// covered when any array concludes so, whatever its place in the roster. Only a measured
    /// array can conclude it, and at most one array is measured; the default (uncovered) when the
    /// site has no PV.
    pub fn pv_snow_state(&self) -> PvSnowState {
        PvSnowState {
            covered: self
                .asset_configs
                .iter()
                .filter_map(|cfg| cfg.as_any().downcast_ref::<PvInverter>())
                .any(|pv| pv.snow_state.covered),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entities::asset_params::{AssetParams, PvParams};

    /// W7: the measured array may be the second one; its conclusion is the site's.
    #[test]
    fn pv_snow_state_is_covered_when_any_array_concludes_so() {
        let array = |id: &str| {
            AssetParams::Pv(PvParams {
                id: id.into(),
                ..Default::default()
            })
        };
        let mut sim =
            SimState::from_params(&[array("pv-east"), array("pv-west")], chrono::Utc::now());
        assert!(!sim.pv_snow_state().covered);
        let (_, cfg) = sim.find_asset_mut("pv-west").unwrap();
        cfg.as_any_mut()
            .downcast_mut::<PvInverter>()
            .unwrap()
            .snow_state
            .covered = true;
        assert!(sim.pv_snow_state().covered);
    }
}
