use chrono::{DateTime, Utc};
use std::sync::Arc;
use tracing::{debug, info, warn};

use crate::controller::milp_planner::asset_port::AssetMilpContext;
use crate::controller::simulator_port::SimSnapshot;
use crate::controller::trace::ControllerEvent;
use crate::controller::{SolveRequest, SolverPort};
use crate::entities::asset::PlanTrigger;
use crate::entities::asset_params::AssetParams;
use crate::entities::capacity::{AlertWindow, OadrCapacityState, SimpleWindow};
use crate::entities::device_session::{BaselineOverride, EvSession, HeaterTarget, ShiftableLoad};
use crate::entities::plan::Plan;
use crate::entities::planner_params::PlannerParams;
use crate::entities::sim_inject::SimInjectState;
use crate::entities::tariff_snapshot::{TariffSnapshot, TariffTimeSeries};
use crate::entities::PlannerObjective;
use crate::planner_events::{PlannerEvent, PlannerEventTx};
use crate::simulator::SimState;
use crate::state::AppState;

/// Clone the `SimState` under its Mutex, logging when the lock wait was long.
/// The clone releases the Mutex immediately — MILP solving takes 18–60s on
/// Pi4 ARM64 and must never hold the sim lock for its duration.
pub async fn clone_sim_snapshot(
    sim: &Arc<tokio::sync::Mutex<SimState>>,
    trigger_reason: &str,
) -> SimState {
    let lock_start = std::time::Instant::now();
    let snap = sim.lock().await.clone();
    let lock_ms = lock_start.elapsed().as_millis();
    if lock_ms > 500 {
        warn!(lock_wait_ms = lock_ms, trigger = %trigger_reason, "planner: sim lock wait was long");
    } else {
        debug!(lock_wait_ms = lock_ms, "planner: sim lock acquired");
    }
    snap
}

pub struct PlanningService;

/// Align a UTC timestamp down to the nearest `step_s`-second boundary.
///
/// All plan replans that fire within the same `step_s` window produce an identical
/// `now_aligned`, giving the acceptance gate slot-comparable plans and making successive
/// plans differ by exactly an integer multiple of `step_s` — never an arbitrary offset.
/// Uses `rem_euclid` so pre-epoch timestamps (negative unix) are handled correctly.
pub fn align_to_step(raw: DateTime<Utc>, step_s: u64) -> DateTime<Utc> {
    let ts = raw.timestamp();
    let step = step_s as i64;
    DateTime::<Utc>::from_timestamp(ts - ts.rem_euclid(step), 0)
        .expect("step-aligned timestamp is always valid")
}

/// Apply a pending one-shot PV-irradiance inject to a cloned sim snapshot, if the sim
/// tick hasn't applied it yet. No-op when `inject.pv_irradiance` is `None` — including
/// when the tick already applied and cleared it before this snapshot was cloned.
pub fn apply_pending_pv_inject(
    sim_snap: &mut SimState,
    inject: &SimInjectState,
    now: DateTime<Utc>,
) {
    if let Some(forced) = inject.pv_irradiance {
        use crate::assets::{AssetConfig, PvInverter};
        let natural = PvInverter::natural_irradiance_at(now);
        if let Some((_, AssetConfig::Pv(pv))) = sim_snap.find_asset_mut(crate::ids::ASSET_PV) {
            pv.irradiance_offset = forced - natural;
            pv.pv_alpha = inject.pv_irradiance_alpha;
        }
    }
}

