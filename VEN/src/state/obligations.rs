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
