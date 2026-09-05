//! `Heater`'s `TickOverridable`/`MilpParticipant`/`Thermostat` impls — split
//! into their own file (Spec A Phase 2b, R-file-size) to keep `heater.rs`
//! under the file-size cap; each is its own separate trait+type combination,
//! so (unlike `impl Asset for Heater` itself) they can live in a different
//! file without violating Rust's "one coherent impl per trait/type pair"
//! rule.

use chrono::{DateTime, Utc};

use super::{AssetState, Heater, MilpParticipant, Thermostat, TickOverridable};
use crate::assets::TickOverrides;
use crate::entities::device_session::{EvSession, HeaterTarget};
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
    #[allow(clippy::too_many_arguments)] // trait-mandated signature shared by 4 heterogeneous asset kinds — see trait doc
    fn build_milp_context(
        &self,
        _asset_id: &str,
        state: &AssetState,
        n: usize,
        cum_s: &[i64],
        now: DateTime<Utc>,
        _ev_session: Option<&EvSession>,
        heater_target: Option<&HeaterTarget>,
        _ev_min_charge_kw: f64,
        _v_ev_extra_eur_kwh: f64,
        _v_ev_core_eur_kwh: f64,
        _asap_lateness_eur_kwh_h: f64,
        _v_ev_free_charge_eur_kwh: f64,
        lambda_sw: f64,
        c_terminal_eur_kwh: f64,
        heater_anchor: Vec<Option<f64>>,
        w_ghg_eur_kg: f64,
    ) -> Box<dyn crate::controller::milp_planner::AssetMilpContext> {
        Box::new(
            crate::controller::milp_planner::asset_port::HeaterMilpContext::from_state(
                state,
                self,
                n,
                cum_s,
                now,
                heater_target,
                lambda_sw,
                c_terminal_eur_kwh,
                heater_anchor,
                w_ghg_eur_kg,
            ),
        )
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
    fn thermostat_setpoint_kw(&self, state: &AssetState, target_c: f64) -> f64 {
        let AssetState::Heater(s) = state else {
            unreachable!("Heater/state mismatch")
        };
        if s.temperature_c < target_c {
            self.max_kw
        } else {
            0.0
        }
    }
}
