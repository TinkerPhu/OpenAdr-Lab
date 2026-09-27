// ── The thermostat rule ───────────────────────────────────────────────────────
// Split out of `heater.rs` to stay under the VEN/src/ 500-production-line cap
// (`ven-architecture`). One concept, one place: what the thermostat forces
// regardless of what anyone commands, at both ends of the band.
//
// Three levels of precedence, most authoritative first:
//   1. the safety ceiling (`temp_safety_max_c`, relaxed to it only in Absorb)
//   2. the emergency at `temp_min_c` — comfort beats relay protection
//   3. the ceiling deadband — off at `temp_max_c`, out until `max - delta`
//
// `step_inner`, `capability_inner` and `flexibility_floor_inner` all read this
// single rule, which is what keeps the live tick, the MILP's view of the asset
// and the capacity projection from ever disagreeing about the same heater.

use super::heater::{Heater, HeaterEmergencyMode, HeaterState};

impl Heater {
    /// Is the thermostat's emergency heat running under `mode`: at/below `temp_min_c`,
    /// or still inside the hysteresis band of an emergency that already fired. Curtail
    /// suppresses it: drifting toward ambient below `temp_min_c` is then the desired
    /// response, not a fault to fight (§2 — no physical floor on this side).
    pub(crate) fn emergency_active_in(
        &self,
        state: &HeaterState,
        mode: HeaterEmergencyMode,
    ) -> bool {
        mode != HeaterEmergencyMode::Curtail
            && (state.temperature_c <= self.temp_min_c
                || (state.emergency_latched
                    && state.temperature_c < self.temp_min_c + self.thermostat_delta_c))
    }

    /// The power the thermostat forces regardless of setpoint, if any: off at
    /// the forced-off ceiling, full power in an emergency. The single rule
    /// `step_inner`, `capability_inner` and `flexibility_floor_inner` all read.
    pub(crate) fn thermostat_forced_kw(&self, state: &HeaterState) -> Option<f64> {
        self.thermostat_forced_kw_in(state, self.emergency_mode)
    }

    /// Same rule, evaluated as if `mode` were active — answers the arbiter's
    /// what-if questions ("heat forced without Curtail?", "room under Absorb?").
    pub(crate) fn thermostat_forced_kw_in(
        &self,
        state: &HeaterState,
        mode: HeaterEmergencyMode,
    ) -> Option<f64> {
        let emergency_active = self.emergency_active_in(state, mode);
        // Absorb mode relaxes the forced-off ceiling from temp_max_c to the true
        // safety ceiling temp_safety_max_c (§2).
        let safety_ceiling_c = if mode == HeaterEmergencyMode::Absorb {
            self.temp_safety_max_c
        } else {
            self.temp_max_c
        };
        if state.temperature_c >= safety_ceiling_c {
            Some(0.0)
        } else if emergency_active {
            // Comfort outranks relay protection: a tank at its floor reheats at
            // once, even carrying a ceiling latch from earlier.
            Some(self.max_kw)
        } else if state.ceiling_latched
            && state.temperature_c > self.temp_max_c - self.thermostat_delta_c
        {
            // Cut out at the ceiling, stay out until a full delta below it.
            Some(0.0)
        } else {
            None
        }
    }
}
