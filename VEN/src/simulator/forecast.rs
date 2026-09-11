//! Plan-driven forward re-simulation, shared by
//! `controller::capacity_headroom::compute_site_headroom_forecast` and
//! `resolve_plan_state_at` below. Re-simulates each asset forward from its
//! REAL current state — never `Plan.planned_state_by_asset`, a stale
//! solve-time-only snapshot — driven by the active plan's own
//! already-decided setpoint schedule. `pv-competence-consolidation` sections
//! 4/5 retired this module's own separate PV-frame/`pv_ceiling_kw` path:
//! `PvInverter` now implements `Asset::simulate_forward` itself
//! (weather/decay-aware), so PV flows through `simulated_trajectory` like
//! every other asset kind — no more PV-specific handling here.

use std::collections::HashMap;

use chrono::{DateTime, Utc};

use crate::assets::{Asset, AssetHandle, AssetState, Trajectory};
use crate::entities::plan::{Plan, PlanTimeSlot};

use super::SimState;

/// Re-simulate an asset forward from its REAL current state, driven by the
/// plan's own `planned_kw_by_asset` schedule for this asset — one setpoint
/// per remaining slot, giving one projected state per slot start (see
/// `Asset::simulate_forward`'s doc comment: each `TrajectoryPoint` pairs the
/// state BEFORE that slot's step with the setpoint driving it).
///
/// Shared by `controller::capacity_headroom::compute_site_headroom_forecast`
/// (capability-per-slot) and `resolve_plan_state_at` (state-at-a-single-`t1`)
/// so there is exactly one place that runs this simulation —
/// `planstate-t1-resolver`'s D1: two independent implementations of "the
/// plan-driven forecast" is exactly what this master plan exists to remove.
///
/// The schedule carries one extra trailing sentinel point beyond
/// `future_slots` itself, holding the last slot's own setpoint for its own
/// real duration (`last_slot.end`). Without it, `Asset::simulate_forward`'s
/// default body never applies a real, non-zero-`dt` step for the *last*
/// setpoint in any schedule — its trailing point is a zero-duration
/// re-evaluation of whatever state came before, so the last remaining
/// slot's own committed action would otherwise never be reflected in any
/// returned point, for any caller, regardless of how many slots there are
/// (confirmed via review while implementing `resolve_plan_state_at`, which
/// needs exactly that "after the last slot completes" state; `future_slots`
/// itself is bounds-checked by every existing caller, so this extra point
/// is silently ignored where it isn't wanted).
///
/// `pub(crate)` (not private) so `controller::capacity_headroom`
/// (`unified-capacity-envelope-engine`, Spec E) can compute each asset's
/// per-slot trajectory once and read every slot's point off it, rather than
/// calling `resolve_plan_state_at` once per slot (which would redundantly
/// recompute this same walk for every slot requested).
pub(crate) fn simulated_trajectory(
    entry: &super::AssetEntry,
    cfg: &dyn Asset,
    future_slots: &[&PlanTimeSlot],
) -> Trajectory {
    let handle = AssetHandle {
        config: cfg,
        id: &entry.id,
        state: &entry.state,
        history: &entry.history,
    };
    let mut schedule: Vec<(DateTime<Utc>, f64)> = future_slots
        .iter()
        .map(|s| {
            (
                s.start,
                s.planned_kw_by_asset.get(&entry.id).copied().unwrap_or(0.0),
            )
        })
        .collect();
    if let (Some(last_slot), Some(&(_, last_kw))) = (future_slots.last(), schedule.last()) {
        schedule.push((last_slot.end, last_kw));
    }
    handle.simulate_forward(&entry.state, &schedule)
}

