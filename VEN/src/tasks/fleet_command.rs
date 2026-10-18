//! Carrying out objective commands from the fleet channel (openspec `opportunistic-objective`
//! D10).
//!
//! Reads each payload the fleet publisher received on `command/objective`, asks
//! `services::objective::decide_fleet_command` what it means and applies the answer through
//! `set_objective`, the setter the VEN UI uses too. Owns only the memory of the last command
//! applied, which is what tells a broker redelivery from a new command. Never decides what a
//! payload means itself and never writes the objective setting directly.

use chrono::{DateTime, Utc};
use tracing::{info, warn};

use crate::app_ctx::{ActiveObjective, PlanTriggerTx, Telemetry};
use crate::entities::objective_setting::ObjectiveSource;
use crate::services::objective::{decide_fleet_command, set_objective, FleetCommandDecision};

/// Apply commands until the publisher's channel closes.
pub fn spawn(
    mut commands: tokio::sync::mpsc::Receiver<Vec<u8>>,
    setting: ActiveObjective,
    trigger: PlanTriggerTx,
    telemetry: Telemetry,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut last_applied: Option<DateTime<Utc>> = None;
        while let Some(payload) = commands.recv().await {
            let current = setting.read().await.clone();
            let decision = decide_fleet_command(&payload, last_applied, &current);
            let target = match decision {
                FleetCommandDecision::Apply(cmd) => {
                    last_applied = Some(cmd.requested_at);
                    info!(objective = ?cmd.objective, requested_at = %cmd.requested_at, "fleet command: objective set");
                    Some((cmd.objective, ObjectiveSource::Fleet))
                }
                FleetCommandDecision::RevertToProfile => {
                    info!(objective = ?current.profile_objective, "fleet command withdrawn: back to the profile objective");
                    Some((current.profile_objective, ObjectiveSource::Profile))
                }
                FleetCommandDecision::Ignore(why) => {
                    info!(why, "fleet command ignored");
                    None
                }
                FleetCommandDecision::Malformed(error) => {
                    warn!(error, "fleet command unreadable; objective unchanged");
                    None
                }
            };
            if let Some((objective, source)) = target {
                set_objective(
                    &setting,
                    &trigger,
                    telemetry.as_ref(),
                    objective,
                    source,
                    Utc::now(),
                )
                .await;
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entities::asset::{PlanTrigger, PlanTriggerSignal};
    use crate::entities::objective_setting::ObjectiveSetting;
    use crate::entities::planner_params::PlannerObjective;
    use std::sync::Arc;

    /// End to end through the channel: a command applies with the fleet as its source, its
    /// redelivery is ignored after a local change, and the withdrawal of a command no longer in
    /// force leaves that change alone.
    #[tokio::test]
    async fn commands_apply_once_and_never_undo_a_later_local_change() {
        let setting: ActiveObjective = Arc::new(tokio::sync::RwLock::new(
            ObjectiveSetting::from_profile(PlannerObjective::MinCost),
        ));
        let (tx, _rx) = tokio::sync::watch::channel(PlanTriggerSignal::bare(PlanTrigger::Periodic));
        let trigger: PlanTriggerTx = Arc::new(tx);
        let telemetry: Telemetry = Arc::new(crate::controller::telemetry_port::NoTelemetry);
        let (cmd_tx, cmd_rx) = tokio::sync::mpsc::channel(8);
        let task = spawn(cmd_rx, setting.clone(), trigger.clone(), telemetry.clone());

        let command = br#"{"objective": "opportunistic", "requested_at": "2026-10-16T12:00:00Z"}"#;
        cmd_tx.send(command.to_vec()).await.unwrap();
        cmd_tx.send(Vec::new()).await.unwrap(); // withdrawn while in force
        cmd_tx.send(command.to_vec()).await.unwrap(); // redelivery
        drop(cmd_tx);
        task.await.unwrap();
        let s = setting.read().await.clone();
        assert_eq!(
            (s.objective, s.source),
            (PlannerObjective::MinCost, ObjectiveSource::Profile),
            "applied, withdrawn back to the profile, redelivery ignored"
        );

        // A local change after the fleet's command survives the command's redelivery.
        let (cmd_tx, cmd_rx) = tokio::sync::mpsc::channel(8);
        let task = spawn(cmd_rx, setting.clone(), trigger, telemetry.clone());
        cmd_tx.send(command.to_vec()).await.unwrap();
        // Let the consumer apply it before the local change.
        while setting.read().await.source != ObjectiveSource::Fleet {
            tokio::task::yield_now().await;
        }
        setting
            .write()
            .await
            .set(PlannerObjective::MinGhg, ObjectiveSource::VenUi, Utc::now());
        cmd_tx.send(command.to_vec()).await.unwrap();
        cmd_tx.send(Vec::new()).await.unwrap();
        drop(cmd_tx);
        task.await.unwrap();
        let s = setting.read().await.clone();
        assert_eq!(
            (s.objective, s.source),
            (PlannerObjective::MinGhg, ObjectiveSource::VenUi)
        );
    }
}
