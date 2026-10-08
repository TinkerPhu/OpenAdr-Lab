//! The PV asset's snow-cover conclusion, read for publication (R-55).

use crate::assets::PvInverter;
use crate::entities::pv_snow::PvSnowState;

use super::SimState;

impl SimState {
    /// What the PV asset currently concludes about snow on its panels
    /// (`PvInverter::observe_snow`); the default (uncovered) when the site has no PV.
    pub fn pv_snow_state(&self) -> PvSnowState {
        self.asset_configs
            .iter()
            .find_map(|cfg| cfg.as_any().downcast_ref::<PvInverter>())
            .map(|pv| pv.snow_state)
            .unwrap_or_default()
    }
}
