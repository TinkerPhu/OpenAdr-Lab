//! `Heater`'s `TickOverridable`/`MilpParticipant`/`Thermostat` impls — split
//! into their own file (Spec A Phase 2b, R-file-size) to keep `heater.rs`
//! under the file-size cap; each is its own separate trait+type combination,
//! so (unlike `impl Asset for Heater` itself) they can live in a different
//! file without violating Rust's "one coherent impl per trait/type pair"
//! rule.

use super::heater::HeaterState;
use super::own_state::own;
use super::{AssetState, Heater, MilpParticipant, Thermostat, TickOverridable};
use crate::assets::TickOverrides;
use crate::entities::timeline::HeaterPlanTrajectory;

impl TickOverridable for Heater {
    /// Delegates to the pre-existing inherent `Heater::apply_tick_overrides`
    /// (design.md Decision D5). Deliberately named the same as the trait
    /// method it wraps — safe because Rust always resolves `Self::method(...)`
    /// to an inherent method over a same-named trait method, and the two have
    /// different arities (5 plain args here vs. `(state, overrides)` on the
    /// trait) so there's no ambiguity either way. Kept rather than renamed to
    /// avoid touching the inherent method's own (unrelated) call sites.
    fn apply_tick_overrides(&mut self, _state: &mut AssetState, overrides: &TickOverrides) {
        Self::apply_tick_overrides(
            self,
            overrides.heater_ambient_temp_c_override,
            overrides.heater_temp_min_override,
            overrides.heater_temp_max_override,
            overrides.heater_emergency_curtail_override,
            overrides.heater_emergency_absorb_override,
        );
    }
}

impl MilpParticipant for Heater {
    fn build_milp_context(
        &self,
        inputs: &super::MilpBuildInputs<'_>,
    ) -> Box<dyn crate::controller::milp_planner::AssetMilpContext> {
        let mut ctx = crate::controller::milp_planner::asset_port::HeaterMilpContext::from_state(
            inputs.asset_id,
            inputs.state,
            self,
            inputs.n,
            inputs.cum_s,
            inputs.now,
            inputs.heater_target,
            self.switching_penalty_eur,
            inputs.c_terminal_eur_kwh,
            inputs.heater_anchor.clone(),
            inputs.planner.w_ghg,
        );
        // Only the tier preference is a heater friction; relay switching is priced by
        // `lambda_sw_eur`, which the context already carries.
        ctx.friction = crate::controller::milp_planner::FrictionCosts {
            startup_eur: 0.0,
            ramp_eur_kw: 0.0,
            tier_penalty_eur: inputs.planner.w_tier_penalty_eur,
        };
        Box::new(ctx)
    }
}

impl Thermostat for Heater {
    fn plan_trajectory(&self, live_state: &AssetState) -> Option<HeaterPlanTrajectory> {
        Self::plan_trajectory(self, live_state)
    }

    /// Moved here verbatim from `AssetConfig::thermostat_setpoint_kw`'s only
    /// real arm (Heater is the only asset kind with thermostat behavior
    /// today). Returns `f64` directly rather than `Option<f64>` — the
    /// original's `None` case was purely "not a Heater," which capability-gating
    /// (`as_thermostat() -> Option<&dyn Thermostat>`) already handles; within
    /// this arm the original always returned `Some(...)`.
    fn default_request_target_c(&self) -> Option<f64> {
        self.default_target_temp_c
    }

    fn thermal_request_defaults(
        &self,
        state: &AssetState,
    ) -> crate::entities::asset_params::ThermalRequestDefaults {
        let s: &HeaterState = own(state);
        crate::entities::asset_params::ThermalRequestDefaults {
            temperature_c: s.temperature_c,
            temp_min_c: self.temp_min_c,
            thermal_mass_kwh_per_c: self.thermal_mass_kwh_per_c,
            rated_kw: self.max_kw,
        }
    }

    fn thermostat_setpoint_kw(&self, state: &AssetState, target_c: f64) -> f64 {
        let s: &HeaterState = own(state);
        if s.temperature_c < target_c {
            self.max_kw
        } else {
            0.0
        }
    }
}
