//! Forward-looking per-asset forecast frames for the site headroom trajectory
//! (`controller::envelope_forecast::compute_headroom_forecast`). Infra-side
//! (allowed to touch `Asset`/`AssetConfig`, unlike `controller/`): re-simulates
//! battery/EV/heater forward from their REAL current state — never
//! `Plan.planned_state_by_asset`, a stale solve-time-only snapshot — driven
//! by the active plan's own already-decided setpoint schedule, and re-derives
//! PV's forecast ceiling fresh via `PvInverter::capability_trajectory`. Mirrors
//! `SimState::to_sim_snapshot`'s pattern of flattening asset-level data into a
//! plain, controller-consumable shape.

use std::collections::HashMap;

use chrono::{DateTime, Utc};

use crate::assets::{Asset, AssetConfig, AssetHandle};
use crate::controller::simulator_port::{AssetForecastFrame, AssetForecastPoint};
use crate::entities::device_session::EvSession;
use crate::entities::plan::{Plan, PlanTimeSlot};
use crate::ids::ASSET_PV;

use super::SimState;

/// One frame per remaining plan slot (`slot.start >= now`), each carrying
/// every controllable asset's forecasted capability at that slot.
pub fn build_forecast_frames(
    sim: &SimState,
    plan: &Plan,
    ev_session: Option<&EvSession>,
    now: DateTime<Utc>,
) -> Vec<AssetForecastFrame> {
    let future_slots: Vec<&PlanTimeSlot> = plan.all_slots().filter(|s| s.start >= now).collect();
    if future_slots.is_empty() {
        return Vec::new();
    }

    let mut frames: Vec<AssetForecastFrame> = future_slots
        .iter()
        .map(|s| AssetForecastFrame {
            ts: s.start,
            assets: HashMap::new(),
        })
        .collect();

    for (entry, cfg) in sim.iter_assets() {
        match cfg {
            AssetConfig::Pv(pv) => {
                insert_pv_points(&mut frames, pv, &entry.state, &future_slots, now);
            }
            AssetConfig::BaseLoad(_) => {
                // Uncontrollable, fixed point — never contributes flexibility.
            }
            AssetConfig::Ev(_) => {
                insert_simulated_points(&mut frames, entry, cfg, &future_slots, |slot| {
                    // EvState::plugged is never toggled by step() (verified: no
                    // `plugged =` assignment in ev.rs::step_inner) — a pure
                    // physics projection would keep showing "still plugged" for
                    // the whole horizon even past the real session's deadline.
                    // Zero this asset out past the live session end instead of
                    // trusting a physics-only artifact.
                    ev_session.is_some_and(|s| slot.start < s.departure_time)
                });
            }
            AssetConfig::Battery(_) | AssetConfig::Heater(_) => {
                insert_simulated_points(&mut frames, entry, cfg, &future_slots, |_| true);
            }
        }
    }

    frames
}

/// Battery/EV/heater: re-simulate forward from the asset's REAL current state,
/// driven by the plan's own `planned_kw_by_asset` schedule for this asset —
/// one setpoint per remaining slot, giving one projected state per slot start
/// (see `Asset::simulate_forward`'s doc comment: each `TrajectoryPoint` pairs
/// the state BEFORE that slot's step with the setpoint driving it).
fn insert_simulated_points(
    frames: &mut [AssetForecastFrame],
    entry: &super::AssetEntry,
    cfg: &AssetConfig,
    future_slots: &[&PlanTimeSlot],
    include_at: impl Fn(&PlanTimeSlot) -> bool,
) {
    let handle = AssetHandle {
        config: cfg,
        id: &entry.id,
        state: &entry.state,
        history: &entry.history,
    };
    let schedule: Vec<(DateTime<Utc>, f64)> = future_slots
        .iter()
        .map(|s| {
            (
                s.start,
                s.planned_kw_by_asset.get(&entry.id).copied().unwrap_or(0.0),
            )
        })
        .collect();
    let traj = handle.simulate_forward(&entry.state, &schedule);

    for (i, point) in traj.points.iter().enumerate() {
        let Some(slot) = future_slots.get(i) else {
            continue;
        };
        if !include_at(slot) {
            continue;
        }
        let cap = handle.capability(&point.state);
        frames[i].assets.insert(
            entry.id.clone(),
            AssetForecastPoint {
                planned_kw: schedule[i].1,
                cap_max_import_kw: cap.max_import_kw,
                cap_max_export_kw: cap.max_export_kw,
            },
        );
    }
}

