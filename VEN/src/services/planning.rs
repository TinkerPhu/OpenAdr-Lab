use chrono::{DateTime, Utc};
use tracing::{debug, info};

use crate::controller::trace::ControllerEvent;
use crate::entities::asset::PlanTrigger;
use crate::entities::plan::Plan;
use crate::entities::PlannerObjective;
use crate::planner_events::{PlannerEvent, PlannerEventTx};
use crate::state::AppState;

pub struct PlanningService;

/// Result of one completed planning cycle.
pub struct PlanCycleResult {
    pub adopted: bool,
    pub plan: Plan,
    /// The PlanCycle controller event pushed during this cycle (reuse for status reports).
    pub plan_cycle_event: ControllerEvent,
}

/// Pure function — evaluate whether the new plan should replace the current one.
///
/// Hard triggers (anything other than Periodic) always adopt.
/// Periodic triggers adopt only when the cost improvement exceeds the effective threshold,
/// or when the current plan has fully decayed past its decay window.
pub fn evaluate_acceptance_gate(
    current: Option<&Plan>,
    new_plan: &Plan,
    trigger: &PlanTrigger,
    threshold_eur: f64,
    decay_s: f64,
    now: DateTime<Utc>,
) -> bool {
    let is_hard_trigger = !matches!(trigger, PlanTrigger::Periodic);

    if is_hard_trigger || threshold_eur == 0.0 {
        return true;
    }

    let Some(current) = current else {
        return true; // no existing plan → always adopt
    };

    let elapsed_s = (now - current.created_at).num_seconds().max(0) as f64;
    let decay_factor = if decay_s > 0.0 {
        (1.0 - elapsed_s / decay_s).max(0.0)
    } else {
        1.0
    };
    let effective_threshold = threshold_eur * decay_factor;
    let fully_decayed = decay_s > 0.0 && elapsed_s >= decay_s;

    let current_total = current.objective_eur + current.friction_eur;
    let new_total = new_plan.objective_eur + new_plan.friction_eur;
    let improvement = current_total - new_total;

    if fully_decayed || improvement > effective_threshold {
        true
    } else {
        debug!(
            improvement_eur = improvement,
            effective_threshold_eur = effective_threshold,
            elapsed_s,
            "periodic plan rejected: improvement below threshold"
        );
        false
    }
}

