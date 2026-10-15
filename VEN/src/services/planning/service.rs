//! `PlanningService`'s solve/adopt methods — split out of `planning/mod.rs` to stay
//! under the 500-production-line cap (R-40 watch-list item, crossed by the R-29
//! `solve_plan` panic-fallback fix).

use chrono::{DateTime, Utc};
use std::sync::Arc;
use tracing::info;

use crate::controller::trace::ControllerEvent;
use crate::controller::{SolveRequest, SolverPort};
use crate::entities::asset::PlanTrigger;
use crate::entities::plan::Plan;
use crate::entities::PlannerObjective;
use crate::planner_events::{PlannerEvent, PlannerEventTx};
use crate::state::AppState;

use super::{evaluate_acceptance_gate, heater_block_ends, PlanCycleResult, PlanningService};

impl PlanningService {
    /// Run the MILP solver on a blocking thread via the injected `SolverPort`,
    /// awaiting completion. Called by `tasks/planning.rs` once the cycle's
    /// `SolveRequest` is built — mirrors `adopt_if_warranted`'s role as the
    /// post-/pre-solve service-layer step.
    pub async fn solve_plan(solver: &Arc<dyn SolverPort>, req: SolveRequest) -> Plan {
        let solver = solver.clone();
        // Clone fallback fields before `req` moves into the closure below — if
        // solver.solve panics, req is unrecoverable (R-29: JoinError -> fallback_plan()
        // instead of re-panicking, same as a real solve error in run_planner's Err arm).
        let fallback_planner = req.planner.clone();
        let fallback_now = req.now;
        let fallback_trigger = req.trigger.clone();
        let fallback_objective = req.objective_override.unwrap_or(req.planner.objective);
        tokio::task::spawn_blocking(move || {
            let plan = solver.solve(req);
            // Return this blocking thread's freed heap pages to the OS immediately.
            // Without this, glibc keeps a large solve's dirtied pages mapped for reuse,
            // so RSS ratchets up to the largest solve's high-water mark and never comes
            // back down between cycles (observed: harder-solving VENs sitting 5-10x
            // above trivial ones with no leak, just un-trimmed peak working set).
            #[cfg(all(target_os = "linux", target_env = "gnu"))]
            unsafe {
                libc::malloc_trim(0);
            }
            plan
        })
        .await
        .unwrap_or_else(|join_err| {
            tracing::error!(error = %join_err, "planner task panicked; returning fallback plan");
            crate::controller::milp_planner::fallback_plan(
                &fallback_planner,
                fallback_now,
                fallback_trigger,
                &[],
                None,
                format!("planner task panicked: {join_err}"),
                fallback_objective,
            )
        })
    }

    /// Post-solve step: evaluate acceptance gate, adopt or reject, emit events, update state.
    ///
    /// Called by `tasks/planning.rs` after `spawn_blocking` returns the solved Plan.
    /// Accepts `objective` explicitly since it lives in `AppCtx`, not `AppState`.
    #[allow(clippy::too_many_arguments)]
    pub async fn adopt_if_warranted(
        mut plan: Plan,
        trigger: &PlanTrigger,
        trigger_reason: &str,
        trigger_event_ids: &[String],
        threshold_eur: f64,
        decay_s: f64,
        gate_switch_penalty_eur: f64,
        heater_stage_sizes_kw: &std::collections::BTreeMap<String, f64>,
        solver_ms: u64,
        objective: PlannerObjective,
        state: &AppState,
        event_tx: &PlannerEventTx,
        now: DateTime<Utc>,
    ) -> PlanCycleResult {
        // GB-25: stamp the real solve time before either the SSE emit or adoption below reads
        // it, so both the live event and the persisted Plan/plan-history row agree. Mirrors how
        // `results.rs` already stamps `mip_gap_target` at construction time.
        plan.solver_ms = Some(solver_ms);

        // Emit PlanReady before gate evaluation so SSE clients always receive it.
        let _ = event_tx.send(PlannerEvent::PlanReady {
            plan_id: plan.id,
            objective,
            solver_ms,
            objective_eur: plan.objective_eur,
            friction_eur: plan.friction_eur,
            solve_status: plan.solve_status,
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
            heater_stage_sizes_kw,
            now,
        );

        let slot_count = plan.slots.len();
        if adopted {
            info!(trigger = %trigger_reason, slot_count, "planner: plan adopted");
            state.set_active_plan(Some(plan.clone())).await;
            let anchor = heater_block_ends(&plan, now, heater_stage_sizes_kw.keys());
            state.set_anchor_until(anchor).await;

            // §5.5: reset the arbiter's residual accumulator on every plan
            // adoption (any trigger, not just hard ones) and re-snapshot each
            // SoC-coupled asset's available capacity as the new baseline.
            if let Some(sim_snap) = state.sim().await {
                let new_capacities: std::collections::HashMap<String, f64> = sim_snap
                    .assets
                    .iter()
                    .filter(|(_, snap)| snap.has_state_of_charge())
                    .map(|(asset_id, snap)| {
                        let capacity_kwh = snap
                            .available_charge_kwh
                            .unwrap_or(0.0)
                            .max(snap.available_discharge_kwh.unwrap_or(0.0));
                        (asset_id.clone(), capacity_kwh)
                    })
                    .collect();
                state.reset_residual(&new_capacities).await;
            }
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
            event_ids: trigger_event_ids.to_vec(),
            total_slots: slot_count,
        };
        state.push_controller_event(plan_cycle_event).await;

        PlanCycleResult { adopted, plan }
    }
}
