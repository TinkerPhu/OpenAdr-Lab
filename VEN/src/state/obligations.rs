//! Report-obligation accessors — split out of `mod.rs` to keep it under the
//! file-size cap; behaves as an ordinary `impl AppState` block.
use chrono::{DateTime, Utc};

use crate::entities::capacity::OadrReportObligation;

use super::AppState;

impl AppState {
    pub async fn report_obligations(&self) -> Vec<OadrReportObligation> {
        self.hems.read().await.report_obligations.clone()
    }

    pub async fn set_report_obligations(&self, obligations: Vec<OadrReportObligation>) {
        self.hems.write().await.report_obligations = obligations;
    }

    /// Append new obligations without duplicating existing ones (keyed by id).
    pub async fn add_obligations(&self, new_obs: Vec<OadrReportObligation>) {
        if new_obs.is_empty() {
            return;
        }
        let mut hems = self.hems.write().await;
        for ob in new_obs {
            if !hems.report_obligations.iter().any(|e| e.id == ob.id) {
                hems.report_obligations.push(ob);
            }
        }
    }

    /// Advance a fulfilled obligation to its next cycle. `fulfilled` stays false —
    /// recurrence is driven entirely by `due_at`; `retire_obligations_not_in` below is
    /// what actually stops an obligation, not this flag.
    pub async fn rearm_obligation(&self, id: uuid::Uuid, next_due_at: DateTime<Utc>) {
        let mut hems = self.hems.write().await;
        if let Some(ob) = hems.report_obligations.iter_mut().find(|o| o.id == id) {
            ob.due_at = next_due_at;
        }
    }

    /// GB-23: remove a single obligation by id — used when the VTN has
    /// confirmed (via 404 on report submission) that its source event/program
    /// no longer exists. Unlike `retire_obligations_not_in`, this targets
    /// exactly the one obligation whose own submission 404'd, not every
    /// obligation sharing its `event_id` (design.md D2).
    pub async fn remove_obligation(&self, id: uuid::Uuid) {
        let mut hems = self.hems.write().await;
        hems.report_obligations.retain(|o| o.id != id);
    }

    /// Remove obligations whose parent event is no longer in the active poll set.
    pub async fn retire_obligations_not_in(
        &self,
        active_event_ids: &std::collections::HashSet<String>,
    ) {
        let mut hems = self.hems.write().await;
        hems.report_obligations
            .retain(|o| active_event_ids.contains(&o.event_id));
    }

    /// Return all unfulfilled obligations whose due_at <= now.
    pub async fn due_obligations(&self, now: DateTime<Utc>) -> Vec<OadrReportObligation> {
        self.hems
            .read()
            .await
            .report_obligations
            .iter()
            .filter(|o| o.is_due(now))
            .cloned()
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_obligation(id: uuid::Uuid, event_id: &str) -> OadrReportObligation {
        OadrReportObligation {
            id,
            event_id: event_id.to_string(),
            program_id: Some("prog-1".to_string()),
            payload_type: "USAGE".to_string(),
            reading_type: "DIRECT_READ".to_string(),
            resource_name: None,
            due_at: Utc::now(),
            interval_duration_s: 900,
            fulfilled: false,
            created_at: Utc::now(),
            historical: true,
        }
    }

    #[tokio::test]
    async fn remove_obligation_removes_only_the_matching_id() {
        let state = AppState::new();
        let id_a = uuid::Uuid::new_v4();
        let id_b = uuid::Uuid::new_v4();
        state
            .add_obligations(vec![
                make_obligation(id_a, "evt-a"),
                make_obligation(id_b, "evt-b"),
            ])
            .await;

        state.remove_obligation(id_a).await;

        let obs = state.report_obligations().await;
        assert_eq!(obs.len(), 1);
        assert_eq!(obs[0].id, id_b);
    }

    #[tokio::test]
    async fn remove_obligation_no_op_when_id_not_present() {
        let state = AppState::new();
        let id_a = uuid::Uuid::new_v4();
        state
            .add_obligations(vec![make_obligation(id_a, "evt-a")])
            .await;

        state.remove_obligation(uuid::Uuid::new_v4()).await;

        assert_eq!(state.report_obligations().await.len(), 1);
    }
}