/// Build per-asset MILP contexts from live simulator state, for the current plan cycle.
///
/// Must run before the blocking solve so asset states are captured at this instant, not
/// whenever the solver thread happens to get scheduled.
#[allow(clippy::too_many_arguments)]
pub fn build_asset_contexts(
    sim_snap: &SimState,
    n_slots: usize,
    cum_s: &[i64],
    now: DateTime<Utc>,
    ev_sess: Option<&EvSession>,
    heat_tgt: Option<&HeaterTarget>,
    asset_params: &[AssetParams],
    planner: &PlannerParams,
    lambda_sw: f64,
    heater_c_terminal_eur_kwh: f64,
    battery_c_terminal_eur_kwh: f64,
    heater_anchor: &[Option<f64>],
) -> Vec<Box<dyn AssetMilpContext>> {
    let min_ev_charge_kw = asset_params
        .iter()
        .find_map(|p| match p {
            AssetParams::Ev(e) => Some(e.min_charge_kw),
            _ => None,
        })
        .unwrap_or(0.0);

    sim_snap
        .iter_assets()
        .filter_map(|(entry, cfg)| {
            // Use per-asset c_terminal: heater and battery get their own coefficient;
            // EV gets 0.0 (deadline constraint handles the charging incentive).
            let c_terminal = match cfg {
                crate::assets::AssetConfig::Heater(_) => heater_c_terminal_eur_kwh,
                crate::assets::AssetConfig::Battery(_) => battery_c_terminal_eur_kwh,
                _ => 0.0,
            };
            cfg.build_milp_context(
                &entry.state,
                n_slots,
                cum_s,
                now,
                ev_sess,
                heat_tgt,
                min_ev_charge_kw,
                planner.v_ev_extra_eur_kwh,
                planner.v_ev_core_eur_kwh,
                planner.asap_lateness_eur_kwh_h,
                planner.v_ev_free_charge_eur_kwh,
                lambda_sw,
                c_terminal,
                if matches!(cfg, crate::assets::AssetConfig::Heater(_)) {
                    heater_anchor.to_vec()
                } else {
                    vec![]
                },
            )
        })
        .collect()
}

/// Assemble the fully-owned `SolveRequest` for one plan cycle from already-computed
/// per-cycle locals. Pure field marshalling — kept as a function (not an inline struct
/// literal in the task loop) so the loop body stays focused on orchestration.
#[allow(clippy::too_many_arguments)]
pub fn build_solve_request(
    asset_contexts: Vec<Box<dyn AssetMilpContext>>,
    assets: SimSnapshot,
    tariffs: TariffTimeSeries,
    capacity: OadrCapacityState,
    alert_windows: Vec<AlertWindow>,
    simple_windows: Vec<SimpleWindow>,
    planner: PlannerParams,
    grid_max_import_kw: f64,
    grid_max_export_kw: f64,
    asset_params: Vec<AssetParams>,
    now: DateTime<Utc>,
    trigger: PlanTrigger,
    ev_session: Option<EvSession>,
    heater_target: Option<HeaterTarget>,
    shiftable_loads: Vec<ShiftableLoad>,
    baseline_override: Option<BaselineOverride>,
    objective_override: Option<PlannerObjective>,
    pv_forecast_override: Option<f64>,
) -> SolveRequest {
    SolveRequest {
        asset_contexts,
        assets,
        tariffs,
        capacity,
        alert_windows,
        simple_windows,
        planner,
        grid_max_import_kw,
        grid_max_export_kw,
        asset_params,
        now,
        trigger,
        ev_session,
        heater_target,
        shiftable_loads,
        baseline_override,
        objective_override,
        pv_forecast_override,
    }
}

/// Precomputed planner-cycle inputs derived from tariffs, asset params, and the
/// previous plan/anchor state — everything the solver needs beyond live asset
/// state and simulator context. Pure; unit-testable without `AppState` or HiGHS.
pub struct PlanCycleInputs {
    pub tariff_ts: TariffTimeSeries,
    pub n_slots: usize,
    pub cum_s: Vec<i64>,
    pub lambda_sw: f64,
    pub heater_c_terminal_eur_kwh: f64,
    pub battery_c_terminal_eur_kwh: f64,
    pub heater_anchor: Vec<Option<f64>>,
}

