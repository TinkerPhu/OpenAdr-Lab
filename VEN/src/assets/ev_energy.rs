//! `EvCharger`'s energy to its target, split out of `ev.rs` to stay under the file-size cap.
//!
//! The one product behind three answers: the time to target (the arbiter's latest start), a
//! session's core energy (the planner) and the unserved energy at departure (the fleet report).
//! Reads the pack and the active target; never decides when the car leaves.

use super::ev::{EvCharger, EvState};

impl EvCharger {
    /// Energy \[kWh\] to lift the pack from one state of charge to another; zero when already there.
    /// The one product behind the time to target, a session's core energy and the unserved energy.
    pub fn energy_to_soc_kwh(&self, from_soc_frac: f64, to_soc_frac: f64) -> f64 {
        (to_soc_frac - from_soc_frac).max(0.0) * self.battery_kwh
    }

    /// Energy \[kWh\] to lift the state of charge to the active target (`soc_target_frac`, which
    /// a session's target overrides each tick); zero at or above it.
    pub fn energy_to_target_kwh_inner(&self, state: &EvState) -> f64 {
        self.energy_to_soc_kwh(state.soc_frac, self.soc_target_frac)
    }

    /// Seconds at the full charge rate to deliver `energy_to_target_kwh_inner`.
    pub fn full_effort_time_to_target_s_inner(&self, state: &EvState) -> f64 {
        crate::entities::units::s_from_h(
            self.energy_to_target_kwh_inner(state) / self.max_charge_kw,
        )
    }
}
