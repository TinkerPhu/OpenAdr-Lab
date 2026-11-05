//! What the forecaster does with one incoming message, one clock tick, and the retained status of another
//! producer: the routing between the broker and `engine`, with no broker in it.
//!
//! Owns: topic -> action, the status that follows every recompute, and the single-producer decision.
//! Reads: what is handed in. Must never: read a clock or open a socket.

use chrono::{DateTime, Duration, Utc};
use lab_forecast::feed_topics::{measurement_topic, weather_forecast_topic};
use lab_forecast::forecast_payload::ForecastStatus;
use lab_forecast::measurement_wire::parse_pv_measurement;
use lab_forecast::weather_wire::parse_forecast_message;

use crate::engine::{Engine, Publication};

/// The input topics the forecaster listens on.
#[derive(Debug, Clone, PartialEq)]
pub struct FeedTopics {
    pub weather_forecast: String,
    /// Only when some array's measurement is switched on in the profile.
    pub pv_measurement: Option<String>,
}

impl FeedTopics {
    pub fn new(root: &str, site: &str, any_array_measured: bool) -> Self {
        Self {
            weather_forecast: weather_forecast_topic(root, site),
            pv_measurement: any_array_measured.then(|| measurement_topic(root, site, "pv")),
        }
    }
}

/// What came of one message.
#[derive(Debug, Default, PartialEq)]
pub struct Handled {
    /// Retained messages to publish now.
    pub publications: Vec<Publication>,
    /// A message that was refused, for the log.
    pub rejected: Option<String>,
}

/// Route one incoming message. A weather forecast triggers a recompute and the status; a measurement is
/// only remembered (it is read at the next recompute); anything else is ignored.
pub fn handle_message(
    engine: &mut Engine,
    feeds: &FeedTopics,
    topic: &str,
    payload: &[u8],
    now: DateTime<Utc>,
) -> Handled {
    if topic == feeds.weather_forecast {
        return match parse_forecast_message(payload) {
            Ok(forecast) => {
                engine.on_weather(forecast);
                Handled {
                    publications: handle_tick(engine, now),
                    rejected: None,
                }
            }
            Err(reason) => rejected("weather forecast", &reason),
        };
    }
    if feeds.pv_measurement.as_deref() == Some(topic) {
        return match parse_pv_measurement(payload) {
            Ok(reading) => {
                engine.on_pv_measurement(reading);
                Handled::default()
            }
            Err(reason) => rejected("PV measurement", &reason),
        };
    }
    Handled::default()
}

fn rejected(what: &str, reason: &str) -> Handled {
    Handled {
        publications: Vec::new(),
        rejected: Some(format!("{what} rejected: {reason}")),
    }
}

/// The periodic recompute: the series, then the status that describes it.
pub fn handle_tick(engine: &mut Engine, now: DateTime<Utc>) -> Vec<Publication> {
    let mut publications = engine.recompute(now);
    publications.push(engine.status(now));
    publications
}

