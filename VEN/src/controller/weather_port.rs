// WeatherForecastPort trait — the in-process seam between weather-consuming
// code (services, planner) and any weather data source (currently: an MQTT
// adapter, see `crate::weather`). Same ring as VtnPort/SolverPort/SimulatorPort.
// See docs/plans/weather-forecast-plugin.md for the full architecture.

use async_trait::async_trait;

use crate::entities::weather::WeatherForecast;

#[async_trait]
pub trait WeatherForecastPort: Send + Sync {
    /// Latest known forecast, however it arrived. Never blocks on network
    /// I/O — reads a cached snapshot kept fresh by a background task.
    /// `None` when no forecast has ever been received (or no weather
    /// source is configured at all).
    async fn latest(&self) -> Option<WeatherForecast>;
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn noop_weather_port_always_returns_none() {
        let port = NoopWeatherPort;
        assert!(port.latest().await.is_none());
    }
}
