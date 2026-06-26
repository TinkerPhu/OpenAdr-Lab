use chrono::{DateTime, Utc};
use tracing::{debug, info};

use crate::controller::trace::ControllerEvent;
use crate::entities::asset::PlanTrigger;
use crate::entities::plan::Plan;
use crate::entities::PlannerObjective;
use crate::planner_events::{PlannerEvent, PlannerEventTx};
use crate::state::AppState;

pub struct PlanningService;

/// Returns the end time of the consecutive heater block containing `now` in `plan`.
///
/// Reads the heater power in the first future slot (start of block), then walks forward
/// while heater power stays within 0.1 kW of that value. Returns the `end` of the last
/// slot in the run, or `None` when no future slots exist.
pub fn heater_block_end(plan: &Plan, now: DateTime<Utc>) -> Option<DateTime<Utc>> {
    let mut iter = plan.all_slots().filter(|s| s.end > now).peekable();
    let kw0 = iter
        .peek()?
        .planned_kw_by_asset
        .get("heater")
        .copied()
        .unwrap_or(0.0);
    iter.take_while(|s| {
        let kw = s.planned_kw_by_asset.get("heater").copied().unwrap_or(0.0);
        (kw - kw0).abs() < 0.1
    })
    .last()
    .map(|s| s.end)
}

/// Build a per-slot heater anchor vector for the next planning cycle.
///
/// Each entry is `Some(kw)` for slots whose start is strictly before `anchor_until`
/// (pinning the heater tier binaries to the current plan's values) and `None` for slots
/// beyond the anchor window (left free for the optimizer).
/// Returns all-None when `plan` or `anchor_until` is absent.
pub fn build_heater_anchor(
    plan: Option<&Plan>,
    anchor_until: Option<DateTime<Utc>>,
    now: DateTime<Utc>,
    n_slots: usize,
) -> Vec<Option<f64>> {
    let mut out = vec![None; n_slots];
    let (Some(plan), Some(until)) = (plan, anchor_until) else {
        return out;
    };
    for (i, slot) in plan
        .all_slots()
        .filter(|s| s.end > now)
        .take(n_slots)
        .enumerate()
    {
        if slot.start >= until {
            break;
        }
        out[i] = Some(
            slot.planned_kw_by_asset
                .get("heater")
                .copied()
                .unwrap_or(0.0),
        );
    }
    out
}

/// Result of one completed planning cycle.
pub struct PlanCycleResult {
    pub adopted: bool,
    pub plan: Plan,
    /// The PlanCycle controller event pushed during this cycle (reuse for status reports).
    pub plan_cycle_event: ControllerEvent,
}

/// Count heater relay switches in `plan` for all slots starting at or after `now`.
///
/// A switch is a transition where the heater power changes by more than 0.1 kW between
/// consecutive future slots. Past slots are excluded so this reflects the remaining
/// switching burden of the plan from the current moment onward.
pub fn count_heater_switches(plan: &Plan, now: DateTime<Utc>) -> usize {
    let mut count = 0usize;
    let mut prev: Option<f64> = None;
    for slot in plan.all_slots().filter(|s| s.start >= now) {
        let kw = slot
            .planned_kw_by_asset
            .get("heater")
            .copied()
            .unwrap_or(0.0);
        if prev.is_some_and(|p| (p - kw).abs() > 0.1) {
            count += 1;
        }
        prev = Some(kw);
    }
    count
}