/// PV: no `simulate_forward` — re-derive the forecast ceiling fresh via its
/// own `capability_trajectory` (already forecast-aware, already re-derives
/// from `now`), paired with the plan's own `pv_used_kw` at each slot.
fn insert_pv_points(
    frames: &mut [AssetForecastFrame],
    pv: &crate::assets::PvInverter,
    state: &crate::assets::AssetState,
    future_slots: &[&PlanTimeSlot],
    now: DateTime<Utc>,
) {
    let last = match future_slots.last() {
        Some(s) => s,
        None => return,
    };
    let duration = last.end - now;
    let resolution = if future_slots.len() > 1 {
        future_slots[1].start - future_slots[0].start
    } else {
        future_slots[0].end - future_slots[0].start
    };
    let traj = pv.capability_trajectory(state, duration, resolution, now);

    for (i, slot) in future_slots.iter().enumerate() {
        let Some((_, cap)) = traj.get(i) else {
            continue;
        };
        frames[i].assets.insert(
            ASSET_PV.to_string(),
            AssetForecastPoint {
                planned_kw: slot.pv_used_kw,
                cap_max_import_kw: cap.max_import_kw,
                cap_max_export_kw: cap.max_export_kw,
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entities::asset::PlanTrigger;
    use crate::entities::asset_params::{AssetParams, BatteryParams, EvParams, PvParams};
    use crate::entities::plan::{Plan, PlanTimeSlot, PlanZone, PlanningHorizon, SolveStatus};
    use crate::entities::planner_params::PlannerObjective;
    use crate::ids::{ASSET_BATTERY, ASSET_EV};
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

    #[test]
    fn battery_capability_evolves_across_slots_not_flat_copied() {
        // capability() is a step function (max_charge_kw until soc hits
        // exactly 1.0, then 0) -- a small capacity + a strong charge rate
        // pushes the battery to full within the very first slot, so the
        // real evidence of per-slot re-simulation is the step transition
        // itself (5.0 -> 0.0 between slot 0 and slot 1), not a gradual
        // decline (this asset never declines gradually).
        let now = Utc::now();
        let sim = SimState::from_params(
            &[AssetParams::Battery(BatteryParams {
                id: ASSET_BATTERY.to_string(),
                capacity_kwh: 2.0,
                max_charge_kw: 5.0,
                max_discharge_kw: 5.0,
                initial_soc: 0.5,
                round_trip_efficiency: 1.0,
                min_soc: 0.1,
                c_terminal_eur_kwh: Some(0.0),
            })],
            now,
        );
        let mut plan = make_plan(900, 2, now); // 2 x 15-min slots
        for slot in &mut plan.slots {
            // Charge at max rate: 0.5 soc -> full needs 1.0 kWh headroom;
            // 5 kW for 15 min = 1.25 kWh > that, so slot 0's step alone
            // reaches full.
            slot.planned_kw_by_asset
                .insert(ASSET_BATTERY.to_string(), 5.0);
        }

        let frames = build_forecast_frames(&sim, &plan, None, now);
        assert_eq!(frames.len(), 2);
        let import_caps: Vec<f64> = frames
            .iter()
            .map(|f| f.assets[ASSET_BATTERY].cap_max_import_kw)
            .collect();
        assert!(
            import_caps[0] > 0.0 && import_caps[1] == 0.0,
            "expected a step transition from 5.0 (not yet full) to 0.0 \
             (full after slot 0's step) — proves capability is re-derived \
             per slot from the re-simulated state, not flat-copied; got {:?}",
            import_caps
        );
    }

    #[test]
    fn pv_ignores_the_plans_own_stale_forecast_field() {
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
        let mut plan = make_plan(900, 1, now);
        // Deliberately wrong: nothing in this codebase can physically produce
        // -999 kW from a 5 kW-rated inverter. If build_forecast_frames ever
        // starts reading Plan.pv_forecast_kw instead of re-deriving fresh via
        // capability_trajectory, this value would leak straight through.
        plan.slots[0].pv_forecast_kw = -999.0;

        let frames = build_forecast_frames(&sim, &plan, None, now);
        let cap_export = frames[0].assets[ASSET_PV].cap_max_export_kw;
        assert!(
            cap_export >= -5.0,
            "PV ceiling must be bounded by inverter_max_kw regardless of Plan.pv_forecast_kw, got {}",
            cap_export
        );
    }

    #[test]
    fn only_slots_at_or_after_now_are_included() {
        let now = Utc::now();
        let sim = SimState::from_params(&[], now);
        // Plan starts 30 minutes in the past relative to `now`.
        let plan = make_plan(900, 4, now - Duration::minutes(30));

        let frames = build_forecast_frames(&sim, &plan, None, now);
        assert!(frames.iter().all(|f| f.ts >= now));
        assert_eq!(
            frames.len(),
            plan.all_slots().filter(|s| s.start >= now).count()
        );
    }

    #[test]
    fn ev_zeroes_out_past_the_live_sessions_departure() {
        let now = Utc::now();
        let sim = SimState::from_params(
            &[AssetParams::Ev(EvParams {
                id: ASSET_EV.to_string(),
                max_charge_kw: 7.0,
                max_discharge_kw: 0.0,
                initial_soc: 0.5,
                battery_kwh: 60.0,
                soc_target: 0.8,
                default_charge_kw: 0.0,
                min_charge_kw: 1.4,
                response_delay_s: 0.0,
            })],
            now,
        );
        let plan = make_plan(900, 4, now); // 4 x 15-min slots = 1h horizon
        let session = EvSession {
            id: Uuid::new_v4(),
            target_soc: 0.8,
            departure_time: now + Duration::minutes(30), // ends mid-horizon
            soft_deadline: false,
            mode: Default::default(),
            budget_eur: None,
            comfort_rates: vec![],
            created_at: now,
            updated_at: now,
        };

        let frames = build_forecast_frames(&sim, &plan, Some(&session), now);
        assert!(
            frames[0].assets.contains_key(ASSET_EV),
            "EV should still contribute before its session ends"
        );
        assert!(
            !frames[3].assets.contains_key(ASSET_EV),
            "EV must not contribute past its live session's departure_time"
        );
    }
}