impl PlanningService {
    /// Post-solve step: evaluate acceptance gate, adopt or reject, emit events, update state.
    ///
    /// Called by `tasks/planning.rs` after `spawn_blocking` returns the solved Plan.
    /// Accepts `objective` explicitly since it lives in `AppCtx`, not `AppState`.
    pub async fn adopt_if_warranted(
        plan: Plan,
        trigger: &PlanTrigger,
        trigger_reason: &str,
        threshold_eur: f64,
        decay_s: f64,
        solver_ms: u64,
        objective: PlannerObjective,
        state: &AppState,
        event_tx: &PlannerEventTx,
        now: DateTime<Utc>,
    ) -> PlanCycleResult {
        // Emit PlanReady before gate evaluation so SSE clients always receive it.
        let _ = event_tx.send(PlannerEvent::PlanReady {
            plan_id: plan.id,
            objective,
            solver_ms,
            objective_eur: plan.objective_eur,
            friction_eur: plan.friction_eur,
            slot_count: plan.slots.len(),
            trigger: trigger_reason.to_string(),
        });

        let current = state.active_plan().await;
        let adopted = evaluate_acceptance_gate(
            current.as_ref(),
            &plan,
            trigger,
            threshold_eur,
            decay_s,
            now,
        );

        let slot_count = plan.slots.len();
        if adopted {
            info!(trigger = %trigger_reason, slot_count, "planner: plan adopted");
            let _ = event_tx.send(PlannerEvent::CorrectionCleared {
                ts: now,
                reason: "superseded_by_replan".to_string(),
            });
            state.set_active_plan(Some(plan.clone())).await;
        } else {
            info!(
                trigger = %trigger_reason,
                slot_count,
                "planner: plan NOT adopted (periodic below threshold)"
            );
        }

        let plan_cycle_event = ControllerEvent::PlanCycle {
            ts: now,
            trigger_reason: trigger_reason.to_string(),
            total_slots: slot_count,
        };
        state.push_controller_event(plan_cycle_event.clone()).await;

        PlanCycleResult { adopted, plan, plan_cycle_event }
    }
}

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;
    use uuid::Uuid;

    /// Build a minimal Plan for gate testing. Only objective_eur, friction_eur, created_at
    /// affect the acceptance gate; all other fields are filled with harmless defaults.
    fn make_plan_at(objective_eur: f64, friction_eur: f64, created_at: DateTime<Utc>) -> Plan {
        serde_json::from_value(serde_json::json!({
            "id": Uuid::new_v4().to_string(),
            "created_at": created_at.to_rfc3339(),
            "trigger": "PERIODIC",
            "horizon": {
                "start_time": "2026-01-01T00:00:00Z",
                "end_time": "2026-01-02T00:00:00Z",
                "step_size_s": 900,
                "num_steps": 96,
                "far_horizon": "2026-01-02T00:00:00Z"
            },
            "slots": [],
            "summary": {
                "total_cost_eur": 0.0,
                "total_co2_g": 0.0,
                "total_import_kwh": 0.0,
                "total_export_kwh": 0.0
            },
            "envelopes": [],
            "warnings": [],
            "objective_eur": objective_eur,
            "friction_eur": friction_eur
        }))
        .expect("test Plan must deserialize")
    }

    fn make_plan(objective_eur: f64, friction_eur: f64) -> Plan {
        make_plan_at(objective_eur, friction_eur, Utc::now())
    }

    fn make_plan_aged(objective_eur: f64, friction_eur: f64, age_s: i64) -> Plan {
        make_plan_at(
            objective_eur,
            friction_eur,
            Utc::now() - Duration::seconds(age_s),
        )
    }

    #[test]
    fn test_gate_rejects_below_threshold_on_periodic() {
        let current = make_plan(10.0, 1.0); // total 11.0
        let new_plan = make_plan(9.5, 1.0); // total 10.5, improvement 0.5 < threshold 1.0
        let adopt = evaluate_acceptance_gate(
            Some(&current),
            &new_plan,
            &PlanTrigger::Periodic,
            1.0,
            3600.0,
            Utc::now(),
        );
        assert!(!adopt, "improvement below threshold must be rejected on periodic trigger");
    }

    #[test]
    fn test_gate_accepts_on_deviation_trigger() {
        let current = make_plan(10.0, 1.0);
        let new_plan = make_plan(10.0, 1.5); // worse cost
        let adopt = evaluate_acceptance_gate(
            Some(&current),
            &new_plan,
            &PlanTrigger::DeviceDeviation,
            1.0,
            3600.0,
            Utc::now(),
        );
        assert!(adopt, "DeviceDeviation is a hard trigger and must always adopt");
    }

    #[test]
    fn test_gate_accepts_when_no_current_plan() {
        let new_plan = make_plan(10.0, 1.0);
        let adopt = evaluate_acceptance_gate(
            None,
            &new_plan,
            &PlanTrigger::Periodic,
            5.0,
            3600.0,
            Utc::now(),
        );
        assert!(adopt, "no existing plan must always adopt");
    }

    #[test]
    fn test_gate_accepts_after_decay_window() {
        // Plan is 2h old, decay_s = 3600 → fully_decayed → force-adopt
        let current = make_plan_aged(10.0, 0.0, 7200);
        let new_plan = make_plan(10.0, 0.0); // same cost
        let adopt = evaluate_acceptance_gate(
            Some(&current),
            &new_plan,
            &PlanTrigger::Periodic,
            1.0,
            3600.0,
            Utc::now(),
        );
        assert!(adopt, "plan past decay window must be replaced unconditionally");
    }

    #[test]
    fn test_gate_accepts_epsilon_improvement() {
        let current = make_plan(10.0, 1.0); // total 11.0
        let new_plan = make_plan(8.9, 1.0); // total 9.9, improvement 1.1 > threshold 1.0
        let adopt = evaluate_acceptance_gate(
            Some(&current),
            &new_plan,
            &PlanTrigger::Periodic,
            1.0,
            3600.0,
            Utc::now(),
        );
        assert!(adopt, "improvement exceeding threshold must be accepted");
    }
}