/// Pure function — evaluate whether the new plan should replace the current one.
///
/// Hard triggers (anything other than Periodic) always adopt.
/// Periodic triggers adopt only when the cost improvement exceeds the effective threshold
/// plus any switch-count surcharge, or when the current plan has fully decayed.
///
/// `gate_switch_penalty_eur`: per-extra-switch surcharge added to the effective threshold.
/// 0.0 disables the switch guard (backward-compatible default).
pub fn evaluate_acceptance_gate(
    current: Option<&Plan>,
    new_plan: &Plan,
    trigger: &PlanTrigger,
    threshold_eur: f64,
    decay_s: f64,
    gate_switch_penalty_eur: f64,
    now: DateTime<Utc>,
) -> bool {
    let is_hard_trigger = !matches!(trigger, PlanTrigger::Periodic);

    if is_hard_trigger || (threshold_eur == 0.0 && gate_switch_penalty_eur == 0.0) {
        return true;
    }

    let Some(current) = current else {
        return true; // no existing plan → always adopt
    };

    // Force-adopt when the existing plan has no future slots (all expired).
    // Prevents a stale plan from permanently blocking forecasts when the threshold
    // would otherwise reject every periodic replan.
    // Empty slot list is not treated as "expired" — that indicates a plan that was
    // never populated (e.g. a zero-asset solve); the normal threshold logic applies.
    let current_expired = current
        .all_slots()
        .map(|s| s.end)
        .max()
        .is_some_and(|end| end <= now);
    if current_expired {
        return true;
    }

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

    // Switch surcharge: extra heater relay operations in the new plan raise the bar.
    // fully_decayed still bypasses — decay is an escape hatch for stale plans regardless.
    let switch_surcharge = if gate_switch_penalty_eur > 0.0 {
        let extra = count_heater_switches(new_plan, now)
            .saturating_sub(count_heater_switches(current, now)) as f64;
        extra * gate_switch_penalty_eur
    } else {
        0.0
    };

    if fully_decayed || improvement > effective_threshold + switch_surcharge {
        true
    } else {
        debug!(
            improvement_eur = improvement,
            effective_threshold_eur = effective_threshold,
            switch_surcharge_eur = switch_surcharge,
            elapsed_s,
            "periodic plan rejected: improvement below threshold + switch surcharge"
        );
        false
    }
}

impl PlanningService {
    /// Post-solve step: evaluate acceptance gate, adopt or reject, emit events, update state.
    ///
    /// Called by `tasks/planning.rs` after `spawn_blocking` returns the solved Plan.
    /// Accepts `objective` explicitly since it lives in `AppCtx`, not `AppState`.
    #[allow(clippy::too_many_arguments)]
    pub async fn adopt_if_warranted(
        plan: Plan,
        trigger: &PlanTrigger,
        trigger_reason: &str,
        threshold_eur: f64,
        decay_s: f64,
        gate_switch_penalty_eur: f64,
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
            gate_switch_penalty_eur,
            now,
        );

