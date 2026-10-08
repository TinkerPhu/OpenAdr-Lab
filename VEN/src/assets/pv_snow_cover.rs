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
        let Some((snow_free_kw, snow_possible)) =
            crate::entities::solar::weather_snow_reference_at(series, now)
        else {
            return;
        };
        if !snow_possible {
            // Warm air melts a covered panel: the forecast's own melt rule, applied to now, so a
            // state concluded in the cold cannot outlive the cold when the measurement is gone.
            self.snow_state.covered = false;
        }
        let Some(measured_kw) = self.measured_power_kw else {
            return;
        };
        let expected_kw = self
            .generation_limit_kw
            .map_or(snow_free_kw, |limit_kw| snow_free_kw.min(limit_kw));
        let evidence = crate::entities::pv_snow::SnowEvidence {
            expected_kw,
            measured_kw,
            snow_possible,
        };
        self.snow_state = self.snow_state.observed(evidence, self.rated_kw);
    }
}
