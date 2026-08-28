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

use crate::assets::{Asset, AssetConfig, AssetHandle};
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
        match cfg {
            AssetConfig::Pv(pv) => {
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
                    // trusting a physics-only artifact. No session at all means
                    // no known deadline to respect, so the EV keeps contributing
                    // (map_or's `true` default) rather than being excluded
                    // outright — this asset's forecast must not depend on a
                    // user-created session existing at all.
                    ev_session.is_none_or(|s| slot.start < s.departure_time)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entities::asset::PlanTrigger;
    use crate::entities::asset_params::{AssetParams, BatteryParams, EvParams, PvParams};
    use crate::entities::plan::{Plan, PlanTimeSlot, PlanZone, PlanningHorizon, SolveStatus};
    use crate::entities::planner_params::PlannerObjective;
    use crate::ids::{ASSET_BATTERY, ASSET_EV};
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
}
