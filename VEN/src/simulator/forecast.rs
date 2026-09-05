//! Forward-looking per-asset forecast frames for the site headroom trajectory
//! (`controller::envelope_forecast::compute_headroom_forecast`). Infra-side
//! (allowed to touch `Asset`/`AssetConfig`, unlike `controller/`): re-simulates
//! battery/EV/heater forward from their REAL current state — never
//! `Plan.planned_state_by_asset`, a stale solve-time-only snapshot — driven
//! by the active plan's own already-decided setpoint schedule, and re-derives
//! PV's forecast ceiling fresh — never from `Plan.pv_forecast_kw` (a
//! solve-time value that may be minutes stale) — through the shared
//! `entities::solar::pv_ceiling_kw`, the same weather-first resolution the
//! planner's own `p_pv_kw` input uses. Mirrors `SimState::to_sim_snapshot`'s
//! pattern of flattening asset-level data into a plain, controller-consumable
//! shape.

use std::collections::HashMap;

use chrono::{DateTime, Utc};

use crate::assets::{Asset, AssetHandle, AssetState, Trajectory};
use crate::controller::simulator_port::{AssetForecastFrame, AssetForecastPoint};
use crate::entities::device_session::EvSession;
use crate::entities::plan::{Plan, PlanTimeSlot};
use crate::entities::solar::{pv_ceiling_kw, PvCeilingParams};
use crate::ids::ASSET_PV;

use super::SimState;

/// One frame per remaining plan slot (`slot.start >= now`), each carrying
/// every controllable asset's forecasted capability at that slot.
///
/// `weather_pv_kw`, when supplied, must already be aligned to those same
/// remaining slots (one entry per `future_slots` element, resolved through
/// `entities::solar::resolve_weather_pv_kw` against their own start times) —
/// the identical input shape the planner builds for `p_pv_kw`, so headroom
/// and plan resolve PV from the same numbers.
pub fn build_forecast_frames(
    sim: &SimState,
    plan: &Plan,
    ev_session: Option<&EvSession>,
    weather_pv_kw: Option<&[f64]>,
    pv_forecast_override: Option<f64>,
    now: DateTime<Utc>,
) -> Vec<AssetForecastFrame> {
    let future_slots: Vec<&PlanTimeSlot> = plan.all_slots().filter(|s| s.start >= now).collect();
    if future_slots.is_empty() {
        return Vec::new();
    }
    // `pv_alpha` decays per zone-A step, so the unit is the FINEST zone's
    // width — not this slot's own width, which widens across the horizon.
    let zone_a_step_s = plan
        .horizon
        .zones
        .first()
        .map(|z| z.step_s as i64)
        .unwrap_or(plan.horizon.step_size_s as i64)
        .max(1);

    let mut frames: Vec<AssetForecastFrame> = future_slots
        .iter()
        .map(|s| AssetForecastFrame {
            ts: s.start,
            assets: HashMap::new(),
        })
        .collect();

    for (entry, cfg) in sim.iter_assets() {
        // Dispatched by `asset_type_str()` rather than a match on the old
        // `AssetConfig` enum (Spec A) — PV needs its concrete type (no
        // `simulate_forward`, ceiling comes from weather instead), the rest
        // share `insert_simulated_points` with a per-kind `include_at` filter.
        match cfg.asset_type_str() {
            "pv" => {
                let pv = cfg
                    .as_any()
                    .downcast_ref::<crate::assets::PvInverter>()
                    .expect("asset_type_str() == \"pv\" implies a PvInverter");
                insert_pv_points(
                    &mut frames,
                    pv,
                    &future_slots,
                    weather_pv_kw,
                    pv_forecast_override,
                    zone_a_step_s,
                    now,
                );
            }
            "base_load" => {
                // Uncontrollable, fixed point — never contributes flexibility.
            }
            "ev" => {
                insert_simulated_points(&mut frames, entry, cfg, &future_slots, |slot| {
                    // EvState::plugged is never toggled by step() (verified: no
                    // `plugged =` assignment in ev.rs::step_inner) — a pure
                    // physics projection would keep showing "still plugged" for
                    // the whole horizon even past the real session's deadline.
                    // Zero this asset out past the live session end instead of
                    // trusting a physics-only artifact. No session at all means
                    // no known deadline to respect, so the EV keeps contributing
                    // (map_or's `true` default) rather than being excluded
                    // outright — this asset's forecast must not depend on a
                    // user-created session existing at all.
                    ev_session.is_none_or(|s| slot.start < s.departure_time)
                });
            }
            _ => {
                // battery, heater
                insert_simulated_points(&mut frames, entry, cfg, &future_slots, |_| true);
            }
        }
    }

    frames
}

