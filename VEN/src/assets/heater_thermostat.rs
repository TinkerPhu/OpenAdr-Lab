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
// `forced_kw_ahead` replays the same rule forward so the planner can pin what the
// thermostat will force (R-94); it must never re-state the rule itself.

use super::heater::{Heater, HeaterEmergencyMode, HeaterState};
use crate::entities::units::energy_kwh;

impl Heater {
    /// The ceiling the thermostat cuts out at under `mode`: `temp_max_c`, or the true
    /// safety ceiling `temp_safety_max_c` in Absorb (§2).
    fn cut_out_ceiling_c(&self, mode: HeaterEmergencyMode) -> f64 {
        if mode == HeaterEmergencyMode::Absorb {
            self.temp_safety_max_c
        } else {
            self.temp_max_c
        }
    }

    /// Both latches after one step from `state` to `new_temp_c` under `mode`:
    /// `(emergency_latched, ceiling_latched)`. Arming and release both read
    /// `state.temperature_c` -- the value `thermostat_forced_kw_in` judged the step
    /// by -- so a latch cannot disagree with the decision it belongs to. Keying
    /// release off `new_temp_c` would re-latch as soon as the reheat began.
    pub(crate) fn latches_after(
        &self,
        state: &HeaterState,
        new_temp_c: f64,
        mode: HeaterEmergencyMode,
    ) -> (bool, bool) {
        let ceiling_c = self.cut_out_ceiling_c(mode);
        let emergency_latched = self.emergency_active_in(state, mode)
            && new_temp_c < self.temp_min_c + self.thermostat_delta_c;
        let ceiling_latched = state.temperature_c >= ceiling_c
            || new_temp_c >= ceiling_c
            || (state.ceiling_latched && state.temperature_c > ceiling_c - self.thermostat_delta_c);
        (emergency_latched, ceiling_latched)
    }

    /// What the thermostat will force in each slot from `state` on, as long as it
    /// forces anything: one entry per leading slot whose start the rule overrides,
    /// stopping at the first slot it leaves free (an empty Vec when it forces
    /// nothing now). Replayed in the planner's own tank model -- a constant
    /// `q_dem_kw` -- so the pinned slots agree with the MILP's trajectory. A slot is
    /// pinned when the latch holds at its start; one releasing mid-slot is left to
    /// the planner, which a 5-minute replan corrects.
    pub(crate) fn forced_kw_ahead(
        &self,
        state: &HeaterState,
        q_dem_kw: f64,
        dt_h: &[f64],
    ) -> Vec<f64> {
        let mode = self.emergency_mode;
        let mut state = state.clone();
        let mut forced = Vec::new();
        for &dt in dt_h {
            let Some(kw) = self.thermostat_forced_kw_in(&state, mode) else {
                break;
            };
            forced.push(kw);
            let new_temp_c =
                state.temperature_c + energy_kwh(kw - q_dem_kw, dt) / self.thermal_mass_kwh_per_c;
            (state.emergency_latched, state.ceiling_latched) =
                self.latches_after(&state, new_temp_c, mode);
            state.temperature_c = new_temp_c;
        }
        forced
    }

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
        // safety ceiling temp_safety_max_c (§2), and the deadband with it.
        let ceiling_c = self.cut_out_ceiling_c(mode);
        if state.temperature_c >= ceiling_c {
            Some(0.0)
        } else if emergency_active {
            // Comfort outranks relay protection: a tank at its floor reheats at
            // once, even carrying a ceiling latch from earlier.
            Some(self.max_kw)
        } else if state.ceiling_latched && state.temperature_c > ceiling_c - self.thermostat_delta_c
        {
            // Cut out at the ceiling, stay out until a full delta below it.
            Some(0.0)
        } else {
            None
        }
    }
}
