//! `Heater`'s time to the target it holds, split out of `heater.rs` to stay under the file-size cap.
//!
//! The one place the tank's energy gap becomes time, so the arbiter's latest start
//! (`opportunistic::request_regime`) converts nothing itself. The target is the ceiling the
//! heater holds this tick (`temp_max_c`: a request's or the comfort target, lowered by
//! `apply_tick_overrides`). Reads the thermal model; never decides when the heater starts.

use super::heater::{Heater, HeaterState};

impl Heater {
    /// Seconds at full power to lift the tank from its temperature to the held target, with the
    /// loss it will see on the way: `dT/dt = (P - draw - k (T - ambient)) / C` rises toward
    /// `ambient + (P - draw) / k` and reaches the target after `(C / k) ln((eq - T) / (eq - target))`.
    /// Zero at or above the target; `None` when full power never lifts it that far.
    pub fn full_effort_time_to_target_s_inner(&self, state: &HeaterState) -> Option<f64> {
        let target_c = self.temp_max_c;
        if state.temperature_c >= target_c {
            return Some(0.0);
        }
        let net_kw = self.max_kw - self.draw_kw;
        let time_h = if self.k_loss_kw_per_c <= f64::EPSILON {
            (net_kw > 0.0)
                .then(|| (target_c - state.temperature_c) * self.thermal_mass_kwh_per_c / net_kw)?
        } else {
            let eq_c = self.ambient_temp_c + net_kw / self.k_loss_kw_per_c;
            if eq_c <= target_c {
                return None;
            }
            self.thermal_mass_kwh_per_c / self.k_loss_kw_per_c
                * ((eq_c - state.temperature_c) / (eq_c - target_c)).ln()
        };
        Some(crate::entities::units::s_from_h(time_h))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assets::{Asset, AssetState};

    fn heater(k_loss_kw_per_c: f64, target_c: f64) -> Heater {
        Heater {
            default_target_temp_c: None,
            max_kw: 2.5,
            power_stages: 1,
            temp_min_c: 20.0,
            temp_max_c: target_c,
            temp_min_c_profile: 20.0,
            temp_max_c_profile: target_c,
            temp_safety_max_c: target_c,
            thermostat_delta_c: 3.0,
            emergency_mode: Default::default(),
            thermal_mass_kwh_per_c: 2.0,
            k_loss_kw_per_c,
            draw_kw: 0.0,
            switching_penalty_eur: 0.0,
            ambient_temp_c: 10.0,
        }
    }

    fn at(temperature_c: f64) -> HeaterState {
        HeaterState {
            temperature_c,
            actual_power_kw: 0.0,
            emergency_latched: false,
            ceiling_latched: false,
        }
    }

    #[test]
    fn without_loss_it_is_the_energy_over_the_rating() {
        let h = heater(0.0, 50.0);
        let expect_s = 5.0 * h.thermal_mass_kwh_per_c / h.max_kw * 3600.0;
        let got = h.full_effort_time_to_target_s_inner(&at(45.0)).unwrap();
        assert!((got - expect_s).abs() < 1e-6, "{got} vs {expect_s}");
    }

    #[test]
    fn loss_makes_it_longer_than_the_lossless_time() {
        let lossless = heater(0.0, 50.0).full_effort_time_to_target_s_inner(&at(30.0));
        let lossy = heater(0.02, 50.0).full_effort_time_to_target_s_inner(&at(30.0));
        assert!(lossy.unwrap() > lossless.unwrap());
    }

    #[test]
    fn stepping_at_full_power_for_that_time_reaches_the_target() {
        let h = heater(0.02, 50.0);
        let secs = h.full_effort_time_to_target_s_inner(&at(30.0)).unwrap();
        let mut state = at(30.0);
        for _ in 0..secs.round() as u64 {
            state = h
                .step_inner(&state, h.max_kw, chrono::Duration::seconds(1))
                .0;
        }
        assert!(
            (state.temperature_c - 50.0).abs() < 0.05,
            "{}",
            state.temperature_c
        );
    }

    #[test]
    fn at_or_above_the_target_it_is_zero() {
        assert_eq!(
            heater(0.02, 50.0).full_effort_time_to_target_s_inner(&at(50.0)),
            Some(0.0)
        );
        assert_eq!(
            heater(0.02, 50.0).full_effort_time_to_target_s_inner(&at(60.0)),
            Some(0.0)
        );
    }

    #[test]
    fn a_target_full_power_never_reaches_has_no_time() {
        let h = heater(0.1, 1000.0);
        assert_eq!(h.full_effort_time_to_target_s_inner(&at(30.0)), None);
    }

    #[test]
    fn the_asset_declares_it() {
        let h = heater(0.02, 50.0);
        let declared = Asset::full_effort_time_to_target_s(&h, &AssetState::Heater(at(30.0)));
        assert!(declared.is_some());
        assert_eq!(declared, h.full_effort_time_to_target_s_inner(&at(30.0)));
    }
}