/// Re-simulate an asset forward from its REAL current state, driven by the
/// plan's own `planned_kw_by_asset` schedule for this asset — one setpoint
/// per remaining slot, giving one projected state per slot start (see
/// `Asset::simulate_forward`'s doc comment: each `TrajectoryPoint` pairs the
/// state BEFORE that slot's step with the setpoint driving it).
///
/// Shared by `insert_simulated_points` (below, capability-per-slot) and
/// `resolve_plan_state_at` (state-at-a-single-`t1`) so there is exactly one
/// place that runs this simulation — `planstate-t1-resolver`'s D1: two
/// independent implementations of "the plan-driven forecast" is exactly what
/// this master plan exists to remove.
fn simulated_trajectory(
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
    let schedule: Vec<(DateTime<Utc>, f64)> = future_slots
        .iter()
        .map(|s| {
            (
                s.start,
                s.planned_kw_by_asset.get(&entry.id).copied().unwrap_or(0.0),
            )
        })
        .collect();
    handle.simulate_forward(&entry.state, &schedule)
}

/// Battery/EV/heater: capability derived from `simulated_trajectory`'s
/// per-slot state, one `AssetForecastPoint` per remaining slot.
fn insert_simulated_points(
    frames: &mut [AssetForecastFrame],
    entry: &super::AssetEntry,
    cfg: &dyn Asset,
    future_slots: &[&PlanTimeSlot],
    include_at: impl Fn(&PlanTimeSlot) -> bool,
) {
    let handle = AssetHandle {
        config: cfg,
        id: &entry.id,
        state: &entry.state,
        history: &entry.history,
    };
    let traj = simulated_trajectory(entry, cfg, future_slots);

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
                planned_kw: slot
                    .planned_kw_by_asset
                    .get(&entry.id)
                    .copied()
                    .unwrap_or(0.0),
                cap_max_import_kw: cap.max_import_kw,
                cap_max_export_kw: cap.max_export_kw,
            },
        );
    }
}

