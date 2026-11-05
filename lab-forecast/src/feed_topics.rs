//! The names of the input topics a site publishes on: the weather forecast and its heartbeat, and the live
//! measurements. One definition for the VEN and the forecaster; the forecast topics are in `forecast_payload`.
//!
//! Owns: the topic layouts of `docs/architecture/weather_forecast.md` and `real_measurement_mqtt.md`.
//! Reads: root and site names handed in. Must never: validate them (the broker is the authority on what
//! it accepts) or touch a socket.

/// `<root>/weather/<site>/forecast`: the retained hourly weather forecast.
pub fn weather_forecast_topic(root: &str, site: &str) -> String {
    format!("{root}/weather/{site}/forecast")
}

/// `<root>/weather/<site>/status`: the weather source's heartbeat.
pub fn weather_status_topic(root: &str, site: &str) -> String {
    format!("{root}/weather/{site}/status")
}

/// `<root>/measurement/<site>/<signal>`: a live meter reading (`pv`, `base_load`).
pub fn measurement_topic(root: &str, site: &str, signal: &str) -> String {
    format!("{root}/measurement/{site}/{signal}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn weather_topics_follow_the_documented_layout() {
        assert_eq!(
            weather_forecast_topic("openadr-lab", "zunzgen"),
            "openadr-lab/weather/zunzgen/forecast"
        );
        assert_eq!(
            weather_status_topic("openadr-lab", "zunzgen"),
            "openadr-lab/weather/zunzgen/status"
        );
    }

    #[test]
    fn measurement_topic_names_the_signal() {
        assert_eq!(
            measurement_topic("openadr-lab", "zunzgen", "pv"),
            "openadr-lab/measurement/zunzgen/pv"
        );
        assert_eq!(
            measurement_topic("openadr-lab", "zunzgen", "base_load"),
            "openadr-lab/measurement/zunzgen/base_load"
        );
    }
}
