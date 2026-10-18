//! Commanding a VEN's objective from the fleet (openspec `opportunistic-objective` D10).
//!
//! The BFF publishes the command retained on the VEN's `command/objective` leaf; the VEN applies
//! it through its own objective setter and reports the result on its `objective` leaf, which the
//! BFF reads back, so "commanded" and "reported" both come from the broker. This module decides
//! whether a command is valid (against the objectives the VEN itself declares) and what goes on
//! the wire; it never decides anything about the VEN's state.

use std::collections::BTreeMap;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use lab_core::fleet_channel::{leaf, ven_topic, ObjectiveCommand};

use crate::fleet::VenLiveState;

/// Publishes one retained message. A port, so routes are tested without a broker.
pub trait CommandPublisher: Send + Sync {
    fn publish_retained(&self, topic: String, payload: Vec<u8>) -> Result<(), String>;
}

/// The BFF's own client on the lab broker, the one the ingest loop polls.
pub struct MqttCommandPublisher(pub rumqttc::AsyncClient);

impl CommandPublisher for MqttCommandPublisher {
    /// QoS 1, retained, without waiting: a full request queue (broker away) is reported, not
    /// waited out.
    fn publish_retained(&self, topic: String, payload: Vec<u8>) -> Result<(), String> {
        self.0
            .try_publish(topic, rumqttc::QoS::AtLeastOnce, true, payload)
            .map_err(|e| e.to_string())
    }
}

/// Why a command was refused.
#[derive(Debug, Clone, PartialEq)]
pub enum CommandRefused {
    /// The fleet has never heard of this VEN.
    UnknownVen(String),
    /// The VEN has not published its objective, so what it accepts is unknown.
    ObjectivesUnknown(String),
    /// The VEN does not accept this objective.
    NotAccepted { ven: String, objective: String },
}

impl CommandRefused {
    pub fn message(&self) -> String {
        match self {
            Self::UnknownVen(ven) => format!("unknown VEN `{ven}`"),
            Self::ObjectivesUnknown(ven) => {
                format!("VEN `{ven}` has not published the objectives it accepts")
            }
            Self::NotAccepted { ven, objective } => {
                format!("VEN `{ven}` does not accept the objective `{objective}`")
            }
        }
    }
}

/// Whether `objective` may be commanded to `ven`; `None` (release) is always valid for a known
/// VEN.
pub fn validate(
    fleet: &BTreeMap<String, VenLiveState>,
    ven: &str,
    objective: Option<&str>,
) -> Result<(), CommandRefused> {
    let live = fleet
        .get(ven)
        .ok_or_else(|| CommandRefused::UnknownVen(ven.to_string()))?;
    let Some(objective) = objective else {
        return Ok(());
    };
    let accepted = live
        .available_objectives()
        .ok_or_else(|| CommandRefused::ObjectivesUnknown(ven.to_string()))?;
    if accepted.iter().any(|o| o == objective) {
        Ok(())
    } else {
        Err(CommandRefused::NotAccepted {
            ven: ven.to_string(),
            objective: objective.to_string(),
        })
    }
}

/// The retained body of `command/objective`: the command, or empty to release it.
pub fn payload(objective: Option<&str>, requested_at: DateTime<Utc>) -> Vec<u8> {
    match objective {
        None => Vec::new(),
        Some(objective) => serde_json::to_vec(&ObjectiveCommand {
            objective,
            requested_at,
        })
        .expect("an objective command always serialises"),
    }
}

/// Publish the command to one VEN.
pub fn command_ven(
    publisher: &dyn CommandPublisher,
    root: &str,
    ven: &str,
    objective: Option<&str>,
    requested_at: DateTime<Utc>,
) -> Result<(), String> {
    publisher.publish_retained(
        ven_topic(root, ven, leaf::COMMAND_OBJECTIVE),
        payload(objective, requested_at),
    )
}

/// The VENs a fleet-wide command goes to: every known VEN for a release, and every VEN that
/// accepts the objective otherwise. Refused when any VEN that declares its objectives does not
/// accept it, so a fleet switch is all or nothing; a VEN that has not declared them is skipped
/// and reported.
pub fn fleet_targets(
    fleet: &BTreeMap<String, VenLiveState>,
    objective: Option<&str>,
) -> Result<(Vec<String>, Vec<String>), CommandRefused> {
    let (mut targets, mut skipped) = (Vec::new(), Vec::new());
    for ven in fleet.keys() {
        match validate(fleet, ven, objective) {
            Ok(()) => targets.push(ven.clone()),
            Err(CommandRefused::ObjectivesUnknown(ven)) => skipped.push(ven),
            Err(refused) => return Err(refused),
        }
    }
    Ok((targets, skipped))
}