/// PV: no `simulate_forward` — its ceiling is set by the weather, not by any
/// state the plan evolves. Resolved per slot through the one shared
/// `entities::solar::pv_ceiling_kw`, the same function the planner's own
/// `p_pv_kw` input goes through, so the plan and the headroom drawn against
/// it can never disagree about what PV can do.
///
/// Each slot is evaluated at its OWN `start` and its OWN cumulative offset
/// from `now`. The previous implementation resampled onto a uniform grid and
/// zipped by index, which silently handed far-out slots the ceiling of a much
/// earlier wall-clock time once the horizon widened past its first zone.
fn insert_pv_points(
    frames: &mut [AssetForecastFrame],
    pv: &crate::assets::PvInverter,
    future_slots: &[&PlanTimeSlot],
    weather_pv_kw: Option<&[f64]>,
    pv_forecast_override: Option<f64>,
    zone_a_step_s: i64,
    now: DateTime<Utc>,
) {
    let params = PvCeilingParams {
        rated_kw: pv.rated_kw,
        inverter_max_kw: pv.inverter_max_kw,
        irradiance_offset: pv.irradiance_offset,
        pv_alpha: pv.pv_alpha,
        zone_a_step_s,
    };

    for (i, slot) in future_slots.iter().enumerate() {
        let ceiling_kw = pv_ceiling_kw(
            &params,
            slot.start,
            (slot.start - now).num_seconds(),
            weather_pv_kw.and_then(|v| v.get(i)).copied(),
            pv_forecast_override,
        );
        frames[i].assets.insert(
            ASSET_PV.to_string(),
            AssetForecastPoint {
                // pv_used_kw is a positive generation magnitude (see
                // controller/timeline.rs, controller/dispatcher.rs, which both
                // negate it for the same reason) — negate here too so it
                // matches cap_max_export_kw's export-negative convention.
                planned_kw: -slot.pv_used_kw,
                // PV can never import, in any slot.
                cap_max_import_kw: 0.0,
                cap_max_export_kw: -ceiling_kw,
            },
        );
    }
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
/// `build_forecast_frames` already runs), picked at the latest remaining
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

    sim.iter_assets()
        .map(|(entry, cfg)| {
            if cfg.asset_type_str() == "pv" {
                return (entry.id.clone(), entry.state.clone());
            }
            let traj = simulated_trajectory(entry, cfg, &future_slots);
            let idx = future_slots
                .iter()
                .rposition(|s| s.start <= t1)
                .unwrap_or(0);
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
    use crate::entities::asset_params::{
        AssetParams, BaseLoadParams, BatteryParams, EvParams, PvParams,
    };
    use crate::entities::plan::{Plan, PlanTimeSlot, PlanZone, PlanningHorizon, SolveStatus};
    use crate::entities::planner_params::PlannerObjective;
    use crate::ids::{ASSET_BASE_LOAD, ASSET_BATTERY, ASSET_EV};
    use chrono::{Duration, TimeZone, Timelike};
    use std::collections::HashMap as Map;
    use uuid::Uuid;

    /// Multi-zone horizon (the real production shape — `plan_zones` in every
    /// shipped profile is 3 zones of widening step). Slot starts are
    /// accumulated from each zone's own `step_s`, so the grid is deliberately
    /// NON-uniform: this is exactly what a uniform-resolution resampler gets
    /// wrong.
    fn make_zoned_plan(zones: &[(u64, usize)], start: DateTime<Utc>) -> Plan {
        let mut plan_slots: Vec<PlanTimeSlot> = Vec::new();
        let mut t = start;
        for (step_s, slots) in zones {
            for _ in 0..*slots {
                let end = t + Duration::seconds(*step_s as i64);
                plan_slots.push(base_slot(plan_slots.len(), t, end));
                t = end;
            }
        }
        let total_slots = plan_slots.len();
        let horizon = PlanningHorizon {
            start_time: start,
            end_time: t,
            step_size_s: zones[0].0,
            num_steps: total_slots,
            far_horizon: t,
            zones: zones
                .iter()
                .map(|(step_s, slots)| PlanZone {
                    step_s: *step_s,
                    slots: *slots,
                })
                .collect(),
        };
        let mut plan = make_plan(zones[0].0, 1, start);
        plan.horizon = horizon;
        plan.slots = plan_slots;
        plan
    }

    fn base_slot(idx: usize, start: DateTime<Utc>, end: DateTime<Utc>) -> PlanTimeSlot {
        PlanTimeSlot {
            slot_index: idx,
            start,
            end,
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
        }
    }

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

        let frames = build_forecast_frames(&sim, &plan, None, None, None, now);
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
    fn pv_planned_kw_is_negative_when_generating_matching_export_negative_convention() {
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
        // pv_used_kw is stored as a positive generation magnitude everywhere
        // else in this codebase (controller/timeline.rs, controller/dispatcher.rs
        // both negate it) — the frame's planned_kw must follow the same
        // export-negative convention as cap_max_export_kw, or every downstream
        // headroom formula that mixes the two silently breaks.
        plan.slots[0].pv_used_kw = 2.0;

        let frames = build_forecast_frames(&sim, &plan, None, None, None, now);
        let planned_kw = frames[0].assets[ASSET_PV].planned_kw;
        assert!(
            (planned_kw - (-2.0)).abs() < 1e-9,
            "expected planned_kw = -2.0 (pv_used_kw negated), got {planned_kw}"
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
        // entities::solar::pv_ceiling_kw, this value would leak straight through.
        plan.slots[0].pv_forecast_kw = -999.0;

        let frames = build_forecast_frames(&sim, &plan, None, None, None, now);
        let cap_export = frames[0].assets[ASSET_PV].cap_max_export_kw;
        assert!(
            cap_export >= -5.0,
            "PV ceiling must be bounded by inverter_max_kw regardless of Plan.pv_forecast_kw, got {}",
            cap_export
        );
    }

    /// Regression: PV's ceiling must be evaluated at each slot's OWN
    /// timestamp. The previous implementation resampled PV onto a uniform
    /// grid (resolution taken from the first two slots) and then zipped it
    /// against the real slots BY INDEX — so on the production 3-zone
    /// horizon, far-out slots were handed the ceiling belonging to a much
    /// earlier wall-clock time. Live symptom: a night slot ~33 h out showed
    /// ~12 kW of PV export headroom because it received a mid-afternoon
    /// value from the day before.
    #[test]
    fn pv_ceiling_is_evaluated_at_each_slots_own_timestamp_on_a_zoned_horizon() {
        // 19:30 UTC start: zone A (96 x 300s = 8h) runs to 03:30, then zone B
        // (96 x 900s = 24h) widens the grid — index and wall-clock diverge
        // from slot 96 onward.
        let now = Utc.with_ymd_and_hms(2026, 8, 28, 19, 30, 0).unwrap();
        let sim = SimState::from_params(
            &[AssetParams::Pv(PvParams {
                id: ASSET_PV.to_string(),
                rated_kw: 14.4,
                inverter_max_kw: 12.5,
                co2_g_kwh: 0.0,
            })],
            now,
        );
        let plan = make_zoned_plan(&[(300, 96), (900, 96)], now);

        let frames = build_forecast_frames(&sim, &plan, None, None, None, now);

        // Every frame whose own timestamp is outside the sin model's daylight
        // window (06:00-18:00 UTC) must have a zero PV ceiling. Under the
        // index-zip bug the deep-zone-B night slots carried ~12.5 kW.
        let mut night_frames = 0;
        for frame in &frames {
            let hour = frame.ts.hour() as f64 + frame.ts.minute() as f64 / 60.0;
            if (6.0..=18.0).contains(&hour) {
                continue;
            }
            night_frames += 1;
            let cap = frame.assets[ASSET_PV].cap_max_export_kw;
            assert!(
                cap.abs() < 1e-9,
                "night slot at {} must have zero PV ceiling, got {cap} kW",
                frame.ts
            );
        }
        assert!(
            night_frames > 50,
            "fixture must span many night slots across both zones, got {night_frames}"
        );
    }

    /// The headroom forecast must resolve PV from the weather feed, not the
    /// sin model, whenever the planner would have — otherwise the plan and
    /// the headroom drawn against it disagree on every cloudy hour.
    #[test]
    fn pv_ceiling_uses_the_weather_series_when_one_is_supplied() {
        // Midnight start: the sin model says 0 for every slot, so any non-zero
        // ceiling can only have come from the weather series.
        let now = Utc.with_ymd_and_hms(2026, 8, 28, 0, 0, 0).unwrap();
        let sim = SimState::from_params(
            &[AssetParams::Pv(PvParams {
                id: ASSET_PV.to_string(),
                rated_kw: 14.4,
                inverter_max_kw: 12.5,
                co2_g_kwh: 0.0,
            })],
            now,
        );
        let plan = make_zoned_plan(&[(300, 4)], now);
        let weather = [3.0, 4.0, 99.0, 0.0];

        let frames = build_forecast_frames(&sim, &plan, None, Some(&weather), None, now);

        assert_eq!(frames.len(), 4);
        assert!((frames[0].assets[ASSET_PV].cap_max_export_kw + 3.0).abs() < 1e-9);
        assert!((frames[1].assets[ASSET_PV].cap_max_export_kw + 4.0).abs() < 1e-9);
        // Clamped to the inverter's own AC ceiling, exactly as the planner does.
        assert!((frames[2].assets[ASSET_PV].cap_max_export_kw + 12.5).abs() < 1e-9);
        assert!(frames[3].assets[ASSET_PV].cap_max_export_kw.abs() < 1e-9);
    }

    /// Guards the disagreement this whole change exists to close: the frame's
    /// PV ceiling and the planner's `p_pv_kw` input must be the same number
    /// for the same slot, because both now resolve through `pv_ceiling_kw`.
    #[test]
    fn pv_ceiling_matches_what_the_planner_resolves_for_the_same_slot() {
        use crate::entities::solar::{pv_ceiling_kw, PvCeilingParams};

        let now = Utc.with_ymd_and_hms(2026, 8, 28, 19, 30, 0).unwrap();
        let sim = SimState::from_params(
            &[AssetParams::Pv(PvParams {
                id: ASSET_PV.to_string(),
                rated_kw: 14.4,
                inverter_max_kw: 12.5,
                co2_g_kwh: 0.0,
            })],
            now,
        );
        let plan = make_zoned_plan(&[(300, 96), (900, 96)], now);
        let frames = build_forecast_frames(&sim, &plan, None, None, None, now);

        let params = PvCeilingParams {
            rated_kw: 14.4,
            inverter_max_kw: 12.5,
            irradiance_offset: 0.0,
            pv_alpha: 0.1,
            zone_a_step_s: 300,
        };
        for frame in &frames {
            let planner_kw = pv_ceiling_kw(
                &params,
                frame.ts,
                (frame.ts - now).num_seconds(),
                None,
                None,
            );
            let frame_kw = -frame.assets[ASSET_PV].cap_max_export_kw;
            assert!(
                (frame_kw - planner_kw).abs() < 1e-9,
                "at {}: frame says {frame_kw} kW, planner resolves {planner_kw} kW",
                frame.ts
            );
        }
    }

    #[test]
    fn only_slots_at_or_after_now_are_included() {
        let now = Utc::now();
        let sim = SimState::from_params(&[], now);
        // Plan starts 30 minutes in the past relative to `now`.
        let plan = make_plan(900, 4, now - Duration::minutes(30));

        let frames = build_forecast_frames(&sim, &plan, None, None, None, now);
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
                v2g_capable: false,
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

        let frames = build_forecast_frames(&sim, &plan, Some(&session), None, None, now);
        assert!(
            frames[0].assets.contains_key(ASSET_EV),
            "EV should still contribute before its session ends"
        );
        assert!(
            !frames[3].assets.contains_key(ASSET_EV),
            "EV must not contribute past its live session's departure_time"
        );
    }

    #[test]
    fn ev_contributes_at_every_slot_when_no_session_is_active() {
        // No user-created charging session at all is the common case (an EV
        // just sitting plugged in) — it must not be read as "deadline already
        // passed, exclude everywhere". Absence of a session means absence of
        // a known deadline, not an implicit one in the past.
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
                v2g_capable: false,
            })],
            now,
        );
        let plan = make_plan(900, 4, now);

        let frames = build_forecast_frames(&sim, &plan, None, None, None, now);

        assert!(
            frames.iter().all(|f| f.assets.contains_key(ASSET_EV)),
            "EV must contribute at every slot when there is no session to bound it"
        );
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

        // Direct comparison: the same schedule build_forecast_frames itself
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
    fn base_load_is_included_even_though_build_forecast_frames_skips_it() {
        // build_forecast_frames deliberately excludes base_load from
        // capability frames (it never contributes flexibility) -- but
        // resolve_plan_state_at answers "what state is asset X in", which is
        // a well-defined question for base_load too (assetMaxPower's own
        // roster includes it), so it must not be silently dropped here.
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
        let last_slot_start = plan.slots.last().unwrap().start;
        let far_future = last_slot_start + Duration::hours(10);

        let at_last_slot = resolve_plan_state_at(&sim, &plan, last_slot_start, now);
        let past_horizon = resolve_plan_state_at(&sim, &plan, far_future, now);

        assert_eq!(
            battery_soc(&at_last_slot[ASSET_BATTERY]),
            battery_soc(&past_horizon[ASSET_BATTERY]),
            "a t1 past the plan's horizon must return the same state as its last remaining slot, not panic or extrapolate"
        );
    }

    /// R-69 visibility check (design.md's Risks section): the resolver
    /// reuses `battery.rs`'s (asymmetric) efficiency model, same as
    /// `build_forecast_frames` already does -- this does not create the
    /// mismatch against `battery_milp.rs`'s symmetric model, but if R-69
    /// (`openspec/changes/battery-efficiency-model-reconciliation/`) is
    /// still open, the resolver's SoC will disagree with what the planner
    /// itself believed when it produced `planned_state_by_asset` for a
    /// partial (non-full-cycle) charge. This test records that disagreement
    /// explicitly rather than silently accepting a loose tolerance.
    #[test]
    fn r69_partial_cycle_soc_disagrees_with_planned_state_by_asset_until_r69_lands() {
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

        // What the planner believed (battery_milp.rs's symmetric sqrt(rte) split):
        // stored = 5.0 * sqrt(0.81) = 4.5 kWh -> soc = 0.45.
        let planner_believed_soc = 5.0 * round_trip_efficiency.sqrt() / 10.0;

        // What the resolver (battery.rs's asymmetric, all-loss-on-charge model) reports:
        // stored = 5.0 * 0.81 = 4.05 kWh -> soc = 0.405.
        let resolved = resolve_plan_state_at(&sim, &plan, t1, now);
        let resolver_soc = battery_soc(&resolved[ASSET_BATTERY]);

        assert!(
            (resolver_soc - planner_believed_soc).abs() > 1e-6,
            "R-69 has apparently been resolved (battery.rs and battery_milp.rs now agree on \
             partial-cycle SoC: resolver={resolver_soc}, planner-believed={planner_believed_soc}) \
             -- if this assertion now fails, update this test to assert equality instead, and \
             note the resolution in this change's journal entry"
        );
    }
}
