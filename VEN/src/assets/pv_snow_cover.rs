//! The PV asset's own snow-cover conclusion (R-55), split out of `pv.rs` to stay under the
//! VEN/src/ 500-production-line cap; behaves as an ordinary `impl PvInverter` block.

use super::PvInverter;

impl PvInverter {
    /// Cross-check the live measurement against the snow-free weather forecast and update
    /// `snow_state` (R-55). Silent when there is nothing to compare: no measurement, no fresh
    /// weather series, or conditions too dim to tell (`PvSnowState::observed`). A generation
    /// limit lowers what the array is expected to deliver, so a curtailed array is not read
    /// as a covered one.
    pub(crate) fn observe_snow(&mut self) {
        let (Some(series), Some(now)) = (self.weather_forecast.as_deref(), self.live_inputs_at)
        else {
            return;
        };
        self.snow_state = lab_forecast::pv_snow_observation::observe_snow(
            self.snow_state,
            lab_forecast::pv_snow_observation::SnowObservationInput {
                series,
                now,
                measured_kw: self.measured_power_kw,
                generation_limit_kw: self.generation_limit_kw,
                rated_kw: self.rated_kw,
            },
        );
    }
}
