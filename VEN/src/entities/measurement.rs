//! Pure resolution logic shared by both real-measurement feeds (PV, baseline
//! load). Unlike `solar::resolve_weather_pv_kw`, a measurement is a live
//! meter reading for *right now*, not a forecast series — it never feeds the
//! planner's forward horizon, only the current live tick.

use chrono::{DateTime, Utc};

/// `(value_kw, reading's own timestamp)` — the shared vocabulary for a
/// real-measurement reading across the transport (`measurement.rs`),
/// translation (`measurement_translation.rs`), port
/// (`controller::MeasurementPort`), and resolution (this module) layers.
pub type MeasurementReading = (f64, DateTime<Utc>);

/// A measurement is considered stale (and treated as absent) once older than
/// this — much tighter than weather's 2h forecast-staleness threshold, since
/// this is a live meter, not an hourly forecast.
pub const MEASUREMENT_STALENESS_THRESHOLD: chrono::Duration = chrono::Duration::minutes(5);

/// The full "should this tick trust the measured reading" decision: `None`
/// unless a reading has actually been received AND it's still fresh relative
/// to `now`. Pure — the only I/O (fetching `latest_kw()` from a
/// `MeasurementPort`) happens in the caller.
pub fn resolve_measured_kw(
    latest: Option<MeasurementReading>,
    now: DateTime<Utc>,
    staleness_threshold: chrono::Duration,
) -> Option<f64> {
    let (value_kw, reading_at) = latest?;
    if now - reading_at > staleness_threshold {
        return None;
    }
    Some(value_kw)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(offset_s: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(1_700_000_000 + offset_s, 0).unwrap()
    }

    #[test]
    fn resolve_measured_kw_none_when_never_received() {
        assert_eq!(
            resolve_measured_kw(None, t(0), MEASUREMENT_STALENESS_THRESHOLD),
            None
        );
    }

    #[test]
    fn resolve_measured_kw_returns_value_when_fresh() {
        let latest = Some((3.5, t(0)));
        assert_eq!(
            resolve_measured_kw(latest, t(60), MEASUREMENT_STALENESS_THRESHOLD),
            Some(3.5)
        );
    }

    #[test]
    fn resolve_measured_kw_none_when_exactly_at_threshold_or_older() {
        let latest = Some((3.5, t(0)));
        let threshold_s = MEASUREMENT_STALENESS_THRESHOLD.num_seconds();
        assert_eq!(
            resolve_measured_kw(latest, t(threshold_s), MEASUREMENT_STALENESS_THRESHOLD),
            Some(3.5)
        );
        assert_eq!(
            resolve_measured_kw(latest, t(threshold_s + 1), MEASUREMENT_STALENESS_THRESHOLD),
            None
        );
    }
}
