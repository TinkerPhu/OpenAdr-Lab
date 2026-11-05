//! What the container is told through its environment.
//!
//! Owns: the names and defaults of the settings. Reads: a lookup function (the process environment in
//! `main`, a map in tests). Must never: read the environment itself, so a test needs no global state.

use std::time::Duration;

/// Everything the container needs besides the VEN profile file.
#[derive(Debug, Clone, PartialEq)]
pub struct Config {
    pub mqtt_host: String,
    pub mqtt_port: u16,
    pub mqtt_username: Option<String>,
    pub mqtt_password: Option<String>,
    /// `<root>` of every topic: weather and measurement in, forecast out.
    pub root: String,
    pub site: String,
    /// The VEN profile YAML that declares the PV arrays.
    pub profile_path: String,
    /// Who publishes: a second container with another id is refused while this one is live.
    pub producer_id: String,
    /// `valid_until - generated_at` of every series, and the oldest weather forecast still used.
    pub freshness: chrono::Duration,
    pub model_version: String,
    /// How often a recompute happens besides every new weather forecast.
    pub recompute_every: Duration,
}

impl Config {
    /// Read the settings through `get`; the error names the variable that is missing or unusable.
    pub fn from_lookup(get: &dyn Fn(&str) -> Option<String>) -> Result<Self, String> {
        let required = |name: &str| {
            get(name)
                .filter(|v| !v.is_empty())
                .ok_or_else(|| format!("{name} is not set"))
        };
        let number =
            |name: &str, default: u64| -> Result<u64, String> {
                match get(name) {
                    None => Ok(default),
                    Some(v) => v.parse::<u64>().ok().filter(|n| *n > 0).ok_or_else(|| {
                        format!("{name} must be a positive whole number, got '{v}'")
                    }),
                }
            };
        let port = number("FORECASTER_MQTT_PORT", 1883)?;
        Ok(Self {
            mqtt_host: required("FORECASTER_MQTT_HOST")?,
            mqtt_port: u16::try_from(port)
                .map_err(|_| format!("FORECASTER_MQTT_PORT must be a port, got '{port}'"))?,
            mqtt_username: get("FORECASTER_MQTT_USERNAME"),
            mqtt_password: get("FORECASTER_MQTT_PASSWORD"),
            root: get("FORECASTER_MQTT_ROOT").unwrap_or_else(|| "openadr-lab".into()),
            site: required("FORECASTER_SITE_ID")?,
            profile_path: required("FORECASTER_PROFILE_PATH")?,
            producer_id: get("FORECASTER_PRODUCER_ID")
                .or_else(|| get("HOSTNAME"))
                .unwrap_or_else(|| "lab-forecaster".into()),
            freshness: chrono::Duration::minutes(
                i64::try_from(number("FORECASTER_FRESHNESS_MINUTES", 90)?)
                    .map_err(|_| "FORECASTER_FRESHNESS_MINUTES is too large".to_string())?,
            ),
            model_version: get("FORECASTER_MODEL_VERSION").unwrap_or_else(|| "pv-weather-1".into()),
            recompute_every: Duration::from_secs(number("FORECASTER_RECOMPUTE_SECONDS", 900)?),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn lookup(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let map: HashMap<String, String> = pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        move |k| map.get(k).cloned()
    }

    const MINIMAL: [(&str, &str); 3] = [
        ("FORECASTER_MQTT_HOST", "broker"),
        ("FORECASTER_SITE_ID", "zunzgen"),
        ("FORECASTER_PROFILE_PATH", "/profiles/ven-1.yaml"),
    ];

    #[test]
    fn from_lookup_applies_the_defaults_to_a_minimal_environment() {
        let c = Config::from_lookup(&lookup(&MINIMAL)).unwrap();
        assert_eq!(c.mqtt_host, "broker");
        assert_eq!(c.mqtt_port, 1883);
        assert_eq!(c.root, "openadr-lab");
        assert_eq!(c.site, "zunzgen");
        assert_eq!(c.profile_path, "/profiles/ven-1.yaml");
        assert_eq!(c.freshness, chrono::Duration::minutes(90));
        assert_eq!(c.model_version, "pv-weather-1");
        assert_eq!(c.recompute_every, Duration::from_secs(900));
        assert_eq!(c.mqtt_username, None);
    }

    #[test]
    fn from_lookup_reads_every_override() {
        let mut env = MINIMAL.to_vec();
        env.extend([
            ("FORECASTER_MQTT_PORT", "1884"),
            ("FORECASTER_MQTT_USERNAME", "fc"),
            ("FORECASTER_MQTT_PASSWORD", "secret"),
            ("FORECASTER_MQTT_ROOT", "lab2"),
            ("FORECASTER_PRODUCER_ID", "fc-a"),
            ("FORECASTER_FRESHNESS_MINUTES", "120"),
            ("FORECASTER_MODEL_VERSION", "pv-weather-2"),
            ("FORECASTER_RECOMPUTE_SECONDS", "600"),
        ]);
        let c = Config::from_lookup(&lookup(&env)).unwrap();
        assert_eq!(c.mqtt_port, 1884);
        assert_eq!(c.mqtt_username.as_deref(), Some("fc"));
        assert_eq!(c.mqtt_password.as_deref(), Some("secret"));
        assert_eq!(c.root, "lab2");
        assert_eq!(c.producer_id, "fc-a");
        assert_eq!(c.freshness, chrono::Duration::minutes(120));
        assert_eq!(c.model_version, "pv-weather-2");
        assert_eq!(c.recompute_every, Duration::from_secs(600));
    }

    #[test]
    fn the_producer_id_falls_back_to_the_hostname_then_to_a_fixed_name() {
        let mut env = MINIMAL.to_vec();
        env.push(("HOSTNAME", "3f2a1b"));
        assert_eq!(
            Config::from_lookup(&lookup(&env)).unwrap().producer_id,
            "3f2a1b"
        );
        assert_eq!(
            Config::from_lookup(&lookup(&MINIMAL)).unwrap().producer_id,
            "lab-forecaster"
        );
    }

    #[test]
    fn from_lookup_names_a_missing_required_variable() {
        for missing in [
            "FORECASTER_MQTT_HOST",
            "FORECASTER_SITE_ID",
            "FORECASTER_PROFILE_PATH",
        ] {
            let env: Vec<_> = MINIMAL
                .iter()
                .filter(|(k, _)| *k != missing)
                .copied()
                .collect();
            let err = Config::from_lookup(&lookup(&env)).unwrap_err();
            assert!(err.contains(missing), "{err}");
        }
    }

    #[test]
    fn from_lookup_names_an_unusable_number() {
        for (var, bad) in [
            ("FORECASTER_MQTT_PORT", "http"),
            ("FORECASTER_FRESHNESS_MINUTES", "0"),
            ("FORECASTER_FRESHNESS_MINUTES", "soon"),
            ("FORECASTER_RECOMPUTE_SECONDS", "0"),
        ] {
            let mut env = MINIMAL.to_vec();
            env.push((var, bad));
            let err = Config::from_lookup(&lookup(&env)).unwrap_err();
            assert!(err.contains(var), "{var}={bad}: {err}");
        }
    }
}
