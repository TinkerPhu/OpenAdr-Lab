//! The weather feed's wire contract (`docs/architecture/weather_forecast.md`, "Wire contract"):
//! the status message and the parse + validate of a forecast payload. Pure; no broker.

use serde::{Deserialize, Serialize};

use crate::weather::WeatherForecast;

/// `WeatherStatusMessage` per the wire contract's Topic 2 schema.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct WeatherStatusMessage {
    pub schema_version: String,
    pub source_id: String,
    pub ts: chrono::DateTime<chrono::Utc>,
    pub status: WeatherStatus,
    pub last_successful_fetch_at: Option<chrono::DateTime<chrono::Utc>>,
    pub consecutive_failures: Option<u32>,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum WeatherStatus {
    Ok,
    Stale,
    Error,
    Offline,
}

/// Validate a parsed `WeatherForecast` against the wire contract's numeric
/// bounds. Serde already enforces the required-field set (non-`Option`
/// fields fail to deserialize if absent); this catches out-of-range values
/// serde's type system can't. Rejects the *entire* message on any violation.
fn validate_forecast(f: &WeatherForecast) -> Result<(), String> {
    if !(-90.0..=90.0).contains(&f.location.latitude_deg) {
        return Err(format!(
            "latitude_deg out of range: {}",
            f.location.latitude_deg
        ));
    }
    if !(-180.0..=180.0).contains(&f.location.longitude_deg) {
        return Err(format!(
            "longitude_deg out of range: {}",
            f.location.longitude_deg
        ));
    }
    if f.samples.is_empty() {
        return Err("samples must not be empty".to_string());
    }
    for s in &f.samples {
        if s.age_h > 240 {
            return Err(format!("age_h out of range: {}", s.age_h));
        }
        if !(-60.0..=60.0).contains(&s.temperature_c) {
            return Err(format!("temperature_c out of range: {}", s.temperature_c));
        }
        if !(0.0..=1500.0).contains(&s.ghi_w_m2) {
            return Err(format!("ghi_w_m2 out of range: {}", s.ghi_w_m2));
        }
        if let Some(w) = s.wind_speed_kmh {
            if !(0.0..=300.0).contains(&w) {
                return Err(format!("wind_speed_kmh out of range: {w}"));
            }
        }
        if let Some(r) = s.rain_prob_pct {
            if !(0.0..=100.0).contains(&r) {
                return Err(format!("rain_prob_pct out of range: {r}"));
            }
        }
        if let Some(sn) = s.new_snowfall_cm {
            if !(0.0..=200.0).contains(&sn) {
                return Err(format!("new_snowfall_cm out of range: {sn}"));
            }
        }
        if let Some(v) = s.irradiance_variability {
            if !(0.0..=1.0).contains(&v) {
                return Err(format!("irradiance_variability out of range: {v}"));
            }
        }
    }
    Ok(())
}

/// Parse+validate an inbound `forecast` topic payload. Standalone (no
/// `rumqttc` event-loop dependency) so it's unit-testable without a broker.
pub fn parse_forecast_message(payload: &[u8]) -> Result<WeatherForecast, String> {
    let forecast: WeatherForecast =
        serde_json::from_slice(payload).map_err(|e| format!("parse error: {e}"))?;
    validate_forecast(&forecast)?;
    Ok(forecast)
}