pub fn build_plan_cycle_inputs(
    rates: &[TariffSnapshot],
    planner: &PlannerParams,
    asset_params: &[AssetParams],
    current_plan: Option<&Plan>,
    anchor_until: Option<DateTime<Utc>>,
    now: DateTime<Utc>,
) -> PlanCycleInputs {
    let tariff_ts = TariffTimeSeries::from_snapshots(rates);

    let n_slots: usize = planner.plan_zones.iter().map(|z| z.slots).sum();
    let cum_s: Vec<i64> = {
        let mut v = Vec::with_capacity(n_slots + 1);
        v.push(0i64);
        for zone in &planner.plan_zones {
            for _ in 0..zone.slots {
                v.push(v.last().unwrap() + zone.step_s as i64);
            }
        }
        v
    };

    let heater_anchor = build_heater_anchor(current_plan, anchor_until, now, n_slots);

    let lambda_sw = asset_params
        .iter()
        .find_map(|p| match p {
            AssetParams::Heater(h) => Some(h.switching_penalty_eur),
            _ => None,
        })
        .unwrap_or(0.0);

    let avg_imp_eur_kwh = {
        let total: f64 = (0..n_slots)
            .map(|t| {
                let slot_t = now + chrono::Duration::seconds(cum_s[t]);
                tariff_ts
                    .import_eur_kwh
                    .interpolate_at(slot_t)
                    .unwrap_or(0.25)
            })
            .sum();
        if n_slots > 0 {
            total / n_slots as f64
        } else {
            0.25
        }
    };

    let heater_c_terminal_eur_kwh = asset_params
        .iter()
        .find_map(|p| match p {
            AssetParams::Heater(h) => Some(
                h.c_terminal_eur_kwh
                    .unwrap_or(avg_imp_eur_kwh + planner.c_ctrl_imp_malus_eur_kwh),
            ),
            _ => None,
        })
        .unwrap_or(0.0);
    let battery_c_terminal_eur_kwh = asset_params
        .iter()
        .find_map(|p| match p {
            AssetParams::Battery(b) => Some(
                b.c_terminal_eur_kwh
                    .unwrap_or(avg_imp_eur_kwh * b.round_trip_efficiency),
            ),
            _ => None,
        })
        .unwrap_or(0.0);

    PlanCycleInputs {
        tariff_ts,
        n_slots,
        cum_s,
        lambda_sw,
        heater_c_terminal_eur_kwh,
        battery_c_terminal_eur_kwh,
        heater_anchor,
    }
}

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
    // Only anchor an active heating block. When the heater is off in the first future slot
    // (kw0 ≈ 0 — including fallback plans that have no heater data at all), anchoring all
    // subsequent slots to "off" forces the dynamics to drain the tank below its domain lower
    // bound, making the next MILP infeasible at presolve. No anchor is needed for off-state:
    // the switching penalty in Phase 2 already discourages rapid re-cycling.
    if kw0 < 0.1 {
        return None;
    }
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
}

