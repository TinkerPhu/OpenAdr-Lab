//! Wire-payload → `(value_kw, reading_at)` translation for real-measurement
//! MQTT feeds. **This is the one file a downstream deployer of this repo
//! needs to rewrite** to match their own meter/inverter/gateway's message
//! format — every other measurement-feed file (`measurement.rs`,
//! `controller/measurement_port.rs`) is device-agnostic transport plumbing.
//!
//! The shipped default expects `{"power_kw": f64, "ts": rfc3339}` — swap the
//! body of either function to parse whatever your own hardware/bridge emits.

use chrono::{DateTime, Utc};
use serde::Deserialize;

use crate::controller::MeasurementReading;

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
}