        let slot_count = plan.slots.len();
        if adopted {
            info!(trigger = %trigger_reason, slot_count, "planner: plan adopted");
            state.set_active_plan(Some(plan.clone())).await;
            let anchor = heater_block_end(&plan, now);
            state.set_anchor_until(anchor).await;
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

        PlanCycleResult {
            adopted,
            plan,
            plan_cycle_event,
        }
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

    /// Build a plan whose single slot ended `slot_age_s` seconds ago.
    fn make_plan_with_expired_slot(objective_eur: f64, slot_age_s: i64) -> Plan {
        let now = Utc::now();
        let slot_end = now - Duration::seconds(slot_age_s);
        let slot_start = slot_end - Duration::seconds(300);
        serde_json::from_value(serde_json::json!({
            "id": Uuid::new_v4().to_string(),
            "created_at": (now - Duration::seconds(slot_age_s + 300)).to_rfc3339(),
            "trigger": "PERIODIC",
            "horizon": {
                "start_time": slot_start.to_rfc3339(),
                "end_time": slot_end.to_rfc3339(),
                "step_size_s": 300,
                "num_steps": 1,
                "far_horizon": slot_end.to_rfc3339()
            },
            "slots": [{
                "slot_index": 0,
                "start": slot_start.to_rfc3339(),
                "end": slot_end.to_rfc3339(),
                "import_tariff_eur_kwh": 0.20,
                "export_tariff_eur_kwh": 0.05,
                "co2_g_kwh": 300.0,
                "grid_effective_cost": 0.26,
                "rate_estimated": false,
                "import_cap_kw": 10.0,
                "export_cap_kw": 5.0,
                "baseline_kw": 0.4,
                "pv_forecast_kw": 0.0,
                "surplus_available_kw": 0.0,
                "allocations": [],
                "net_import_kw": 0.4,
                "net_export_kw": 0.0,
                "import_flexibility_kw": 0.0,
                "export_flexibility_kw": 0.0,
                "bat_charge_kw": 0.0,
                "bat_discharge_kw": 0.0,
                "planned_kw_by_asset": {},
                "planned_state_by_asset": {}
            }],
            "summary": {
                "total_cost_eur": 0.0,
                "total_co2_g": 0.0,
                "total_import_kwh": 0.0,
                "total_export_kwh": 0.0
            },
            "envelopes": [],
            "warnings": [],
            "objective_eur": objective_eur,
            "friction_eur": 0.0
        }))
        .expect("test Plan with expired slot must deserialize")
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
            0.0,
            Utc::now(),
        );
        assert!(
            !adopt,
            "improvement below threshold must be rejected on periodic trigger"
        );
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
            0.0,
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
            0.0,
            Utc::now(),
        );
        assert!(
            adopt,
            "plan past decay window must be replaced unconditionally"
        );
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
            0.0,
            Utc::now(),
        );
        assert!(adopt, "improvement exceeding threshold must be accepted");
    }

    #[test]
    fn test_gate_adopts_when_current_plan_slots_all_expired() {
        // Existing plan has one slot that ended 60 s ago — effectively stale.
        // Even though cost improvement is zero, the gate must force-adopt so
        // the timeline never loses its forecast window.
        let current = make_plan_with_expired_slot(10.0, 60);
        let new_plan = make_plan(10.0, 0.0); // same cost, no improvement
        let adopt = evaluate_acceptance_gate(
            Some(&current),
            &new_plan,
            &PlanTrigger::Periodic,
            5.0, // high threshold that would normally block adoption
            0.0, // no decay
            0.0,
            Utc::now(),
        );
        assert!(
            adopt,
            "stale plan with all-expired slots must be replaced unconditionally"
        );
    }

    // ── heater_block_end and build_heater_anchor tests ────────────────────────

    use chrono::TimeZone;

    fn fixed_now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 4, 11, 6, 0, 0).unwrap()
    }

    fn make_plan_with_heater_slots(start: DateTime<Utc>, step_s: i64, kws: &[f64]) -> Plan {
        let slots: Vec<serde_json::Value> = kws
            .iter()
            .enumerate()
            .map(|(i, &kw)| {
                let slot_start = start + Duration::seconds(i as i64 * step_s);
                let slot_end = slot_start + Duration::seconds(step_s);
                serde_json::json!({
                    "slot_index": i,
                    "start": slot_start.to_rfc3339(),
                    "end": slot_end.to_rfc3339(),
                    "import_tariff_eur_kwh": 0.25,
                    "export_tariff_eur_kwh": 0.08,
                    "co2_g_kwh": 300.0,
                    "grid_effective_cost": 0.25,
                    "rate_estimated": false,
                    "import_cap_kw": 25.0,
                    "export_cap_kw": 10.0,
                    "baseline_kw": 0.5,
                    "pv_forecast_kw": 0.0,
                    "surplus_available_kw": 0.0,
                    "allocations": [],
                    "net_import_kw": kw + 0.5,
                    "net_export_kw": 0.0,
                    "import_flexibility_kw": 0.0,
                    "export_flexibility_kw": 0.0,
                    "planned_kw_by_asset": {"heater": kw},
                    "planned_state_by_asset": {}
                })
            })
            .collect();
        let end = start + Duration::seconds(kws.len() as i64 * step_s);
        serde_json::from_value(serde_json::json!({
            "id": Uuid::new_v4().to_string(),
            "created_at": start.to_rfc3339(),
            "trigger": "PERIODIC",
            "horizon": {
                "start_time": start.to_rfc3339(),
                "end_time": end.to_rfc3339(),
                "step_size_s": step_s,
                "num_steps": kws.len(),
                "far_horizon": end.to_rfc3339()
            },
            "slots": slots,
            "summary": {
                "total_cost_eur": 0.0,
                "total_co2_g": 0.0,
                "total_import_kwh": 0.0,
                "total_export_kwh": 0.0
            },
            "envelopes": [],
            "warnings": [],
            "objective_eur": 0.0,
            "friction_eur": 0.0
        }))
        .expect("test plan must deserialize")
    }

    #[test]
    fn test_heater_block_end_on_block() {
        let now = fixed_now();
        let step_s = 1200i64; // 20 min
                              // 4 slots: [on, on, on, off]
        let plan = make_plan_with_heater_slots(now, step_s, &[2.0, 2.0, 2.0, 0.0]);
        let result = heater_block_end(&plan, now);
        let expected = now + Duration::seconds(3 * step_s); // end of slot 2
        assert_eq!(
            result,
            Some(expected),
            "on-block end should be end of slot 2"
        );
    }

    #[test]
    fn test_heater_block_end_off_block() {
        let now = fixed_now();
        let step_s = 1200i64;
        // 4 slots: [off, off, on, on]
        let plan = make_plan_with_heater_slots(now, step_s, &[0.0, 0.0, 2.0, 2.0]);
        let result = heater_block_end(&plan, now);
        let expected = now + Duration::seconds(2 * step_s); // end of slot 1
        assert_eq!(
            result,
            Some(expected),
            "off-block end should be end of slot 1"
        );
    }

    #[test]
    fn test_heater_block_end_no_future_slots() {
        let now = fixed_now();
        let step_s = 1200i64;
        // Plan ended before now (4 slots starting 80 min ago)
        let past = now - Duration::seconds(4 * step_s);
        let plan = make_plan_with_heater_slots(past, step_s, &[2.0, 2.0]);
        let result = heater_block_end(&plan, now);
        assert_eq!(result, None, "no future slots must return None");
    }

    #[test]
    fn test_build_heater_anchor_pins_within_window() {
        let now = fixed_now();
        let n_slots = 6;
        let anchor_until = now + Duration::minutes(60); // 3 × 20-min slots
        let plan = make_plan_with_heater_slots(now, 1200, &[2.0; 6]);
        let anchor = build_heater_anchor(Some(&plan), Some(anchor_until), now, n_slots);
        assert_eq!(anchor[0], Some(2.0), "slot 0 should be pinned");
        assert_eq!(anchor[1], Some(2.0), "slot 1 should be pinned");
        assert_eq!(anchor[2], Some(2.0), "slot 2 should be pinned");
        assert_eq!(anchor[3], None, "slot 3 is after anchor_until");
        assert_eq!(anchor[4], None, "slot 4 is after anchor_until");
        assert_eq!(anchor[5], None, "slot 5 is after anchor_until");
    }

    #[test]
    fn test_build_heater_anchor_no_plan_returns_all_none() {
        let now = fixed_now();
        let anchor = build_heater_anchor(None, Some(now + Duration::hours(1)), now, 4);
        assert!(
            anchor.iter().all(|v| v.is_none()),
            "no plan → all-None anchor"
        );
    }

    #[test]
    fn test_build_heater_anchor_no_until_returns_all_none() {
        let now = fixed_now();
        let plan = make_plan_with_heater_slots(now, 1200, &[2.0; 4]);
        let anchor = build_heater_anchor(Some(&plan), None, now, 4);
        assert!(
            anchor.iter().all(|v| v.is_none()),
            "no anchor_until → all-None anchor"
        );
    }

    // ── count_heater_switches tests ───────────────────────────────────────────

    /// Builds a plan with heater slots starting at `now` and a custom objective_eur.
    /// Used for gate surcharge tests that need both heater power patterns and cost.
    fn make_heater_plan(now: DateTime<Utc>, kws: &[f64], objective_eur: f64) -> Plan {
        let step_s = 1200_i64;
        let slots: Vec<serde_json::Value> = kws
            .iter()
            .enumerate()
            .map(|(i, &kw)| {
                let slot_start = now + Duration::seconds(i as i64 * step_s);
                let slot_end = slot_start + Duration::seconds(step_s);
                serde_json::json!({
                    "slot_index": i,
                    "start": slot_start.to_rfc3339(),
                    "end": slot_end.to_rfc3339(),
                    "import_tariff_eur_kwh": 0.25,
                    "export_tariff_eur_kwh": 0.08,
                    "co2_g_kwh": 300.0,
                    "grid_effective_cost": 0.25,
                    "rate_estimated": false,
                    "import_cap_kw": 25.0,
                    "export_cap_kw": 10.0,
                    "baseline_kw": 0.5,
                    "pv_forecast_kw": 0.0,
                    "surplus_available_kw": 0.0,
                    "allocations": [],
                    "net_import_kw": kw + 0.5,
                    "net_export_kw": 0.0,
                    "import_flexibility_kw": 0.0,
                    "export_flexibility_kw": 0.0,
                    "planned_kw_by_asset": {"heater": kw},
                    "planned_state_by_asset": {}
                })
            })
            .collect();
        let plan_end = now + Duration::seconds(kws.len() as i64 * step_s);
        serde_json::from_value(serde_json::json!({
            "id": Uuid::new_v4().to_string(),
            "created_at": now.to_rfc3339(),
            "trigger": "PERIODIC",
            "horizon": {
                "start_time": now.to_rfc3339(),
                "end_time": plan_end.to_rfc3339(),
                "step_size_s": step_s,
                "num_steps": kws.len(),
                "far_horizon": plan_end.to_rfc3339()
            },
            "slots": slots,
            "summary": {"total_cost_eur": 0.0, "total_co2_g": 0.0, "total_import_kwh": 0.0, "total_export_kwh": 0.0},
            "envelopes": [],
            "warnings": [],
            "objective_eur": objective_eur,
            "friction_eur": 0.0
        }))
        .expect("test heater plan must deserialize")
    }

    #[test]
    fn test_count_switches_empty_plan() {
        // All slots start before `now` — filter gives zero → 0 switches.
        let now = fixed_now();
        let past = now - Duration::seconds(4 * 1200);
        let plan = make_plan_with_heater_slots(past, 1200, &[2.0, 0.0, 2.0, 0.0]);
        assert_eq!(count_heater_switches(&plan, now), 0);
    }

    #[test]
    fn test_count_switches_one_block() {
        // All future slots at the same kW → no tier changes → 0 switches.
        let now = fixed_now();
        let plan = make_plan_with_heater_slots(now, 1200, &[2.0, 2.0, 2.0, 2.0]);
        assert_eq!(count_heater_switches(&plan, now), 0);
    }

    #[test]
    fn test_count_switches_filters_past_slots() {
        // 6-slot plan: slots 0–1 start before `now`, slots 2–5 start at/after `now`.
        // Past slots (0.0, 0.0) are filtered; future slots (0.0, 2.0, 0.0, 2.0) give 3 transitions.
        let now = fixed_now();
        let start = now - Duration::seconds(2 * 1200);
        let plan = make_plan_with_heater_slots(start, 1200, &[0.0, 0.0, 0.0, 2.0, 0.0, 2.0]);
        assert_eq!(count_heater_switches(&plan, now), 3);
    }

    // ── gate switch-count guard tests ─────────────────────────────────────────

    #[test]
    fn test_gate_rejects_noisier_plan_below_surcharge() {
        // current: 1 switch ([2,2,0,0]), new: 3 switches ([2,0,2,0]), extra=2.
        // improvement = 10.0 − 9.5 = 0.5; surcharge = 2 × 0.50 = 1.0 → 0.5 < 1.0 → reject.
        let now = fixed_now();
        let current = make_heater_plan(now, &[2.0, 2.0, 0.0, 0.0], 10.0);
        let new_plan = make_heater_plan(now, &[2.0, 0.0, 2.0, 0.0], 9.5);
        let adopt = evaluate_acceptance_gate(
            Some(&current),
            &new_plan,
            &PlanTrigger::Periodic,
            0.0,
            0.0,
            0.50,
            now,
        );
        assert!(
            !adopt,
            "improvement (0.5) below surcharge (1.0): must reject"
        );
    }

    #[test]
    fn test_gate_accepts_noisier_plan_above_surcharge() {
        // current: 1 switch, new: 3 switches (extra=2); but improvement=2.5 > surcharge=1.0 → accept.
        let now = fixed_now();
        let current = make_heater_plan(now, &[2.0, 2.0, 0.0, 0.0], 10.0);
        let new_plan = make_heater_plan(now, &[2.0, 0.0, 2.0, 0.0], 7.5);
        let adopt = evaluate_acceptance_gate(
            Some(&current),
            &new_plan,
            &PlanTrigger::Periodic,
            0.0,
            0.0,
            0.50,
            now,
        );
        assert!(
            adopt,
            "improvement (2.5) exceeds surcharge (1.0): must accept"
        );
    }

    #[test]
    fn test_gate_accepts_cleaner_plan_at_zero_surcharge() {
        // gate_switch_penalty_eur=0.0 disables switch guard; only cost improvement matters.
        // Use a positive threshold so the gate actually evaluates cost rather than short-circuiting.
        // current: 3 switches, cost 10.0; new: 1 switch, cost 8.5 → improvement 1.5 > threshold 1.0.
        // With penalty=0.0 the extra 2 switches of current don't add surcharge → accepted.
        let now = fixed_now();
        let current = make_heater_plan(now, &[2.0, 0.0, 2.0, 0.0], 10.0); // 3 switches
        let new_plan = make_heater_plan(now, &[2.0, 2.0, 0.0, 0.0], 8.5); // 1 switch, cheaper
        let adopt = evaluate_acceptance_gate(
            Some(&current),
            &new_plan,
            &PlanTrigger::Periodic,
            1.0, // active threshold: forces gate to evaluate rather than short-circuit
            0.0,
            0.0, // switch guard disabled
            now,
        );
        assert!(
            adopt,
            "penalty=0 disables surcharge: improvement (1.5) > threshold (1.0) must accept"
        );
    }

    #[test]
    fn test_gate_hard_trigger_ignores_surcharge() {
        // Hard trigger (UserRequest) always adopts regardless of switch surcharge.
        let now = fixed_now();
        let current = make_heater_plan(now, &[2.0, 2.0, 0.0, 0.0], 10.0); // 1 switch
        let new_plan = make_heater_plan(now, &[2.0, 0.0, 2.0, 0.0], 10.0); // 3 switches, same cost
        let adopt = evaluate_acceptance_gate(
            Some(&current),
            &new_plan,
            &PlanTrigger::UserRequest,
            0.0,
            0.0,
            0.50,
            now,
        );
        assert!(adopt, "hard trigger must bypass switch surcharge");
    }

    #[test]
    fn test_gate_decayed_accepts_despite_surcharge() {
        // Fully decayed plan is replaced unconditionally even with high switch surcharge.
        // Use make_plan_at with no slots so current_expired stays false and the test
        // exercises the fully_decayed path, not the current_expired path.
        let now = fixed_now();
        // current has no slots → current_expired=false; created 2 h ago, decay_s=3600 → fully_decayed=true.
        let current = make_plan_at(10.0, 0.0, now - Duration::seconds(7200));
        // new_plan has 3 switches (kws change: 0→2, 2→0, 0→2) vs current 0 → surcharge=3×0.50=1.50.
        // improvement = 10.0 − 10.0 = 0.0 < threshold(1.0) + surcharge(1.50) → would reject w/o decay.
        let new_plan = make_heater_plan(now, &[2.0, 0.0, 2.0, 0.0], 10.0);
        let adopt = evaluate_acceptance_gate(
            Some(&current),
            &new_plan,
            &PlanTrigger::Periodic,
            1.0,
            3600.0,
            0.50,
            now,
        );
        assert!(
            adopt,
            "fully decayed plan must be replaced despite switch surcharge"
        );
    }
}