/// "If the active plan runs as intended until `t1`, what state is each asset
/// in?" (`planstate-t1-resolver`, Spec D of the asset-max-power-forecast
/// master plan). Feeds `assets::asset_max_power`'s (Spec C) starting state —
/// built once here and reused by Spec E, not re-derived per caller.
///
/// `t1` at or before `now` returns every asset's live `SimState` value
/// unchanged, with no simulation — the one point where ground truth exists
/// must not carry forecast error. For a future `t1`, non-PV assets reuse
/// `simulated_trajectory`'s per-slot state (the same computation
/// `compute_site_headroom_forecast` already runs), picked at the latest remaining
/// slot with `start <= t1` — `t1` landing between two slot boundaries snaps
/// down to the earlier one (no interpolation); `t1` past the plan's last
/// remaining slot returns that last slot's state rather than panicking or
/// extrapolating.
///
/// PV is the one exception: it always returns its current live state,
/// regardless of `t1`. `PvState::curtailment_source` reflects whatever
/// external decision (manual command, plan, capacity limiter, arbiter,
/// comms-loss) is active *right now* — no model anywhere in this codebase
/// forecasts how it will change, and running it through `simulate_forward`
/// would only replay today's frozen irradiance/weather inputs, not a real
/// forecast. This is a documented scope limit, not an oversight — see
/// `openspec/changes/planstate-t1-resolver/design.md`'s Risks section
/// (deleted once this change lands; see `docs/history/project_journal.md`
/// for the design record) before assuming this resolves more than it does.
///
/// Not yet called from production code -- this change (`planstate-t1-resolver`)
/// only builds and unit-tests the resolver; wiring it (and Spec C's
/// `asset_max_power`) into the unified capacity/envelope engine is Spec E's job.
#[allow(dead_code)]
pub fn resolve_plan_state_at(
    sim: &SimState,
    plan: &Plan,
    t1: DateTime<Utc>,
    now: DateTime<Utc>,
) -> HashMap<String, AssetState> {
    let live_snapshot = || {
        sim.iter_assets()
            .map(|(entry, _cfg)| (entry.id.clone(), entry.state.clone()))
            .collect()
    };

    if t1 <= now {
        return live_snapshot();
    }

    let future_slots: Vec<&PlanTimeSlot> = plan.all_slots().filter(|s| s.start >= now).collect();
    if future_slots.is_empty() {
        return live_snapshot();
    }

    // One boundary per `future_slots` entry (its `start`, matching
    // `traj.points[i]`'s "state before this slot's own action" semantics)
    // plus one trailing boundary at the last slot's own `end` — matching
    // `simulated_trajectory`'s appended sentinel point, the one point that
    // genuinely reflects the state AFTER the last remaining slot's action
    // completes. Without this trailing boundary, `t1` at or past the plan's
    // true horizon end would resolve to the second-to-last point instead
    // (the last slot's own action still uncommitted), silently
    // under-reporting the plan's real effect.
    let boundaries: Vec<DateTime<Utc>> = future_slots
        .iter()
        .map(|s| s.start)
        .chain(future_slots.last().map(|s| s.end))
        .collect();
    sim.iter_assets()
        .map(|(entry, cfg)| {
            if cfg.asset_type_str() == "pv" {
                return (entry.id.clone(), entry.state.clone());
            }
            let traj = simulated_trajectory(entry, cfg, &future_slots);
            let idx = boundaries.iter().rposition(|&b| b <= t1).unwrap_or(0);
            let state = traj
                .points
                .get(idx)
                .map(|p| p.state.clone())
                .unwrap_or_else(|| entry.state.clone());
            (entry.id.clone(), state)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entities::asset::PlanTrigger;
    use crate::entities::asset_params::{AssetParams, BaseLoadParams, BatteryParams, PvParams};
    use crate::entities::plan::{Plan, PlanTimeSlot, PlanZone, PlanningHorizon, SolveStatus};
    use crate::entities::planner_params::PlannerObjective;
    use crate::ids::{ASSET_BASE_LOAD, ASSET_BATTERY, ASSET_PV};
    use chrono::Duration;
    use std::collections::HashMap as Map;
    use uuid::Uuid;

    fn make_plan(step_s: u64, slots: usize, start: DateTime<Utc>) -> Plan {
        let horizon = PlanningHorizon {
            start_time: start,
            end_time: start + Duration::seconds((step_s * slots as u64) as i64),
            step_size_s: step_s,
            num_steps: slots,
            far_horizon: start + Duration::seconds((step_s * slots as u64) as i64),
            zones: vec![PlanZone { step_s, slots }],
        };
        let plan_slots: Vec<PlanTimeSlot> = (0..slots)
            .map(|i| PlanTimeSlot {
                slot_index: i,
                start: start + Duration::seconds((step_s * i as u64) as i64),
                end: start + Duration::seconds((step_s * (i + 1) as u64) as i64),
                import_tariff_eur_kwh: 0.25,
                export_tariff_eur_kwh: 0.08,
                co2_g_kwh: 300.0,
                grid_effective_cost: 0.25,
                marginal_cost_import_eur_per_kwh: 0.25,
                marginal_cost_export_eur_per_kwh: 0.25,
                rate_estimated: false,
                import_cap_kw: 25.0,
                export_cap_kw: 10.0,
                allocations: vec![],
                pv_forecast_kw: 0.0,
                pv_used_kw: 0.0,
                baseline_kw: 0.0,
                surplus_available_kw: 0.0,
                net_import_kw: 0.0,
                net_export_kw: 0.0,
                import_flexibility_kw: 0.0,
                export_flexibility_kw: 0.0,
                planned_kw_by_asset: Map::new(),
                planned_state_by_asset: Map::new(),
                bat_charge_kw: 0.0,
                bat_discharge_kw: 0.0,
            })
            .collect();
        Plan {
            id: Uuid::new_v4(),
            created_at: start,
            trigger: PlanTrigger::Periodic,
            objective: PlannerObjective::MinCost,
            horizon,
            slots: plan_slots,
            objective_eur: 0.0,
            friction_eur: 0.0,
            cost_breakdown: Default::default(),
            soc_trajectory_kwh: vec![],
            summary: Default::default(),
            envelopes: vec![],
            warnings: vec![],
            solve_status: SolveStatus::Optimal,
            penalty_rules_active: vec![],
            solver_ms: None,
            mip_gap_target: None,
        }
    }

    // ── resolve_plan_state_at (planstate-t1-resolver, Spec D) ───────────────

    fn battery_soc(state: &AssetState) -> f64 {
        match state {
            AssetState::Battery(s) => s.soc,
            other => panic!("expected AssetState::Battery, got {other:?}"),
        }
    }

    #[test]
    fn t1_at_or_before_now_returns_live_state_unchanged() {
        // A nonzero planned charge setpoint would move SoC if simulated --
        // t1 <= now must skip simulation entirely and hand back the live
        // value untouched.
        let now = Utc::now();
        let sim = SimState::from_params(
            &[AssetParams::Battery(BatteryParams {
                id: ASSET_BATTERY.to_string(),
                capacity_kwh: 10.0,
                max_charge_kw: 5.0,
                max_discharge_kw: 5.0,
                initial_soc: 0.5,
                round_trip_efficiency: 1.0,
                min_soc: 0.1,
                c_terminal_eur_kwh: Some(0.0),
            })],
            now,
        );
        let mut plan = make_plan(900, 4, now);
        for slot in &mut plan.slots {
            slot.planned_kw_by_asset
                .insert(ASSET_BATTERY.to_string(), 5.0);
        }

        let at_now = resolve_plan_state_at(&sim, &plan, now, now);
        let in_the_past = resolve_plan_state_at(&sim, &plan, now - Duration::seconds(60), now);

        assert_eq!(battery_soc(&at_now[ASSET_BATTERY]), 0.5);
        assert_eq!(battery_soc(&in_the_past[ASSET_BATTERY]), 0.5);
    }

    #[test]
    fn battery_state_at_a_future_slot_matches_direct_simulate_forward() {
        let now = Utc::now();
        let sim = SimState::from_params(
            &[AssetParams::Battery(BatteryParams {
                id: ASSET_BATTERY.to_string(),
                capacity_kwh: 10.0,
                max_charge_kw: 5.0,
                max_discharge_kw: 5.0,
                initial_soc: 0.5,
                round_trip_efficiency: 1.0,
                min_soc: 0.1,
                c_terminal_eur_kwh: Some(0.0),
            })],
            now,
        );
        let mut plan = make_plan(900, 4, now); // 4 x 15-min slots
        for slot in &mut plan.slots {
            slot.planned_kw_by_asset
                .insert(ASSET_BATTERY.to_string(), 5.0);
        }
        let t1 = plan.slots[2].start;

        let resolved = resolve_plan_state_at(&sim, &plan, t1, now);

        // Direct comparison: the same schedule simulated_trajectory itself
        // uses, run by hand via simulate_forward.
        let (entry, cfg) = sim.find_asset(ASSET_BATTERY).unwrap();
        let handle = AssetHandle {
            config: cfg,
            id: &entry.id,
            state: &entry.state,
            history: &entry.history,
        };
        let schedule: Vec<(DateTime<Utc>, f64)> =
            plan.slots.iter().map(|s| (s.start, 5.0)).collect();
        let traj = handle.simulate_forward(&entry.state, &schedule);
        let expected_soc = battery_soc(&traj.points[2].state);

        assert_eq!(
            battery_soc(&resolved[ASSET_BATTERY]),
            expected_soc,
            "resolver must reuse the same computation as a direct simulate_forward call, not a second implementation"
        );
    }

    #[test]
    fn base_load_is_included_even_though_it_has_no_flexibility() {
        // compute_site_headroom_forecast/compute_site_capacity_curve
        // deliberately exclude base_load (it never contributes flexibility)
        // -- but resolve_plan_state_at answers "what state is asset X in",
        // which is a well-defined question for base_load too (asset_max_power's
        // own roster includes it), so it must not be silently dropped here.
        let now = Utc::now();
        let sim = SimState::from_params(
            &[AssetParams::BaseLoad(BaseLoadParams {
                baseline_kw: 0.7,
                ..Default::default()
            })],
            now,
        );
        let plan = make_plan(900, 2, now);
        let t1 = plan.slots[1].start;

        let resolved = resolve_plan_state_at(&sim, &plan, t1, now);

        assert!(
            resolved.contains_key(ASSET_BASE_LOAD),
            "base_load must be present in the resolved state map"
        );
        match &resolved[ASSET_BASE_LOAD] {
            AssetState::BaseLoad(s) => assert_eq!(s.actual_power_kw, 0.7),
            other => panic!("expected AssetState::BaseLoad, got {other:?}"),
        }
    }

    #[test]
    fn pv_state_at_a_future_t1_equals_its_current_live_state() {
        let now = Utc::now();
        let sim = SimState::from_params(
            &[AssetParams::Pv(PvParams {
                id: ASSET_PV.to_string(),
                rated_kw: 5.0,
                inverter_max_kw: 5.0,
                co2_g_kwh: 0.0,
            })],
            now,
        );
        let plan = make_plan(900, 4, now);
        let t1 = plan.slots[3].start;

        let resolved = resolve_plan_state_at(&sim, &plan, t1, now);

        let (live_entry, _cfg) = sim.find_asset(ASSET_PV).unwrap();
        let (live_power, live_source) = match &live_entry.state {
            AssetState::Pv(s) => (s.actual_power_kw, s.curtailment_source),
            other => panic!("expected AssetState::Pv, got {other:?}"),
        };
        let (resolved_power, resolved_source) = match &resolved[ASSET_PV] {
            AssetState::Pv(s) => (s.actual_power_kw, s.curtailment_source),
            other => panic!("expected AssetState::Pv, got {other:?}"),
        };
        assert_eq!(
            resolved_power, live_power,
            "PV's resolved state at a future t1 must equal its current live state"
        );
        assert_eq!(resolved_source, live_source);
    }

    #[test]
    fn t1_past_the_last_slot_returns_the_last_available_state() {
        let now = Utc::now();
        let sim = SimState::from_params(
            &[AssetParams::Battery(BatteryParams {
                id: ASSET_BATTERY.to_string(),
                capacity_kwh: 10.0,
                max_charge_kw: 5.0,
                max_discharge_kw: 5.0,
                initial_soc: 0.5,
                round_trip_efficiency: 1.0,
                min_soc: 0.1,
                c_terminal_eur_kwh: Some(0.0),
            })],
            now,
        );
        let mut plan = make_plan(900, 4, now);
        for slot in &mut plan.slots {
            slot.planned_kw_by_asset
                .insert(ASSET_BATTERY.to_string(), 5.0);
        }
        // The true horizon end is the last slot's own END, not its start --
        // between those two timestamps there's a whole slot's worth of real,
        // distinct information (the last slot's own committed action), so
        // that's the earliest point "past the last slot" genuinely means.
        let last_slot_end = plan.slots.last().unwrap().end;
        let far_future = last_slot_end + Duration::hours(10);

        let at_horizon_end = resolve_plan_state_at(&sim, &plan, last_slot_end, now);
        let past_horizon = resolve_plan_state_at(&sim, &plan, far_future, now);

        assert_eq!(
            battery_soc(&at_horizon_end[ASSET_BATTERY]),
            battery_soc(&past_horizon[ASSET_BATTERY]),
            "a t1 past the plan's true horizon end must return the same state as the horizon end itself, not panic or extrapolate"
        );
    }

    #[test]
    fn t1_at_the_last_slots_end_reflects_that_slots_own_committed_action() {
        // Found during review: `Asset::simulate_forward`'s default body only
        // applies a real step for a `windows(2)` PAIR of setpoints -- its
        // lone trailing point is a zero-duration re-evaluation, so without
        // `simulated_trajectory`'s appended sentinel, the LAST remaining
        // slot's own action would never be reflected in any point at all,
        // for any plan length. This pins the fix: a t1 at the last slot's
        // start (action not yet committed) must differ from a t1 at that
        // same slot's end (action committed), by exactly that slot's own
        // effect -- not be silently identical.
        let now = Utc::now();
        let sim = SimState::from_params(
            &[AssetParams::Battery(BatteryParams {
                id: ASSET_BATTERY.to_string(),
                capacity_kwh: 10.0,
                max_charge_kw: 5.0,
                max_discharge_kw: 5.0,
                initial_soc: 0.5,
                round_trip_efficiency: 1.0,
                min_soc: 0.1,
                c_terminal_eur_kwh: Some(0.0),
            })],
            now,
        );
        let mut plan = make_plan(900, 1, now); // single 15-min slot
        plan.slots[0]
            .planned_kw_by_asset
            .insert(ASSET_BATTERY.to_string(), 5.0); // 5 kW * 0.25h = 1.25 kWh -> +0.125 soc

        let at_start = resolve_plan_state_at(&sim, &plan, plan.slots[0].start, now);
        let at_end = resolve_plan_state_at(&sim, &plan, plan.slots[0].end, now);

        assert_eq!(
            battery_soc(&at_start[ASSET_BATTERY]),
            0.5,
            "before the slot's own action, soc must still be the live initial value"
        );
        assert!(
            (battery_soc(&at_end[ASSET_BATTERY]) - 0.625).abs() < 1e-9,
            "after the slot's own action, soc must reflect its committed charge (0.5 + 0.125 = 0.625), got {}",
            battery_soc(&at_end[ASSET_BATTERY])
        );
    }

    /// R-69 visibility check, now a resolution check (resolved 2026-09-11, D-A —
    /// see `docs/history/project_journal.md`'s Phase 3 entry): the resolver reuses
    /// `battery.rs`'s efficiency model, same as `simulated_trajectory` already does,
    /// and both now use the same symmetric `sqrt(round_trip_efficiency)` split
    /// `battery_milp.rs` always did, so the resolver's SoC must agree with what the
    /// planner believed when it produced `planned_state_by_asset`, including for a
    /// partial (non-full-cycle) charge.
    #[test]
    fn r69_partial_cycle_soc_agrees_with_planned_state_by_asset_now_that_r69_has_landed() {
        // R-69 (battery-efficiency-model-reconciliation, resolved 2026-09-11, D-A): this test
        // used to pin the *disagreement* between battery.rs's asymmetric model and
        // battery_milp.rs's symmetric one -- see docs/history/project_journal.md's Phase 3
        // entry for the resolution. battery.rs now also splits loss symmetrically via
        // sqrt(round_trip_efficiency) on both legs, so the resolver and the planner's own
        // belief must agree on partial-cycle SoC too, not just full-cycle totals.
        let now = Utc::now();
        let round_trip_efficiency = 0.81; // sqrt(0.81) = 0.9
        let sim = SimState::from_params(
            &[AssetParams::Battery(BatteryParams {
                id: ASSET_BATTERY.to_string(),
                capacity_kwh: 10.0,
                max_charge_kw: 5.0,
                max_discharge_kw: 5.0,
                initial_soc: 0.0,
                round_trip_efficiency,
                min_soc: 0.0,
                c_terminal_eur_kwh: Some(0.0),
            })],
            now,
        );
        let mut plan = make_plan(3600, 1, now); // one 1h slot, partial cycle (charge only)
        plan.slots[0]
            .planned_kw_by_asset
            .insert(ASSET_BATTERY.to_string(), 5.0); // 5 kWh AC import over the slot
        let t1 = plan.slots[0].end;

        // What the planner believes (battery_milp.rs's symmetric sqrt(rte) split):
        // stored = 5.0 * sqrt(0.81) = 4.5 kWh -> soc = 0.45.
        let planner_believed_soc = 5.0 * round_trip_efficiency.sqrt() / 10.0;

        // What the resolver (battery.rs, now also symmetric) reports: same 4.5 kWh -> soc = 0.45.
        let resolved = resolve_plan_state_at(&sim, &plan, t1, now);
        let resolver_soc = battery_soc(&resolved[ASSET_BATTERY]);

        assert!(
            (resolver_soc - planner_believed_soc).abs() < 1e-9,
            "resolver={resolver_soc}, planner-believed={planner_believed_soc} -- \
             battery.rs and battery_milp.rs must agree on partial-cycle SoC now that both \
             use the symmetric sqrt(round_trip_efficiency) split"
        );
    }
}
