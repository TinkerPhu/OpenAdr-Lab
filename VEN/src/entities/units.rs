//! Unit conversions, written once.
//!
//! Power is kW everywhere in the VEN except the simulated meter's raw watts; an energy is the
//! power times a step in hours. The same arithmetic used to be spelled inline in about 22
//! places in two styles (`num_milliseconds() / 3_600_000.0` and `seconds / 3600.0`), so a
//! changed convention or a slip had to be found everywhere (R-111). Names carry the units.
//! `audit_ven_architecture.py` rule 10 keeps the inline spellings out.

use chrono::Duration;

/// Watts to kilowatts.
pub fn kw_from_w(w: f64) -> f64 {
    w / 1000.0
}

/// Kilowatts to watts.
pub fn w_from_kw(kw: f64) -> f64 {
    kw * 1000.0
}

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

/// Energy [kWh] of `power_kw` held for `minutes`. Not `energy_kwh(power_kw, minutes / 60.0)`:
/// the multiplication comes first so the result is bit-identical to the `power * minutes / 60.0`
/// it replaced.
pub fn energy_kwh_from_min(power_kw: f64, minutes: f64) -> f64 {
    power_kw * minutes / 60.0
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
    fn watts_and_kilowatts_are_exact_inverses() {
        for kw in [0.0, 1.5, -3.25, 7.4, 123.456] {
            assert_eq!(kw_from_w(w_from_kw(kw)), kw);
        }
        assert_eq!(kw_from_w(1500.0), 1.5);
        assert_eq!(w_from_kw(-2.5), -2500.0);
    }

    #[test]
    fn a_fraction_becomes_a_percentage() {
        assert_eq!(pct_from_frac(0.5), 50.0);
        assert_eq!(pct_from_frac(1.0), 100.0);
        assert_eq!(pct_from_frac(0.0), 0.0);
    }

    #[test]
    fn steps_become_hours() {
        assert_eq!(dt_h_from_s(1800.0), 0.5);
        assert_eq!(dt_h_from_s(ZONE_A_STEP_S), 1.0 / 12.0);
        assert_eq!(dt_h_from_duration(Duration::seconds(300)), 1.0 / 12.0);
        assert_eq!(
            dt_h_from_duration(Duration::milliseconds(500)),
            0.5 / 3600.0
        );
    }

    /// The two spellings this module replaced must agree for every whole-second step.
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
    fn energy_is_power_times_hours_or_minutes() {
        assert_eq!(energy_kwh(2.0, 0.25), 0.5);
        assert_eq!(energy_kwh_from_min(2.0, 15.0), 0.5);
        assert_eq!(energy_kwh_from_min(2.0, 60.0), 2.0);
    }
}
