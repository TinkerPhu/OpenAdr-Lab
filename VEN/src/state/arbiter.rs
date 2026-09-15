//! Deviation-arbiter (§5.3/§5.5, `openspec/changes/deviation-arbiter/`)
//! `AppState` accessors — split out of `mod.rs` to keep it under the
//! file-size cap; behaves as an ordinary `impl AppState` block.

use chrono::{DateTime, Utc};
use serde::Serialize;
use std::collections::HashMap;

use super::AppState;

/// Last tick's arbiter reasoning — surfaced via `GET /arbiter-diagnostics`
/// so the reactive levers aren't only-server-side state (ui-transparency).
/// `None` fields mirror `ArbiterOutcome`'s: absent during the no-plan-yet
/// startup window or before the arbiter has run at all this process.
/// `net_kw`/`dev_kw`/`active_lever`/`unresolved_kw` are the deviation pass's;
/// `limit` is the limit-enforcement pass's (GB-47).
#[derive(Debug, Clone, Default, Serialize)]
pub struct ArbiterDiagnostics {
    pub net_kw: Option<f64>,
    pub dev_kw: Option<f64>,
    pub active_lever: Option<String>,
    /// Deviation no lever could take (kW).
    #[serde(default)]
    pub unresolved_kw: f64,
    /// Site net power measured after this tick's physics (kW) — what the
    /// passes' projection of the same tick came to.
    pub measured_net_kw: Option<f64>,
    /// `None` while limit enforcement is off or no hard import limit is in force.
    pub limit: Option<crate::controller::arbiter::limit::LimitPassOutcome>,
    pub updated_at: Option<DateTime<Utc>>,
}

impl AppState {
    pub async fn deviation_arbiter_enabled(&self) -> bool {
        self.hems.read().await.deviation_arbiter_enabled
    }

    pub async fn set_deviation_arbiter_enabled(&self, enabled: bool) {
        self.hems.write().await.deviation_arbiter_enabled = enabled;
    }

    /// Whether the arbiter enforces hard import limits (GB-47). On by default;
    /// switched off only to measure the planner alone.
    pub async fn limit_enforcement_enabled(&self) -> bool {
        self.hems.read().await.limit_enforcement_enabled
    }

    pub async fn set_limit_enforcement_enabled(&self, enabled: bool) {
        self.hems.write().await.limit_enforcement_enabled = enabled;
    }

    pub async fn limit_active_lever(&self) -> Option<String> {
        self.hems.read().await.limit_active_lever.clone()
    }

    pub async fn set_limit_active_lever(&self, lever: Option<String>) {
        self.hems.write().await.limit_active_lever = lever;
    }

    /// Add `kwh` (signed: positive = absorbed extra import/charge, matches the
    /// sign convention `arbiter::reconcile` records) to `asset_id`'s residual
    /// accumulator, creating an entry with `capacity_kwh_at_last_plan = 0.0`
    /// if none exists yet (only populated for battery/EV — see
    /// `entities::arbiter_residual`).
    pub async fn accumulate_residual(&self, asset_id: &str, kwh: f64) {
        let mut hems = self.hems.write().await;
        hems.arbiter_residual
            .entry(asset_id.to_string())
            .or_default()
            .absorbed_kwh += kwh.abs();
    }

    pub async fn residual_state(
        &self,
    ) -> HashMap<String, crate::entities::arbiter_residual::AssetResidual> {
        self.hems.read().await.arbiter_residual.clone()
    }

    /// Reset every tracked asset's absorbed-kWh accumulator to zero and
    /// re-snapshot its capacity baseline from the newly-adopted plan/current
    /// snapshot — called at every plan adoption (any trigger), per §5.5.
    pub async fn reset_residual(&self, new_capacities_kwh: &HashMap<String, f64>) {
        let mut hems = self.hems.write().await;
        for (asset_id, capacity_kwh) in new_capacities_kwh {
            hems.arbiter_residual.insert(
                asset_id.clone(),
                crate::entities::arbiter_residual::AssetResidual {
                    absorbed_kwh: 0.0,
                    capacity_kwh_at_last_plan: *capacity_kwh,
                },
            );
        }
    }

    pub async fn last_residual_trigger_at(&self) -> Option<DateTime<Utc>> {
        self.hems.read().await.last_residual_trigger_at
    }

    pub async fn set_last_residual_trigger_at(&self, at: DateTime<Utc>) {
        self.hems.write().await.last_residual_trigger_at = Some(at);
    }

    pub async fn arbiter_active_lever(&self) -> Option<String> {
        self.hems.read().await.arbiter_active_lever.clone()
    }

    pub async fn set_arbiter_active_lever(&self, lever: Option<String>) {
        self.hems.write().await.arbiter_active_lever = lever;
    }

    pub async fn arbiter_diagnostics(&self) -> ArbiterDiagnostics {
        self.hems.read().await.arbiter_diagnostics.clone()
    }

    pub async fn set_arbiter_diagnostics(&self, diagnostics: ArbiterDiagnostics) {
        self.hems.write().await.arbiter_diagnostics = diagnostics;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn limit_enforcement_is_on_and_deviation_correction_off_by_default() {
        let state = AppState::new();
        assert!(state.limit_enforcement_enabled().await);
        assert!(!state.deviation_arbiter_enabled().await);
    }
}