/// Parse an inbound `status` topic payload.
pub fn parse_status_message(payload: &[u8]) -> Result<WeatherStatusMessage, String> {
    serde_json::from_slice(payload).map_err(|e| format!("parse error: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const VALID_MESSAGE: &str = r#"{
        "schema_version": "1.0.0",
        "source_id": "srf_meteo",
        "location": { "latitude_deg": 46.9480, "longitude_deg": 7.4474 },
        "fetched_at": "2026-07-19T05:54:48Z",
        "samples": [
            { "valid_at": "2026-07-19T06:00:00Z", "age_h": 1, "temperature_c": 16.0, "ghi_w_m2": 97.0 }
        ]
    }"#;

    #[test]
    fn parses_valid_forecast_message() {
        let f = parse_forecast_message(VALID_MESSAGE.as_bytes()).unwrap();
        assert_eq!(f.source_id, "srf_meteo");
        assert_eq!(f.samples.len(), 1);
    }

    /// Schema 2.0.0 of the feed renames the sample fields to the catalogue names; both name sets
    /// are accepted so the producer can switch in place.
    #[test]
    fn parses_catalogue_field_names_like_the_old_ones() {
        let renamed = r#"{
            "schema_version": "2.0.0",
            "source_id": "srf_meteo",
            "location": { "latitude_deg": 46.9480, "longitude_deg": 7.4474 },
            "fetched_at": "2026-07-19T05:54:48Z",
            "samples": [
                { "valid_at": "2026-07-19T06:00:00Z", "age_h": 1, "temperature_C": 16.0,
                  "irradiance_Wm2": 97.0, "windspeed_kmh": 12.0, "rainprob_Perc": 30.0, "snow_cmh": 0.5 }
            ]
        }"#;
        let old = r#"{
            "schema_version": "1.0.0",
            "source_id": "srf_meteo",
            "location": { "latitude_deg": 46.9480, "longitude_deg": 7.4474 },
            "fetched_at": "2026-07-19T05:54:48Z",
            "samples": [
                { "valid_at": "2026-07-19T06:00:00Z", "age_h": 1, "temperature_c": 16.0,
                  "ghi_w_m2": 97.0, "wind_speed_kmh": 12.0, "rain_prob_pct": 30.0, "new_snowfall_cm": 0.5 }
            ]
        }"#;
        let new_names = parse_forecast_message(renamed.as_bytes()).unwrap();
        let old_names = parse_forecast_message(old.as_bytes()).unwrap();
        assert_eq!(new_names.samples, old_names.samples);
    }

    #[test]
    fn rejects_message_missing_required_field() {
        let missing_fetched_at = r#"{
            "schema_version": "1.0.0",
            "source_id": "srf_meteo",
            "location": { "latitude_deg": 46.9480, "longitude_deg": 7.4474 },
            "samples": []
        }"#;
        assert!(parse_forecast_message(missing_fetched_at.as_bytes()).is_err());
    }

    #[test]
    fn rejects_out_of_range_temperature() {
        let bad = r#"{
            "schema_version": "1.0.0",
            "source_id": "srf_meteo",
            "location": { "latitude_deg": 46.9480, "longitude_deg": 7.4474 },
            "fetched_at": "2026-07-19T05:54:48Z",
            "samples": [
                { "valid_at": "2026-07-19T06:00:00Z", "age_h": 1, "temperature_c": 999.0, "ghi_w_m2": 97.0 }
            ]
        }"#;
        assert!(parse_forecast_message(bad.as_bytes()).is_err());
    }

    #[test]
    fn rejects_out_of_range_ghi() {
        let bad = r#"{
            "schema_version": "1.0.0",
            "source_id": "srf_meteo",
            "location": { "latitude_deg": 46.9480, "longitude_deg": 7.4474 },
            "fetched_at": "2026-07-19T05:54:48Z",
            "samples": [
                { "valid_at": "2026-07-19T06:00:00Z", "age_h": 1, "temperature_c": 16.0, "ghi_w_m2": -5.0 }
            ]
        }"#;
        assert!(parse_forecast_message(bad.as_bytes()).is_err());
    }

    #[test]
    fn ignores_unknown_fields_forward_compatibly() {
        let with_extra = r#"{
            "schema_version": "1.0.0",
            "source_id": "srf_meteo",
            "location": { "latitude_deg": 46.9480, "longitude_deg": 7.4474 },
            "fetched_at": "2026-07-19T05:54:48Z",
            "samples": [
                { "valid_at": "2026-07-19T06:00:00Z", "age_h": 1, "temperature_c": 16.0, "ghi_w_m2": 97.0,
                  "some_future_field": "icing_risk_pct" }
            ],
            "a_totally_new_top_level_field": 42
        }"#;
        assert!(parse_forecast_message(with_extra.as_bytes()).is_ok());
    }

    #[test]
    fn parses_valid_status_message() {
        let json = r#"{ "schema_version": "1.0.0", "source_id": "srf_meteo",
            "ts": "2026-07-19T06:05:00Z", "status": "ok",
            "last_successful_fetch_at": "2026-07-19T05:54:48Z", "consecutive_failures": 0 }"#;
        let s = parse_status_message(json.as_bytes()).unwrap();
        assert_eq!(s.status, WeatherStatus::Ok);
    }

    #[test]
    fn rejects_status_message_missing_required_field() {
        let missing_status = r#"{ "schema_version": "1.0.0", "source_id": "srf_meteo",
            "ts": "2026-07-19T06:05:00Z" }"#;
        assert!(parse_status_message(missing_status.as_bytes()).is_err());
    }
}
