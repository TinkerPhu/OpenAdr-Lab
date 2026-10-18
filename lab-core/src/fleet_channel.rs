//! The lab's fleet channel on `lab-mqtt`: `<root>/fleet/<venName>/<leaf>`.
//!
//! The VEN publishes its leaves and reads its commands; the BFF reads every leaf and publishes
//! the commands. Both sides used to spell the topic shape, the leaf names and the broker
//! settings for themselves; they live here once so they cannot drift apart. Message bodies the
//! VEN owns (telemetry, plan, objective setting) stay the VEN's types, passed through by the
//! BFF unchanged (`dto`). Only a body both sides build or read field by field is declared here,
//! which today is the objective command.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Every leaf of the channel, by name.
pub mod leaf {
    /// Live site state (QoS 0, retained).
    pub const TELEMETRY: &str = "telemetry";
    /// Controller decisions (QoS 1, not retained).
    pub const TRACE: &str = "trace";
    /// `online` / `offline`; the broker sends `offline` as the VEN's last will.
    pub const STATUS: &str = "status";
    /// The objective in force and its source: the VEN's `GET /plan/objective` body (retained).
    pub const OBJECTIVE: &str = "objective";
    /// The adopted plan: the VEN's `GET /plan` body (retained).
    pub const PLAN: &str = "plan";
    /// A request to change the objective, an [`ObjectiveCommand`] (retained); an empty payload
    /// withdraws it.
    pub const COMMAND_OBJECTIVE: &str = "command/objective";
}

/// The largest MQTT packet a fleet client sends or accepts [bytes], set on both ends' clients.
///
/// rumqttc's default is 10 KiB each way, and a VEN's `plan` leaf is larger (a 24 h horizon of
/// slots, each with its site flows, tariffs and every asset's planned kW, runs to tens of kB). Above the limit the sender
/// fails the publish inside its event loop and reconnects, re-sends the leaf on the new
/// connection and fails again; the receiver drops the connection that carries every VEN.
/// mosquitto's own default is far above this.
pub const MAX_PACKET_BYTES: usize = 4 * 1024 * 1024;

/// `<root>/fleet/<ven>`: everything one VEN publishes and receives lives under it.
pub fn ven_topic_root(root: &str, ven: &str) -> String {
    format!("{root}/fleet/{ven}")
}

/// `<root>/fleet/<ven>/<leaf>`.
pub fn ven_topic(root: &str, ven: &str, leaf: &str) -> String {
    format!("{}/{leaf}", ven_topic_root(root, ven))
}

/// Every leaf of every VEN, for a subscriber that watches the whole fleet.
pub fn fleet_subscription(root: &str) -> String {
    format!("{root}/fleet/+/#")
}

/// Split `<root>/fleet/<ven>/<leaf>` into the VEN and the leaf (which may hold a `/`, as
/// `command/objective` does).
///
/// `None` for anything not shaped like a fleet topic, so a stray publish on a neighbouring
/// topic cannot invent a VEN.
pub fn parse_topic<'a>(root: &str, topic: &'a str) -> Option<(&'a str, &'a str)> {
    let prefix = format!("{root}/fleet/");
    let rest = topic.strip_prefix(&prefix)?;
    let mut parts = rest.splitn(2, '/');
    let ven = parts.next().filter(|v| !v.is_empty())?;
    let leaf = parts.next().filter(|v| !v.is_empty())?;
    Some((ven, leaf))
}

/// The body of `command/objective`: the VEN's `PUT /plan/objective` body plus when it was
/// issued.
///
/// `O` is the objective's type on each side: the VEN reads its own objective enum, so an
/// unknown name is a parse error there, while the BFF carries the name it validated against the
/// VEN's `available_objectives`. `requested_at` identifies the command: the broker redelivers a
/// retained command after every reconnect, and the VEN applies each `requested_at` once.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ObjectiveCommand<O> {
    pub objective: O,
    pub requested_at: DateTime<Utc>,
}

/// Where the lab broker is and how to log in, read the same way by every client.
#[derive(Clone, Debug, PartialEq)]
pub struct BrokerConfig {
    pub host: String,
    pub port: u16,
    pub username: Option<String>,
    pub password: Option<String>,
    /// The topic root, `openadr-lab` unless `FLEET_MQTT_ROOT` says otherwise.
    pub root: String,
}

impl BrokerConfig {
    /// Read from the process environment; see [`BrokerConfig::from_vars`].
    pub fn from_env() -> Option<Self> {
        Self::from_vars(|k| std::env::var(k).ok())
    }

