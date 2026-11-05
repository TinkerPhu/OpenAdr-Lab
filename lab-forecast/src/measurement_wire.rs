//! Wire-payload → `(value_kw, reading_at)` translation for real-measurement
//! MQTT feeds, shared by the VEN and the forecaster. **This is the one file a downstream deployer of this repo
//! needs to rewrite** to match their own meter/inverter/gateway's message
//! format — every other measurement-feed file (`measurement.rs`,
//! `controller/measurement_port.rs`) is device-agnostic transport plumbing.
//!
//! The shipped default expects `{"power_kw": f64, "ts": rfc3339}` — swap the
//! body of either function to parse whatever your own hardware/bridge emits.

use chrono::{DateTime, Utc};
use serde::Deserialize;

/// `(value_kw, reading's own timestamp)`: the shared vocabulary for a real-measurement reading across
/// the transport, the translation (this file), the VEN's port and its resolution.
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

#[derive(Debug, Deserialize)]
struct DefaultMeasurementMessage {
    power_kw: f64,
    ts: DateTime<Utc>,
}

fn parse_default_message(payload: &[u8]) -> Result<MeasurementReading, String> {
    let msg: DefaultMeasurementMessage =
        serde_json::from_slice(payload).map_err(|e| format!("parse error: {e}"))?;
    if !msg.power_kw.is_finite() || msg.power_kw.abs() > 1_000_000.0 {
        return Err(format!("power_kw out of range: {}", msg.power_kw));
    }
    Ok((msg.power_kw, msg.ts))
}

/// Parse an inbound PV-measurement topic payload.
pub fn parse_pv_measurement(payload: &[u8]) -> Result<MeasurementReading, String> {
    parse_default_message(payload)
}

/// Parse an inbound baseline-load-measurement topic payload.
pub fn parse_base_load_measurement(payload: &[u8]) -> Result<MeasurementReading, String> {
    parse_default_message(payload)
}

#[cfg(test)]
mod tests {
    use super::*;

    const VALID: &str = r#"{"power_kw": 3.2, "ts": "2026-08-03T12:00:00Z"}"#;

    #[test]
    fn parse_pv_measurement_parses_valid_payload() {
        let (kw, ts) = parse_pv_measurement(VALID.as_bytes()).unwrap();
        assert_eq!(kw, 3.2);
        assert_eq!(ts.to_rfc3339(), "2026-08-03T12:00:00+00:00");
    }

    #[test]
    fn parse_base_load_measurement_parses_valid_payload() {
        let (kw, _ts) = parse_base_load_measurement(VALID.as_bytes()).unwrap();
        assert_eq!(kw, 3.2);
    }

    #[test]
    fn rejects_missing_required_field() {
        let bad = r#"{"power_kw": 3.2}"#;
        assert!(parse_pv_measurement(bad.as_bytes()).is_err());
    }

    #[test]
    fn rejects_non_finite_power() {
        let bad = r#"{"power_kw": 1e30, "ts": "2026-08-03T12:00:00Z"}"#;
        assert!(parse_pv_measurement(bad.as_bytes()).is_err());
    }

    #[test]
    fn rejects_malformed_json() {
        assert!(parse_pv_measurement(b"not json").is_err());
    }

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
