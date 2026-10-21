//! Unit conversions, written once.
//!
//! Power is kW everywhere in the VEN, the simulated site meter included; an energy is the
//! power times a step in hours. The same arithmetic used to be spelled inline in about 22
//! places in two styles (`num_milliseconds() / 3_600_000.0` and `seconds / 3600.0`), so a
//! changed convention or a slip had to be found everywhere (R-111). Names carry the units.
//! `audit_ven_architecture.py` rule 10 keeps the inline spellings out.
//!
//! The conversions the BFF makes too (hours from a step, energy from power and back) live in
//! `lab_core::units` and are re-exported here, so the VEN still names one module for all of them.

pub use lab_core::units::{dt_h_from_duration, dt_h_from_s, energy_kwh};

/// Energy [kWh] of `power_kw` held for `minutes`. Not `energy_kwh(power_kw, minutes / 60.0)`:
/// the multiplication comes first so the result is bit-identical to the `power * minutes / 60.0`
/// it replaced.
pub fn energy_kwh_from_min(power_kw: f64, minutes: f64) -> f64 {
    power_kw * minutes / 60.0
}

/// Seconds in `h` hours: the inverse of `dt_h_from_s`.
pub fn s_from_h(h: f64) -> f64 {
    h * 3600.0
}

/// Seconds in `minutes` minutes.
pub fn s_from_min(minutes: f64) -> f64 {
    minutes * 60.0
}

/// A fraction (0..1, e.g. a state of charge) as a percentage.
pub fn pct_from_frac(frac: f64) -> f64 {
    frac * 100.0
}

/// The profile's Zone A planning step [s] (5 minutes), for tests that need the default grid.
#[cfg(test)]
pub const ZONE_A_STEP_S: f64 = 300.0;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fraction_becomes_a_percentage() {
        assert_eq!(pct_from_frac(0.5), 50.0);
        assert_eq!(pct_from_frac(1.0), 100.0);
        assert_eq!(pct_from_frac(0.0), 0.0);
    }

    /// The Zone A step is a twelfth of an hour through the shared conversion.
    #[test]
    fn the_zone_a_step_is_a_twelfth_of_an_hour() {
        assert_eq!(dt_h_from_s(ZONE_A_STEP_S), 1.0 / 12.0);
    }

    #[test]
    fn hours_and_minutes_become_seconds() {
        assert_eq!(s_from_h(1.5), 5400.0);
        assert_eq!(s_from_min(15.0), 900.0);
        assert_eq!(dt_h_from_s(s_from_h(2.25)), 2.25);
    }

    #[test]
    fn energy_is_power_times_minutes() {
        assert_eq!(energy_kwh_from_min(2.0, 15.0), 0.5);
        assert_eq!(energy_kwh_from_min(2.0, 60.0), 2.0);
    }
}