/// Publish to each VEN in turn, `spacing` apart, from a task of its own: every VEN replans when
/// its objective changes, and a planning solve takes tens of seconds on a fleet host, so a switch
/// of the whole fleet at once would put every solve on the same instant.
pub fn command_fleet(
    publisher: Arc<dyn CommandPublisher>,
    root: String,
    targets: Vec<String>,
    objective: Option<String>,
    spacing: std::time::Duration,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        for (i, ven) in targets.iter().enumerate() {
            if i > 0 {
                tokio::time::sleep(spacing).await;
            }
            if let Err(e) = command_ven(
                publisher.as_ref(),
                &root,
                ven,
                objective.as_deref(),
                Utc::now(),
            ) {
                tracing::warn!(ven, error = %e, "fleet objective command not published");
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[derive(Default)]
    pub struct Recorder(pub Mutex<Vec<(tokio::time::Instant, String, Vec<u8>)>>);

    impl CommandPublisher for Recorder {
        fn publish_retained(&self, topic: String, payload: Vec<u8>) -> Result<(), String> {
            self.0
                .lock()
                .unwrap()
                .push((tokio::time::Instant::now(), topic, payload));
            Ok(())
        }
    }

    fn now() -> DateTime<Utc> {
        DateTime::from_timestamp(1_760_000_000, 0).unwrap()
    }

    fn fleet() -> BTreeMap<String, VenLiveState> {
        let mut fleet = BTreeMap::new();
        let objective = br#"{"objective":"min_cost","source":"profile","set_at":null,
            "profile_objective":"min_cost","available_objectives":["min_cost","opportunistic"]}"#;
        for ven in ["ven-1", "ven-2"] {
            crate::fleet::apply_message(&mut fleet, ven, "objective", objective, now());
        }
        crate::fleet::apply_message(&mut fleet, "ven-3", "telemetry", b"{}", now());
        fleet
    }

    #[test]
    fn a_command_is_validated_against_the_objectives_the_ven_declares() {
        let fleet = fleet();
        assert_eq!(validate(&fleet, "ven-1", Some("opportunistic")), Ok(()));
        assert_eq!(
            validate(&fleet, "ven-1", None),
            Ok(()),
            "a release is always valid"
        );
        let refused = validate(&fleet, "ven-1", Some("max_happiness")).unwrap_err();
        assert!(refused.message().contains("ven-1") && refused.message().contains("max_happiness"));
        assert_eq!(
            validate(&fleet, "ven-3", Some("opportunistic")),
            Err(CommandRefused::ObjectivesUnknown("ven-3".into()))
        );
        assert_eq!(
            validate(&fleet, "ven-9", None),
            Err(CommandRefused::UnknownVen("ven-9".into()))
        );
    }

    #[test]
    fn the_command_body_is_the_objective_and_its_issue_time_and_a_release_is_empty() {
        let body: serde_json::Value =
            serde_json::from_slice(&payload(Some("opportunistic"), now())).unwrap();
        assert_eq!(
            body,
            serde_json::json!({"objective": "opportunistic", "requested_at": "2025-10-09T08:53:20Z"})
        );
        assert!(payload(None, now()).is_empty());
    }

    #[test]
    fn a_command_goes_retained_to_the_ven_s_command_leaf() {
        let recorder = Recorder::default();
        command_ven(&recorder, "openadr-lab", "ven-1", None, now()).unwrap();
        let sent = recorder.0.lock().unwrap();
        assert_eq!(sent[0].1, "openadr-lab/fleet/ven-1/command/objective");
        assert!(
            sent[0].2.is_empty(),
            "null publishes an empty retained message"
        );
    }

    /// All or nothing: one VEN refusing refuses the switch; a VEN that declares nothing is
    /// skipped; a release goes to every known VEN.
    #[test]
    fn a_fleet_switch_goes_to_every_accepting_ven_or_to_none() {
        let fleet = fleet();
        assert_eq!(
            fleet_targets(&fleet, Some("opportunistic")),
            Ok((vec!["ven-1".into(), "ven-2".into()], vec!["ven-3".into()]))
        );
        assert!(fleet_targets(&fleet, Some("max_happiness")).is_err());
        assert_eq!(
            fleet_targets(&fleet, None),
            Ok((vec!["ven-1".into(), "ven-2".into(), "ven-3".into()], vec![]))
        );
    }

    #[tokio::test(start_paused = true)]
    async fn a_fleet_switch_publishes_one_ven_at_a_time() {
        let recorder = Arc::new(Recorder::default());
        let started = tokio::time::Instant::now();
        command_fleet(
            recorder.clone(),
            "openadr-lab".into(),
            vec!["ven-1".into(), "ven-2".into(), "ven-3".into()],
            Some("opportunistic".into()),
            std::time::Duration::from_secs(10),
        )
        .await
        .unwrap();
        let sent = recorder.0.lock().unwrap();
        let offsets_s: Vec<u64> = sent
            .iter()
            .map(|(at, _, _)| (*at - started).as_secs())
            .collect();
        assert_eq!(offsets_s, vec![0, 10, 20]);
        assert_eq!(sent[2].1, "openadr-lab/fleet/ven-3/command/objective");
    }
}
