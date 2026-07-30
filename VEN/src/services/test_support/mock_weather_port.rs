use async_trait::async_trait;
use std::sync::Mutex;

use crate::controller::WeatherForecastPort;
use crate::entities::weather::WeatherForecast;

/// Test double for WeatherForecastPort. Seedable; mirrors `MockVtn`/`mock_solver_port`.
pub struct MockWeatherPort {
    forecast: Mutex<Option<WeatherForecast>>,
    alive: Mutex<bool>,
}

impl Default for MockWeatherPort {
    fn default() -> Self {
        Self {
            forecast: Mutex::new(None),
            alive: Mutex::new(true),
        }
    }
}

impl MockWeatherPort {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_forecast(self, forecast: WeatherForecast) -> Self {
        *self.forecast.lock().unwrap() = Some(forecast);
        self
    }

    pub fn set(&self, forecast: Option<WeatherForecast>) {
        *self.forecast.lock().unwrap() = forecast;
    }

    pub fn set_alive(&self, alive: bool) {
        *self.alive.lock().unwrap() = alive;
    }
}

#[async_trait]
impl WeatherForecastPort for MockWeatherPort {
    async fn latest(&self) -> Option<WeatherForecast> {
        self.forecast.lock().unwrap().clone()
    }

    fn is_alive(&self) -> bool {
        *self.alive.lock().unwrap()
    }
}

/// Shared adapter-contract assertion — callable against any
/// `WeatherForecastPort` implementation (mock now, real MQTT adapter once it
/// exists in Phase 6) to assert the trait's behavior is transport-independent:
/// a freshly constructed port with no data yet observed returns `None`.
#[cfg(test)]
pub async fn assert_returns_none_before_any_data(port: &dyn WeatherForecastPort) {
    assert!(
        port.latest().await.is_none(),
        "latest() must return None before any data has arrived"
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entities::weather::GeoPosition;
    use chrono::{TimeZone, Utc};

    fn sample_forecast() -> WeatherForecast {
        WeatherForecast {
            source_id: "srf_meteo".into(),
            location: GeoPosition {
                latitude_deg: 47.4491,
                longitude_deg: 7.8081,
            },
            fetched_at: Utc.with_ymd_and_hms(2026, 7, 19, 6, 0, 0).unwrap(),
            samples: vec![],
        }
    }

    #[tokio::test]
    async fn returns_none_before_any_data_arrives() {
        let mock = MockWeatherPort::new();
        assert_returns_none_before_any_data(&mock).await;
    }

    #[tokio::test]
    async fn returns_most_recently_set_value() {
        let mock = MockWeatherPort::new();
        mock.set(Some(sample_forecast()));
        let latest = mock.latest().await;
        assert_eq!(latest.unwrap().source_id, "srf_meteo");
    }

    #[tokio::test]
    async fn with_forecast_seeds_at_construction() {
        let mock = MockWeatherPort::new().with_forecast(sample_forecast());
        assert!(mock.latest().await.is_some());
    }

    #[test]
    fn defaults_to_alive() {
        let mock = MockWeatherPort::new();
        assert!(mock.is_alive());
    }

    #[test]
    fn set_alive_updates_is_alive() {
        let mock = MockWeatherPort::new();
        mock.set_alive(false);
        assert!(!mock.is_alive());
    }
}