    /// `FLEET_MQTT_HOST` gates the channel: absent or empty means "not part of a monitored
    /// fleet", a normal deployment. The port defaults to **1884**, the lab broker (`:1883` is
    /// always the house broker). Empty credentials mean anonymous: the test broker's compose
    /// file passes `""` rather than omitting them.
    ///
    /// Takes any source of variables so tests need not touch process state.
    pub fn from_vars(get: impl Fn(&str) -> Option<String>) -> Option<Self> {
        let non_empty = |k: &str| get(k).filter(|v| !v.is_empty());
        Some(Self {
            host: non_empty("FLEET_MQTT_HOST")?,
            port: get("FLEET_MQTT_PORT")
                .and_then(|v| v.parse().ok())
                .unwrap_or(1884),
            username: non_empty("FLEET_MQTT_USERNAME"),
            password: non_empty("FLEET_MQTT_PASSWORD"),
            root: get("FLEET_MQTT_ROOT").unwrap_or_else(|| "openadr-lab".into()),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn vars(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let map: HashMap<String, String> = pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        move |k: &str| map.get(k).cloned()
    }

    #[test]
    fn ven_topic_and_parse_topic_are_inverses() {
        for l in [
            leaf::TELEMETRY,
            leaf::TRACE,
            leaf::STATUS,
            leaf::OBJECTIVE,
            leaf::PLAN,
            leaf::COMMAND_OBJECTIVE,
        ] {
            let topic = ven_topic("openadr-lab", "ven-7", l);
            assert_eq!(parse_topic("openadr-lab", &topic), Some(("ven-7", l)));
        }
        assert_eq!(
            ven_topic("openadr-lab", "ven-7", leaf::COMMAND_OBJECTIVE),
            "openadr-lab/fleet/ven-7/command/objective"
        );
    }

    /// A stray publish on a neighbouring topic must not invent a VEN.
    #[test]
    fn parse_topic_rejects_anything_not_shaped_like_a_fleet_topic() {
        for topic in [
            "openadr-lab/measurement/ven-7/pv",
            "openadr-lab/fleet/ven-7",
            "openadr-lab/fleet//telemetry",
            "other-root/fleet/ven-7/telemetry",
            "openadr-lab/fleet/",
        ] {
            assert!(
                parse_topic("openadr-lab", topic).is_none(),
                "{topic} should not parse as a fleet topic"
            );
        }
    }

    #[test]
    fn fleet_subscription_covers_every_leaf_of_every_ven() {
        assert_eq!(fleet_subscription("openadr-lab"), "openadr-lab/fleet/+/#");
    }

    #[test]
    fn objective_command_round_trips_with_its_wire_field_names() {
        let cmd = ObjectiveCommand {
            objective: "opportunistic".to_string(),
            requested_at: DateTime::from_timestamp(1_760_000_000, 0).unwrap(),
        };
        let wire = serde_json::to_value(&cmd).unwrap();
        assert_eq!(
            wire,
            serde_json::json!({"objective": "opportunistic", "requested_at": "2025-10-09T08:53:20Z"})
        );
        let back: ObjectiveCommand<String> = serde_json::from_value(wire).unwrap();
        assert_eq!(back, cmd);
    }

    #[test]
    fn broker_config_is_absent_without_a_host() {
        assert!(BrokerConfig::from_vars(vars(&[])).is_none());
        assert!(BrokerConfig::from_vars(vars(&[("FLEET_MQTT_HOST", "")])).is_none());
    }

    #[test]
    fn broker_config_defaults_to_the_lab_broker_and_anonymous() {
        let c = BrokerConfig::from_vars(vars(&[
            ("FLEET_MQTT_HOST", "lab-mqtt"),
            ("FLEET_MQTT_USERNAME", ""),
            ("FLEET_MQTT_PASSWORD", ""),
        ]))
        .unwrap();
        assert_eq!(
            c,
            BrokerConfig {
                host: "lab-mqtt".into(),
                port: 1884,
                username: None,
                password: None,
                root: "openadr-lab".into(),
            }
        );
    }

    #[test]
    fn broker_config_carries_what_is_set() {
        let c = BrokerConfig::from_vars(vars(&[
            ("FLEET_MQTT_HOST", "lab-mqtt"),
            ("FLEET_MQTT_PORT", "1885"),
            ("FLEET_MQTT_USERNAME", "ven"),
            ("FLEET_MQTT_PASSWORD", "secret"),
            ("FLEET_MQTT_ROOT", "lab2"),
        ]))
        .unwrap();
        assert_eq!(
            (
                c.port,
                c.username.as_deref(),
                c.password.as_deref(),
                c.root.as_str()
            ),
            (1885, Some("ven"), Some("secret"), "lab2")
        );
    }
}
