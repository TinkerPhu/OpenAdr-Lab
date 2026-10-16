//! Power and energy conversions both services make, written once.
//!
//! Power is kW and energy is kWh everywhere in this lab (`docs/reference/WIRE_PROFILE.md`); an
//! energy is a power times a span in hours, and a mean power is an energy divided by one. The
//! VEN builds its reports with these and the BFF reads the fleet's forecasts back with them, so
//! a report's kWh and the fleet chart's kW cannot disagree about the arithmetic between them.
//! The VEN's `entities::units` re-exports them beside its own VEN-only conversions.

use chrono::Duration;

/// A step in seconds as hours - the `dt_h` a power is integrated over.
pub fn dt_h_from_s(s: f64) -> f64 {
    s / 3600.0
}

/// A `Duration` as hours, at millisecond precision. For a whole number of seconds this is
/// bit-identical to `dt_h_from_s(seconds)`.
pub fn dt_h_from_duration(d: Duration) -> f64 {
    d.num_milliseconds() as f64 / 3_600_000.0
}

/// Energy [kWh] of `power_kw` held for `dt_h` hours.
pub fn energy_kwh(power_kw: f64, dt_h: f64) -> f64 {
    power_kw * dt_h
}

/// The mean power [kW] that delivers `energy_kwh` over `dt_h` hours: the inverse of
/// [`energy_kwh`]. `None` for a span that is not positive, which has no mean power.
pub fn mean_power_kw(energy_kwh: f64, dt_h: f64) -> Option<f64> {
    (dt_h > 0.0).then(|| energy_kwh / dt_h)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steps_become_hours() {
        assert_eq!(dt_h_from_s(1800.0), 0.5);
        assert_eq!(dt_h_from_duration(Duration::seconds(300)), 1.0 / 12.0);
        assert_eq!(
            dt_h_from_duration(Duration::milliseconds(500)),
            0.5 / 3600.0
        );
    }

    /// The two spellings must agree for every whole-second step.
    #[test]
    fn a_duration_and_its_seconds_give_the_same_dt_h() {
        for s in [1, 59, 60, 300, 900, 3600, 7200, 86_400, 604_800] {
            assert_eq!(
                dt_h_from_duration(Duration::seconds(s)),
                dt_h_from_s(s as f64),
                "{s} s"
            );
        }
    }

    #[test]
    fn energy_is_power_times_hours() {
        assert_eq!(energy_kwh(2.0, 0.25), 0.5);
    }

    /// What a report says in kWh per interval comes back as the kW it was built from.
    #[test]
    fn mean_power_and_energy_round_trip() {
        for (power_kw, s) in [(2.0, 900), (-3.5, 300), (0.0, 3600), (11.0, 60)] {
            let dt_h = dt_h_from_duration(Duration::seconds(s));
            let back = mean_power_kw(energy_kwh(power_kw, dt_h), dt_h).unwrap();
            assert!(
                (back - power_kw).abs() < 1e-12,
                "{power_kw} kW over {s} s -> {back}"
            );
        }
        assert_eq!(mean_power_kw(0.5, 0.25), Some(2.0));
    }

    #[test]
    fn an_empty_span_has_no_mean_power() {
        assert_eq!(mean_power_kw(1.0, 0.0), None);
        assert_eq!(mean_power_kw(1.0, -0.25), None);
    }
}
