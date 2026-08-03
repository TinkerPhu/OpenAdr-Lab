// MeasurementPort trait — the in-process seam between measurement-consuming
// asset code (PV, baseline load) and any real-measurement data source
// (currently: an MQTT adapter, see `crate::measurement`). Generic at the
// transport level only: one trait shape reused for both signals, each with
// its own independent instance/connection. Same ring as WeatherForecastPort.

use async_trait::async_trait;

pub use crate::entities::measurement::MeasurementReading;

#[async_trait]
pub trait MeasurementPort: Send + Sync {
    /// Latest known reading, however it arrived. Never blocks on network
    /// I/O — reads a cached snapshot kept fresh by a background task. `None`
    /// when no reading has ever been received (or no source is configured
    /// at all).
    async fn latest_kw(&self) -> Option<MeasurementReading>;

    /// Whether the configured source is currently considered alive: a
    /// message seen within 2x the expected publish interval. `false` when no
    /// source is configured at all. Synchronous — reads an in-memory flag.
    fn is_alive(&self) -> bool;
}

/// No-op adapter: always returns `None`/`false`. The composition root wires
/// this in when no measurement MQTT broker is configured for a given signal,
/// so every consumer transparently falls back to its pre-existing
/// non-measurement behavior without any `Option<Arc<dyn MeasurementPort>>`
/// threading.
pub struct NoopMeasurementPort;

#[async_trait]
impl MeasurementPort for NoopMeasurementPort {
    async fn latest_kw(&self) -> Option<MeasurementReading> {
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
    async fn noop_measurement_port_always_returns_none() {
        let port = NoopMeasurementPort;
        assert!(port.latest_kw().await.is_none());
    }

    #[test]
    fn noop_measurement_port_is_never_alive() {
        let port = NoopMeasurementPort;
        assert!(!port.is_alive());
    }
}
