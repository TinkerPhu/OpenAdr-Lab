// WeatherForecastPort trait — the in-process seam between weather-consuming
// code (services, planner) and any weather data source (currently: an MQTT
// adapter, see `crate::weather`). Same ring as VtnPort/SolverPort/SimulatorPort.
// See docs/architecture/weather_forecast.md for the full architecture.

use async_trait::async_trait;

use crate::entities::weather::WeatherForecast;

#[async_trait]
pub trait WeatherForecastPort: Send + Sync {
    /// Latest known forecast, however it arrived. Never blocks on network
    /// I/O — reads a cached snapshot kept fresh by a background task.
    /// `None` when no forecast has ever been received (or no weather
    /// source is configured at all).
    async fn latest(&self) -> Option<WeatherForecast>;

    /// Whether the configured source is currently considered alive (R-52):
    /// for the MQTT adapter, a status-topic heartbeat seen within 2x the
    /// documented interval. `false` when no source is configured at all.
    /// Synchronous — reads an in-memory flag, never touches the network.
    fn is_alive(&self) -> bool;
}

/// No-op adapter: always returns `None`. The composition root wires this in
/// when no weather MQTT broker is configured, so every weather-dependent
/// consumer transparently falls back to its pre-existing non-weather
/// behavior without any `Option<Arc<dyn WeatherForecastPort>>` threading.
pub struct NoopWeatherPort;

#[async_trait]
impl WeatherForecastPort for NoopWeatherPort {
    async fn latest(&self) -> Option<WeatherForecast> {
        None
    }

    fn is_alive(&self) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn noop_weather_port_always_returns_none() {
        let port = NoopWeatherPort;
        assert!(port.latest().await.is_none());
    }

    #[test]
    fn noop_weather_port_is_never_alive() {
        let port = NoopWeatherPort;
        assert!(!port.is_alive());
    }
}