/// Count heater relay switches in `plan` for all slots starting at or after `now`.
///
/// A switch is a transition where the heater power changes by more than 0.1 kW between
/// consecutive future slots. Past slots are excluded so this reflects the remaining
/// switching burden of the plan from the current moment onward.
pub fn count_heater_switches(plan: &Plan, now: DateTime<Utc>) -> f64 {
    // zone_a_step_s is the reference slot width for switch weighting.
    // Falls back to horizon.step_size_s when no zones are present (legacy/test plans),
    // which keeps the per-switch weight at 1.0 for uniform-step plans.
    let zone_a_step_s = plan
        .horizon
        .zones
        .first()
        .map(|z| z.step_s as f64)
        .unwrap_or(plan.horizon.step_size_s as f64);
    let mut count = 0.0f64;
    let mut prev: Option<f64> = None;
    for slot in plan.all_slots().filter(|s| s.start >= now) {
        let kw = slot
            .planned_kw_by_asset
            .get("heater")
            .copied()
            .unwrap_or(0.0);
        if prev.is_some_and(|p| (p - kw).abs() > 0.1) {
            let slot_step_s = (slot.end - slot.start).num_seconds() as f64;
            count += slot_step_s / zone_a_step_s;
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
        let extra =
            (count_heater_switches(new_plan, now) - count_heater_switches(current, now)).max(0.0);
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
    /// Run the MILP solver on a blocking thread via the injected `SolverPort`,
    /// awaiting completion. Called by `tasks/planning.rs` once the cycle's
    /// `SolveRequest` is built — mirrors `adopt_if_warranted`'s role as the
    /// post-/pre-solve service-layer step.
    pub async fn solve_plan(solver: &Arc<dyn SolverPort>, req: SolveRequest) -> Plan {
        let solver = solver.clone();
        tokio::task::spawn_blocking(move || solver.solve(req))
            .await
            .expect("planner task panicked")
    }

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
        state.push_controller_event(plan_cycle_event).await;

        PlanCycleResult { adopted, plan }
    }
}

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration, TimeZone};
    use uuid::Uuid;

    #[test]
    fn test_align_to_step_rounds_down() {
        let make = |h: u32, m: u32, s: u32| Utc.with_ymd_and_hms(2026, 4, 11, h, m, s).unwrap();

        // step_s = 600 (10 min)
        assert_eq!(align_to_step(make(14, 23, 47), 600), make(14, 20, 0));
        assert_eq!(align_to_step(make(14, 20, 0), 600), make(14, 20, 0)); // already aligned
        assert_eq!(align_to_step(make(14, 29, 59), 600), make(14, 20, 0));

        // step_s = 300 (5 min)
        assert_eq!(align_to_step(make(14, 23, 47), 300), make(14, 20, 0));
        assert_eq!(align_to_step(make(14, 25, 0), 300), make(14, 25, 0)); // already aligned
        assert_eq!(align_to_step(make(14, 24, 59), 300), make(14, 20, 0));

        // result timestamp is always an exact multiple of step_s from epoch
        for step_s in [300u64, 600, 900, 1800] {
            let raw = make(14, 23, 47);
            let aligned = align_to_step(raw, step_s);
            assert_eq!(
                aligned.timestamp() % step_s as i64,
                0,
                "step_s={step_s}: aligned timestamp not a multiple of step_s"
            );
        }
    }

    #[test]
    fn test_apply_pending_pv_inject_noop_when_no_pv_asset() {
        // No PV asset present (or no pending inject) — must not panic, must be a no-op.
        let mut sim_snap: SimState = serde_json::from_value(serde_json::json!({
            "asset_configs": [],
            "assets": [],
            "grid": {
                "net_power_w": 0.0, "import_w": 0.0, "export_w": 0.0,
                "voltage_v": 0.0, "import_kwh": 0.0, "export_kwh": 0.0
            },
            "last_tick": Utc::now().to_rfc3339()
        }))
        .expect("minimal SimState must deserialize");
        let inject = SimInjectState::default();
        apply_pending_pv_inject(&mut sim_snap, &inject, Utc::now());
        // No panic is the assertion — nothing else observable without a PV asset.
    }

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
        // When heater is off in the first slot, return None — anchoring heater-off
        // state causes the MILP to fail (forced off + dynamics → tank below domain lb).
        let plan = make_plan_with_heater_slots(now, step_s, &[0.0, 0.0, 2.0, 2.0]);
        let result = heater_block_end(&plan, now);
        assert_eq!(result, None, "off-block must not produce an anchor");
    }

    #[test]
    fn test_heater_block_end_fallback_plan_returns_none() {
        let now = fixed_now();
        // Fallback plan (MILP failed): all slots have no heater data → defaults to 0.0.
        // Must return None; returning Some(horizon_end) would anchor all 288 slots to
        // heater-off, forcing e_tank below its domain lower bound → MILP infeasible.
        let plan = make_plan_with_heater_slots(now, 300, &[0.0; 288]);
        let result = heater_block_end(&plan, now);
        assert_eq!(
            result, None,
            "fallback (all-zero) plan must not produce an anchor"
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
        assert_eq!(count_heater_switches(&plan, now), 0.0);
    }

    #[test]
    fn test_count_switches_one_block() {
        // All future slots at the same kW → no tier changes → 0 switches.
        let now = fixed_now();
        let plan = make_plan_with_heater_slots(now, 1200, &[2.0, 2.0, 2.0, 2.0]);
        assert_eq!(count_heater_switches(&plan, now), 0.0);
    }

    #[test]
    fn test_count_switches_filters_past_slots() {
        // 6-slot plan: slots 0–1 start before `now`, slots 2–5 start at/after `now`.
        // Past slots (0.0, 0.0) are filtered; future slots (0.0, 2.0, 0.0, 2.0) give 3 transitions.
        let now = fixed_now();
        let start = now - Duration::seconds(2 * 1200);
        let plan = make_plan_with_heater_slots(start, 1200, &[0.0, 0.0, 0.0, 2.0, 0.0, 2.0]);
        assert_eq!(count_heater_switches(&plan, now), 3.0);
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

    // ── build_plan_cycle_inputs tests ─────────────────────────────────────────

    use crate::entities::asset_params::{BatteryParams, HeaterParams};
    use crate::entities::plan::PlanZone;

    fn tariff_snapshot(
        start: DateTime<Utc>,
        end: DateTime<Utc>,
        import_eur_kwh: f64,
    ) -> TariffSnapshot {
        TariffSnapshot {
            interval_start: start,
            interval_end: end,
            import_tariff_eur_kwh: Some(import_eur_kwh),
            export_tariff_eur_kwh: None,
            co2_g_kwh: None,
        }
    }

    #[test]
    fn test_build_plan_cycle_inputs_multi_zone_n_slots_and_cum_s() {
        let now = fixed_now();
        let planner = PlannerParams {
            plan_zones: vec![
                PlanZone {
                    step_s: 300,
                    slots: 4,
                },
                PlanZone {
                    step_s: 900,
                    slots: 2,
                },
            ],
            ..PlannerParams::default()
        };
        let inputs = build_plan_cycle_inputs(&[], &planner, &[], None, None, now);

        assert_eq!(inputs.n_slots, 6, "4 zone-A slots + 2 zone-B slots");
        assert_eq!(
            inputs.cum_s,
            vec![0, 300, 600, 900, 1200, 2100, 3000],
            "cum_s must accumulate zone-A step (300s x4) then zone-B step (900s x2)"
        );
    }

    #[test]
    fn test_build_plan_cycle_inputs_lambda_sw_default_when_no_heater() {
        let now = fixed_now();
        let planner = PlannerParams::default();
        let inputs = build_plan_cycle_inputs(&[], &planner, &[], None, None, now);
        assert_eq!(
            inputs.lambda_sw, 0.0,
            "no heater in asset_params → lambda_sw defaults to 0.0"
        );
    }

    #[test]
    fn test_build_plan_cycle_inputs_lambda_sw_from_heater_params() {
        let now = fixed_now();
        let planner = PlannerParams::default();
        let asset_params = vec![AssetParams::Heater(HeaterParams {
            switching_penalty_eur: 0.05,
            ..HeaterParams::default()
        })];
        let inputs = build_plan_cycle_inputs(&[], &planner, &asset_params, None, None, now);
        assert!(
            (inputs.lambda_sw - 0.05).abs() < 1e-9,
            "lambda_sw must be read from HeaterParams.switching_penalty_eur, got {}",
            inputs.lambda_sw
        );
    }

    #[test]
    fn test_build_plan_cycle_inputs_avg_tariff_and_terminal_reward_auto_computed() {
        let now = fixed_now();
        let planner = PlannerParams {
            plan_zones: vec![PlanZone {
                step_s: 3600,
                slots: 2,
            }],
            c_ctrl_imp_malus_eur_kwh: 0.10,
            ..PlannerParams::default()
        };
        // Two 1h slots: [now, now+1h) at 0.20 €/kWh, [now+1h, now+2h) at 0.30 €/kWh.
        let rates = vec![
            tariff_snapshot(now, now + Duration::hours(1), 0.20),
            tariff_snapshot(now + Duration::hours(1), now + Duration::hours(2), 0.30),
        ];
        let asset_params = vec![
            AssetParams::Heater(HeaterParams {
                c_terminal_eur_kwh: None, // auto-compute
                ..HeaterParams::default()
            }),
            AssetParams::Battery(BatteryParams {
                c_terminal_eur_kwh: None, // auto-compute
                round_trip_efficiency: 0.90,
                ..BatteryParams::default()
            }),
        ];
        let inputs = build_plan_cycle_inputs(&rates, &planner, &asset_params, None, None, now);

        // avg_imp_eur_kwh = (0.20 + 0.30) / 2 = 0.25
        let expected_heater = 0.25 + 0.10; // avg + c_ctrl_imp_malus_eur_kwh
        let expected_battery = 0.25 * 0.90; // avg * round_trip_efficiency
        assert!(
            (inputs.heater_c_terminal_eur_kwh - expected_heater).abs() < 1e-9,
            "expected heater terminal reward {expected_heater}, got {}",
            inputs.heater_c_terminal_eur_kwh
        );
        assert!(
            (inputs.battery_c_terminal_eur_kwh - expected_battery).abs() < 1e-9,
            "expected battery terminal reward {expected_battery}, got {}",
            inputs.battery_c_terminal_eur_kwh
        );
    }

    #[test]
    fn test_build_plan_cycle_inputs_terminal_reward_profile_override_wins() {
        let now = fixed_now();
        let planner = PlannerParams {
            plan_zones: vec![PlanZone {
                step_s: 3600,
                slots: 1,
            }],
            ..PlannerParams::default()
        };
        let rates = vec![tariff_snapshot(now, now + Duration::hours(1), 0.20)];
        let asset_params = vec![
            AssetParams::Heater(HeaterParams {
                c_terminal_eur_kwh: Some(0.0), // explicit override disables auto-compute
                ..HeaterParams::default()
            }),
            AssetParams::Battery(BatteryParams {
                c_terminal_eur_kwh: Some(0.42),
                ..BatteryParams::default()
            }),
        ];
        let inputs = build_plan_cycle_inputs(&rates, &planner, &asset_params, None, None, now);

        assert_eq!(
            inputs.heater_c_terminal_eur_kwh, 0.0,
            "profile override Some(0.0) must disable auto-computed heater reward"
        );
        assert!(
            (inputs.battery_c_terminal_eur_kwh - 0.42).abs() < 1e-9,
            "profile override must take precedence over auto-computed battery reward"
        );
    }

    #[test]
    fn test_build_plan_cycle_inputs_no_battery_or_heater_defaults_to_zero() {
        let now = fixed_now();
        let planner = PlannerParams::default();
        let inputs = build_plan_cycle_inputs(&[], &planner, &[], None, None, now);
        assert_eq!(inputs.heater_c_terminal_eur_kwh, 0.0);
        assert_eq!(inputs.battery_c_terminal_eur_kwh, 0.0);
    }

    #[test]
    fn test_build_plan_cycle_inputs_delegates_heater_anchor() {
        // No current plan / no anchor_until → build_heater_anchor returns all-None,
        // confirming build_plan_cycle_inputs actually wires the call through.
        let now = fixed_now();
        let planner = PlannerParams {
            plan_zones: vec![PlanZone {
                step_s: 1200,
                slots: 4,
            }],
            ..PlannerParams::default()
        };
        let inputs = build_plan_cycle_inputs(&[], &planner, &[], None, None, now);
        assert_eq!(inputs.heater_anchor.len(), 4);
        assert!(inputs.heater_anchor.iter().all(|v| v.is_none()));
    }
}
