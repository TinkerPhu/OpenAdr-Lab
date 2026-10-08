//! `AppState` accessors for the PV asset's snow-cover conclusion (R-55): published by the sim
//! tick from the asset (`PvInverter::snow_state`), read by every consumer that turns the weather
//! forecast into a PV forecast, so they all start from the same state instead of each guessing.

use super::AppState;
use crate::entities::pv_snow::PvSnowState;

impl AppState {
    pub async fn pv_snow_state(&self) -> PvSnowState {
        self.hems.read().await.pv_snow_state
    }

    pub async fn set_pv_snow_state(&self, snow_state: PvSnowState) {
        self.hems.write().await.pv_snow_state = snow_state;
    }
}
