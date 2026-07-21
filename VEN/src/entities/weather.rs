//! Weather forecast domain types — see `docs/plans/weather-forecast-plugin.md`
//! for the full design (transport, physics, wire contract). Pure data, no I/O,
//! no `crate::profile` import per the entities-layer rule. Fills in the
//! `ForecastSource::WeatherModel` / `ExternalDataSourceType::Weather` /
//! `Irradiation` placeholders already sketched in `design_vocabulary.rs`.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Generic, supplier-agnostic sky-condition vocabulary. Each MQTT adapter
/// translates its own supplier's icon/description code into this shared set
/// — the translation table lives with the adapter, never here.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SkyCondition {
    Clear,
    MostlyClear,
    PartlyCloudy, // "changing" — pair with a high irradiance_variability score
    Overcast,
    Fog,
    Rain,
    Sleet,
    Snow,
    Thunderstorm,
    Unknown, // adapter couldn't map its supplier's code — never silently guess
}

/// One hour of forecast, as delivered by whichever supplier is configured.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WeatherForecastSample {
    /// Hour this sample is *for* (not when it was fetched).
    pub valid_at: DateTime<Utc>,
    /// 0 = most recent actual/"fact", 1..=N = hours ahead.
    pub age_h: u32,
    pub temperature_c: f64,
    /// Global Horizontal Irradiance, as reported by the supplier — NOT yet
    /// projected onto any panel plane. Projection is VEN's job (`entities::solar`).
    pub ghi_w_m2: f64,
    pub wind_speed_kmh: Option<f64>,
    pub rain_prob_pct: Option<f64>,
    /// New snowfall this hour. Drives the snow-cover state model.
    pub new_snowfall_cm: Option<f64>,
    /// Supplier-specific icon/description, translated by the adapter into
    /// this shared vocabulary.
    pub sky_condition: Option<SkyCondition>,
    /// 0.0 = sky was uniform the whole hour (fully clear OR fully
    /// overcast), 1.0 = maximally broken/alternating sky within the hour.
    pub irradiance_variability: Option<f64>,
}

/// A full forecast pull: one fetch, many hourly samples, tied to a location.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WeatherForecast {
    pub source_id: String, // e.g. "srf_meteo"
    pub location: GeoPosition,
    pub fetched_at: DateTime<Utc>,
    /// Ordered by valid_at ascending. No fixed length promised.
    pub samples: Vec<WeatherForecastSample>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct GeoPosition {
    pub latitude_deg: f64,
    pub longitude_deg: f64,
}

impl WeatherForecast {
    /// Whether this forecast is still trustworthy for planning purposes,
    /// per the staleness policy in `docs/plans/weather-forecast-plugin.md`.
    /// Default threshold (2h) is a starting value, not a measured one.
    pub fn is_fresh(&self, now: DateTime<Utc>, max_age: chrono::Duration) -> bool {
        now.signed_duration_since(self.fetched_at) <= max_age
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn sample(valid_at: DateTime<Utc>, age_h: u32) -> WeatherForecastSample {
        WeatherForecastSample {
            valid_at,
            age_h,
            temperature_c: 16.0,
            ghi_w_m2: 97.0,
            wind_speed_kmh: Some(4.0),
            rain_prob_pct: Some(14.0),
            new_snowfall_cm: None,
            sky_condition: Some(SkyCondition::PartlyCloudy),
            irradiance_variability: Some(0.6),
        }
    }

    #[test]
    fn weather_forecast_sample_round_trips_through_json() {
        let s = sample(Utc.with_ymd_and_hms(2026, 7, 19, 6, 0, 0).unwrap(), 1);
        let json = serde_json::to_string(&s).unwrap();
        let back: WeatherForecastSample = serde_json::from_str(&json).unwrap();
        assert_eq!(s, back);
    }

    #[test]
    fn weather_forecast_round_trips_through_json_matching_wire_example() {
        // Mirrors the wire contract's example message in
        // docs/plans/weather-forecast-plugin.md.
        let json = r#"{
            "source_id": "srf_meteo",
            "location": { "latitude_deg": 47.4491, "longitude_deg": 7.8081 },
            "fetched_at": "2026-07-19T05:54:48Z",
            "samples": [
                {
                    "valid_at": "2026-07-19T05:00:00Z",
                    "age_h": 0,
                    "temperature_c": 15.0,
                    "ghi_w_m2": 0.0,
                    "wind_speed_kmh": 4.0,
                    "rain_prob_pct": 10.0,
                    "sky_condition": "clear",
                    "irradiance_variability": 0.0
                },
                {
                    "valid_at": "2026-07-19T06:00:00Z",
                    "age_h": 1,
                    "temperature_c": 16.0,
                    "ghi_w_m2": 97.0,
                    "wind_speed_kmh": 4.0,
                    "rain_prob_pct": 14.0,
                    "sky_condition": "partly_cloudy",
                    "irradiance_variability": 0.6
                }
            ]
        }"#;
        let forecast: WeatherForecast = serde_json::from_str(json).unwrap();
        assert_eq!(forecast.source_id, "srf_meteo");
        assert_eq!(forecast.samples.len(), 2);
        assert_eq!(
            forecast.samples[1].sky_condition,
            Some(SkyCondition::PartlyCloudy)
        );
        assert_eq!(forecast.samples[1].irradiance_variability, Some(0.6));
        // Round-trip back to JSON and re-parse for full fidelity.
        let back: WeatherForecast =
            serde_json::from_str(&serde_json::to_string(&forecast).unwrap()).unwrap();
        assert_eq!(forecast, back);
    }

    #[test]
    fn sky_condition_variants_are_distinct() {
        assert_ne!(SkyCondition::Clear, SkyCondition::Overcast);
        assert_eq!(SkyCondition::PartlyCloudy, SkyCondition::PartlyCloudy);
    }

    #[test]
    fn sky_condition_serializes_to_documented_wire_strings() {
        assert_eq!(
            serde_json::to_string(&SkyCondition::PartlyCloudy).unwrap(),
            "\"partly_cloudy\""
        );
        assert_eq!(
            serde_json::to_string(&SkyCondition::MostlyClear).unwrap(),
            "\"mostly_clear\""
        );
    }

    #[test]
    fn is_fresh_within_threshold() {
        let now = Utc.with_ymd_and_hms(2026, 7, 19, 6, 5, 0).unwrap();
        let fetched_at = Utc.with_ymd_and_hms(2026, 7, 19, 5, 54, 48).unwrap();
        let forecast = WeatherForecast {
            source_id: "srf_meteo".into(),
            location: GeoPosition {
                latitude_deg: 47.4491,
                longitude_deg: 7.8081,
            },
            fetched_at,
            samples: vec![sample(fetched_at, 0)],
        };
        assert!(forecast.is_fresh(now, chrono::Duration::hours(2)));
    }

    #[test]
    fn is_fresh_past_threshold() {
        let fetched_at = Utc.with_ymd_and_hms(2026, 7, 19, 3, 0, 0).unwrap();
        let now = Utc.with_ymd_and_hms(2026, 7, 19, 6, 0, 0).unwrap(); // 3h later
        let forecast = WeatherForecast {
            source_id: "srf_meteo".into(),
            location: GeoPosition {
                latitude_deg: 47.4491,
                longitude_deg: 7.8081,
            },
            fetched_at,
            samples: vec![sample(fetched_at, 0)],
        };
        assert!(!forecast.is_fresh(now, chrono::Duration::hours(2)));
    }
}