/// Refuse to start while another producer's retained status is live. `retained` is the payload found on
/// the status topic, if any; a status that cannot be read, is offline, or is older than `limit` does not
/// count, and neither does one of our own id (a restart).
pub fn check_single_producer(
    retained: Option<&[u8]>,
    own_producer_id: &str,
    now: DateTime<Utc>,
    limit: Duration,
) -> Result<(), String> {
    let Some(status) = retained.and_then(|bytes| ForecastStatus::parse(bytes).ok()) else {
        return Ok(());
    };
    if status.producer_id != own_producer_id && status.is_live(now, limit) {
        return Err(format!(
            "another producer ('{}') is live on this site (state {:?}, since {})",
            status.producer_id, status.state, status.generated_at
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::EngineSettings;
    use chrono::TimeZone;
    use lab_forecast::forecast_payload::{ForecastState, ForecastTopics, SnowState};
    use lab_forecast::pv_profile::pv_arrays_from_yaml;
    use lab_forecast::weather::{GeoPosition, WeatherForecast, WeatherForecastSample};

    const NOW: fn() -> DateTime<Utc> = || Utc.with_ymd_and_hms(2026, 6, 21, 12, 0, 0).unwrap();

    fn engine(measured: bool) -> Engine {
        let yaml = format!(
            "assets:\n  - type: pv\n    id: pv\n    rated_kw: 10.0\n    weather:\n      latitude_deg: 47.4491\n      longitude_deg: 7.8081\n      tilt_deg: 30.0\n      azimuth_deg: 180.0\n      rated_kwp: 10.0\n    measurement: {{enabled: {measured}}}\n"
        );
        Engine::new(EngineSettings {
            topics: ForecastTopics::new("openadr-lab", "zunzgen").unwrap(),
            arrays: pv_arrays_from_yaml(&yaml).unwrap(),
            producer_id: "fc-a".into(),
            model_version: "pv-weather-1".into(),
            freshness: Duration::minutes(90),
        })
    }

    fn feeds() -> FeedTopics {
        FeedTopics::new("openadr-lab", "zunzgen", true)
    }

    fn weather_message(fetched_at: DateTime<Utc>) -> Vec<u8> {
        let first = Utc.with_ymd_and_hms(2026, 6, 21, 0, 0, 0).unwrap();
        let forecast = WeatherForecast {
            source_id: "test".into(),
            location: GeoPosition {
                latitude_deg: 47.4491,
                longitude_deg: 7.8081,
            },
            fetched_at,
            samples: (0..24)
                .map(|i| WeatherForecastSample {
                    valid_at: first + Duration::hours(i),
                    age_h: i as u32,
                    temperature_c: 20.0,
                    ghi_w_m2: 600.0,
                    wind_speed_kmh: None,
                    rain_prob_pct: None,
                    new_snowfall_cm: None,
                    sky_condition: None,
                    irradiance_variability: Some(0.0),
                })
                .collect(),
        };
        serde_json::to_vec(&forecast).unwrap()
    }

    #[test]
    fn feed_topics_listen_for_the_pv_measurement_only_when_one_is_configured() {
        assert_eq!(
            FeedTopics::new("openadr-lab", "zunzgen", true),
            FeedTopics {
                weather_forecast: "openadr-lab/weather/zunzgen/forecast".into(),
                pv_measurement: Some("openadr-lab/measurement/zunzgen/pv".into()),
            }
        );
        assert_eq!(
            FeedTopics::new("openadr-lab", "zunzgen", false).pv_measurement,
            None
        );
    }

    #[test]
    fn a_weather_message_publishes_the_series_and_then_the_status() {
        let mut e = engine(false);
        let h = handle_message(
            &mut e,
            &feeds(),
            "openadr-lab/weather/zunzgen/forecast",
            &weather_message(NOW() - Duration::minutes(5)),
            NOW(),
        );
        let topics: Vec<&str> = h.publications.iter().map(|p| p.topic.as_str()).collect();
        assert_eq!(
            topics,
            [
                "openadr-lab/forecast/zunzgen/pv/pv/power_kw",
                "openadr-lab/forecast/zunzgen/pv/total/power_kw",
                "openadr-lab/forecast/zunzgen/status",
            ]
        );
        assert_eq!(h.rejected, None);
    }

    #[test]
    fn a_malformed_weather_message_is_rejected_and_publishes_nothing() {
        let mut e = engine(false);
        let h = handle_message(
            &mut e,
            &feeds(),
            "openadr-lab/weather/zunzgen/forecast",
            b"not json",
            NOW(),
        );
        assert!(h.publications.is_empty());
        assert!(h.rejected.unwrap().contains("parse error"));
        assert_eq!(e.state(), ForecastState::ColdStart);
    }

    #[test]
    fn a_measurement_is_remembered_and_publishes_nothing_until_the_next_recompute() {
        let mut e = engine(true);
        let h = handle_message(
            &mut e,
            &feeds(),
            "openadr-lab/measurement/zunzgen/pv",
            format!(
                r#"{{"power_kw": 2.5, "ts": "{}"}}"#,
                (NOW() - Duration::seconds(20)).to_rfc3339()
            )
            .as_bytes(),
            NOW(),
        );
        assert!(h.publications.is_empty());
        assert_eq!(h.rejected, None);
        let s = ForecastStatus::parse(&e.status(NOW()).payload).unwrap();
        assert_eq!(s.input_age_s["pv_measurement"], Some(20));
    }

    #[test]
    fn a_malformed_measurement_is_rejected() {
        let mut e = engine(true);
        let h = handle_message(
            &mut e,
            &feeds(),
            "openadr-lab/measurement/zunzgen/pv",
            br#"{"power_kw": "high"}"#,
            NOW(),
        );
        assert!(h.rejected.is_some());
    }

    #[test]
    fn a_message_on_another_topic_is_ignored() {
        let mut e = engine(false);
        let h = handle_message(&mut e, &feeds(), "openadr-lab/other", b"{}", NOW());
        assert_eq!(h, Handled::default());
    }

    #[test]
    fn a_tick_without_weather_publishes_only_the_cold_start_status() {
        let mut e = engine(false);
        let pubs = handle_tick(&mut e, NOW());
        assert_eq!(pubs.len(), 1);
        let s = ForecastStatus::parse(&pubs[0].payload).unwrap();
        assert_eq!(s.state, ForecastState::ColdStart);
        assert_eq!(s.snow_state["pv"], SnowState::Unobserved);
    }

    #[test]
    fn a_tick_with_weather_republishes_the_series_and_the_status() {
        let mut e = engine(false);
        handle_message(
            &mut e,
            &feeds(),
            "openadr-lab/weather/zunzgen/forecast",
            &weather_message(NOW() - Duration::minutes(5)),
            NOW(),
        );
        let later = NOW() + Duration::minutes(15);
        let pubs = handle_tick(&mut e, later);
        assert_eq!(pubs.len(), 3);
        assert_eq!(
            ForecastStatus::parse(&pubs[2].payload)
                .unwrap()
                .generated_at,
            later
        );
    }

    fn status_bytes(producer: &str, state: ForecastState, at: DateTime<Utc>) -> Vec<u8> {
        let mut s = ForecastStatus::offline(producer, at);
        s.state = state;
        serde_json::to_vec(&s).unwrap()
    }

    #[test]
    fn the_guard_passes_when_nothing_is_retained() {
        assert!(check_single_producer(None, "fc-a", NOW(), Duration::minutes(90)).is_ok());
    }

    #[test]
    fn the_guard_refuses_a_live_status_of_another_producer() {
        let other = status_bytes("fc-b", ForecastState::Ok, NOW() - Duration::minutes(5));
        let err =
            check_single_producer(Some(&other), "fc-a", NOW(), Duration::minutes(90)).unwrap_err();
        assert!(err.contains("fc-b"), "{err}");
    }

    #[test]
    fn the_guard_passes_for_an_offline_a_stale_or_our_own_status() {
        let limit = Duration::minutes(90);
        let offline = status_bytes("fc-b", ForecastState::Offline, NOW());
        let stale = status_bytes("fc-b", ForecastState::Ok, NOW() - Duration::minutes(91));
        let own = status_bytes("fc-a", ForecastState::Ok, NOW());
        for retained in [offline, stale, own] {
            assert!(check_single_producer(Some(&retained), "fc-a", NOW(), limit).is_ok());
        }
    }

    #[test]
    fn the_guard_passes_for_a_retained_message_it_cannot_read() {
        assert!(check_single_producer(Some(b"{{"), "fc-a", NOW(), Duration::minutes(90)).is_ok());
    }
}
