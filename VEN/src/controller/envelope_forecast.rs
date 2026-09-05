//! Forward-looking site headroom trajectory — pure post-processing over an
//! already-solved `Plan` plus freshly re-simulated per-asset forecast frames
//! (`simulator::forecast::build_forecast_frames`). Deliberately outside
//! `controller::milp_planner`: touches no solver internals (no `SolveOutput`,
//! no LP variables) — only the plan's own already-decided setpoint schedule
//! (`planned_kw_by_asset`) plus each asset's pure capability logic. See
//! `entities::plan::SiteFlexibilityForecastSlot`'s doc comment for why this
//! must be recomputed every tick rather than read statically off the plan.

use chrono::{DateTime, Utc};

use crate::controller::simulator_port::{AssetForecastFrame, SimSnapshot};
use crate::entities::device_session::ShiftableLoad;
use crate::entities::plan::{Plan, SiteFlexibilityForecastSlot};
use crate::ids::ASSET_PV;

/// Each slot's up_kw/down_kw is an INDEPENDENT point-in-time counterfactual
/// ("the single best alternate move here, holding the rest of the plan
/// fixed") — not a conserved multi-slot flexibility budget. This is why a
/// shiftable load can show down-flex at several not-yet-run slots and
/// up-flex at its currently-scheduled slot without being double-counted:
/// each is a different, independent hypothetical, never summed together
/// anywhere in this design (matches how the live instant envelope already
/// works — using up_kw now isn't deducted from down_kw next slot either).
pub fn compute_headroom_forecast(
    frames: &[AssetForecastFrame],
    plan: &Plan,
    shiftable_loads: &[ShiftableLoad],
    snapshot: &SimSnapshot,
) -> Vec<SiteFlexibilityForecastSlot> {
    frames
        .iter()
        .map(|frame| {
            let mut up_kw = 0.0_f64;
            let mut down_kw = 0.0_f64;

            for (asset_id, point) in &frame.assets {
                if asset_id == ASSET_PV {
                    // Up: unused generation headroom toward the fresh forecast
                    // ceiling — how much MORE the site could export (further
                    // reduce consumption) if PV weren't held below its ceiling.
                    // Same shape as every other asset's "up" formula.
                    up_kw += (point.planned_kw - point.cap_max_export_kw).max(0.0);
                    // Down: how much of PV's own currently-planned output could
                    // be curtailed away, increasing site consumption by the
                    // same amount. Bounded by PV's own output, not by
                    // cap_max_import_kw (always 0 — PV can never import).
                    down_kw += (-point.planned_kw).max(0.0);
                    continue;
                }
                up_kw += (point.planned_kw - point.cap_max_export_kw).max(0.0);
                down_kw += (point.cap_max_import_kw - point.planned_kw).max(0.0);
            }

            down_kw += shiftable_down_kw(plan, shiftable_loads, snapshot, frame.ts);
            up_kw += shiftable_up_kw(plan, shiftable_loads, snapshot, frame.ts);

            SiteFlexibilityForecastSlot {
                ts: frame.ts,
                up_kw,
                down_kw,
            }
        })
        .collect()
}

/// A `started` shiftable-load asset means the load is already running in
/// reality — fully committed (this model has no interrupt/early-stop
/// mechanism), so it contributes no flexibility of either kind for the rest
/// of its life, regardless of what any current or future plan says. Reads
/// the live `SimSnapshot` (`ShiftableLoadAsset::state_values()`'s `"started"`
/// flag) instead of a bolt-on `&[ShiftableLoadRuntime]` list.
fn already_run(load: &ShiftableLoad, snapshot: &SimSnapshot) -> bool {
    snapshot
        .assets
        .get(&load.asset_id)
        .map(|a| a.values.get("started").copied().unwrap_or(0.0) > 0.5)
        .unwrap_or(false)
}

/// First plan slot where this load's own planned power turns nonzero — i.e.
/// where the CURRENT plan intends to start it, if at all.
fn planned_start(plan: &Plan, load: &ShiftableLoad) -> Option<DateTime<Utc>> {
    plan.all_slots()
        .find(|s| {
            s.planned_kw_by_asset
                .get(&load.asset_id)
                .copied()
                .unwrap_or(0.0)
                != 0.0
        })
        .map(|s| s.start)
}

fn is_planned_running_at(plan: &Plan, load: &ShiftableLoad, ts: DateTime<Utc>) -> bool {
    plan.all_slots()
        .find(|s| s.start == ts)
        .and_then(|s| s.planned_kw_by_asset.get(&load.asset_id).copied())
        .map(|kw| kw != 0.0)
        .unwrap_or(false)
}

/// True if a later start than `chosen_start` still fits before `latest_end`
/// — i.e. the load is genuinely deferrable, not deadline-committed.
fn has_later_valid_start(load: &ShiftableLoad, chosen_start: DateTime<Utc>) -> bool {
    let duration = chrono::Duration::minutes(load.duration_min as i64);
    chosen_start < load.latest_end - duration
}

/// True if starting the load exactly at `ts` would still fit inside its
/// window. Assumes `ts >= now` already (the caller's frames are always
/// built for the remaining horizon only) — this function only checks the
/// load's own window, not the current wall-clock time.
pub(crate) fn valid_start_exists_at(load: &ShiftableLoad, ts: DateTime<Utc>) -> bool {
    let duration = chrono::Duration::minutes(load.duration_min as i64);
    ts >= load.earliest_start && ts + duration <= load.latest_end
}

fn shiftable_down_kw(
    plan: &Plan,
    loads: &[ShiftableLoad],
    snapshot: &SimSnapshot,
    ts: DateTime<Utc>,
) -> f64 {
    loads
        .iter()
        .filter(|l| !already_run(l, snapshot))
        .filter(|l| !is_planned_running_at(plan, l, ts))
        .filter(|l| valid_start_exists_at(l, ts))
        .map(|l| l.power_kw)
        .sum()
}

fn shiftable_up_kw(
    plan: &Plan,
    loads: &[ShiftableLoad],
    snapshot: &SimSnapshot,
    ts: DateTime<Utc>,
) -> f64 {
    loads
        .iter()
        .filter(|l| !already_run(l, snapshot))
        .filter(|l| is_planned_running_at(plan, l, ts))
        .filter(|l| {
            planned_start(plan, l)
                .map(|start| has_later_valid_start(l, start))
                .unwrap_or(false)
        })
        .map(|l| l.power_kw)
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::controller::simulator_port::AssetForecastPoint;
    use crate::entities::asset::PlanTrigger;
    use crate::entities::plan::{PlanTimeSlot, PlanZone, PlanningHorizon};
    use crate::entities::planner_params::PlannerObjective;
    use chrono::Duration;
    use std::collections::HashMap;
    use uuid::Uuid;

    fn make_plan(step_s: u64, slots: usize, now: DateTime<Utc>) -> Plan {
        let horizon = PlanningHorizon {
            start_time: now,
            end_time: now + Duration::seconds((step_s * slots as u64) as i64),
            step_size_s: step_s,
            num_steps: slots,
            far_horizon: now + Duration::seconds((step_s * slots as u64) as i64),
            zones: vec![PlanZone { step_s, slots }],
        };
        let plan_slots: Vec<PlanTimeSlot> = (0..slots)
            .map(|i| PlanTimeSlot {
                slot_index: i,
                start: now + Duration::seconds((step_s * i as u64) as i64),
                end: now + Duration::seconds((step_s * (i + 1) as u64) as i64),
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
                planned_kw_by_asset: HashMap::new(),
                planned_state_by_asset: HashMap::new(),
                bat_charge_kw: 0.0,
                bat_discharge_kw: 0.0,
            })
            .collect();
        Plan {
            id: Uuid::new_v4(),
            created_at: now,
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
            solve_status: crate::entities::plan::SolveStatus::Optimal,
            penalty_rules_active: vec![],
            solver_ms: None,
            mip_gap_target: None,
        }
    }

    fn make_shiftable_load(
        asset_id: &str,
        power_kw: f64,
        duration_min: u32,
        earliest_start: DateTime<Utc>,
        latest_end: DateTime<Utc>,
    ) -> ShiftableLoad {
        ShiftableLoad {
            id: Uuid::new_v4(),
            asset_id: asset_id.to_string(),
            power_kw,
            duration_min,
            earliest_start,
            latest_end,
            mode: Default::default(),
            created_at: earliest_start,
            updated_at: earliest_start,
        }
    }

    fn empty_sim_snapshot() -> SimSnapshot {
        SimSnapshot {
            ts: Utc::now(),
            grid: crate::controller::simulator_port::GridSnapshot {
                net_power_w: 0.0,
                voltage_v: 230.0,
                import_kwh: 0.0,
                export_kwh: 0.0,
                import_limit_kw: f64::MAX,
                export_limit_kw: -f64::MAX,
            },
            assets: HashMap::new(),
        }
    }

    /// A snapshot with one `started` shiftable-load asset entry — mirrors
    /// what `already_run` reads off `ShiftableLoadAsset::state_values()`.
    fn started_snapshot(asset_id: &str) -> SimSnapshot {
        let mut snap = empty_sim_snapshot();
        snap.assets.insert(
            asset_id.to_string(),
            crate::controller::simulator_port::AssetSnapshot {
                power_kw: 0.0,
                asset_type: "shiftable_load".to_string(),
                cap_max_import_kw: 0.0,
                cap_max_export_kw: 0.0,
                available_discharge_kwh: None,
                available_charge_kwh: None,
                default_setpoint_kw: 0.0,
                setpoint_kw: 0.0,
                values: HashMap::from([("started".to_string(), 1.0)]),
            },
        );
        snap
    }

    // ── Generic battery/EV/heater-shaped asset ──────────────────────────────

    #[test]
    fn battery_shaped_point_contributes_up_and_down_symmetrically() {
        let now = Utc::now();
        let plan = make_plan(300, 2, now);
        let frames = vec![AssetForecastFrame {
            ts: now,
            assets: HashMap::from([(
                "battery".to_string(),
                AssetForecastPoint {
                    planned_kw: 0.0,
                    cap_max_import_kw: 5.0,
                    cap_max_export_kw: -5.0,
                },
            )]),
        }];
        let out = compute_headroom_forecast(&frames, &plan, &[], &empty_sim_snapshot());
        assert_eq!(out.len(), 1);
        assert!((out[0].up_kw - 5.0).abs() < 1e-9);
        assert!((out[0].down_kw - 5.0).abs() < 1e-9);
    }

    #[test]
    fn per_slot_values_reflect_that_slots_own_point_not_a_shared_default() {
        let now = Utc::now();
        let plan = make_plan(300, 2, now);
        let frames = vec![
            AssetForecastFrame {
                ts: now,
                assets: HashMap::from([(
                    "battery".to_string(),
                    AssetForecastPoint {
                        planned_kw: 0.0,
                        cap_max_import_kw: 5.0,
                        cap_max_export_kw: -5.0,
                    },
                )]),
            },
            AssetForecastFrame {
                ts: now + Duration::seconds(300),
                assets: HashMap::from([(
                    "battery".to_string(),
                    AssetForecastPoint {
                        planned_kw: 0.0,
                        cap_max_import_kw: 0.0, // battery now full — no more "down"
                        cap_max_export_kw: -5.0,
                    },
                )]),
            },
        ];
        let out = compute_headroom_forecast(&frames, &plan, &[], &empty_sim_snapshot());
        assert!((out[0].down_kw - 5.0).abs() < 1e-9);
        assert!((out[1].down_kw - 0.0).abs() < 1e-9);
    }

    // ── PV: up = unused generation margin, down = curtailment of current output ──

    #[test]
    fn pv_up_kw_is_the_unused_generation_margin_toward_the_ceiling() {
        let now = Utc::now();
        let plan = make_plan(300, 1, now);
        // planned (used) = -1.0 kW, fresh ceiling = -3.0 kW → 2.0 kW of unused
        // generation capacity — more export (further consumption reduction)
        // is available if PV weren't held back below its ceiling.
        let frames = vec![AssetForecastFrame {
            ts: now,
            assets: HashMap::from([(
                "pv".to_string(),
                AssetForecastPoint {
                    planned_kw: -1.0,
                    cap_max_import_kw: 0.0,
                    cap_max_export_kw: -3.0,
                },
            )]),
        }];
        let out = compute_headroom_forecast(&frames, &plan, &[], &empty_sim_snapshot());
        assert!(
            (out[0].up_kw - 2.0).abs() < 1e-9,
            "expected 2.0 kW unused-generation margin as up_kw, got {}",
            out[0].up_kw
        );
    }

    #[test]
    fn pv_down_kw_is_the_currently_planned_output_available_to_curtail() {
        let now = Utc::now();
        let plan = make_plan(300, 1, now);
        // planned (used) = -1.0 kW — that whole 1.0 kW could be curtailed away,
        // increasing site consumption by 1.0 kW. Independent of the ceiling.
        let frames = vec![AssetForecastFrame {
            ts: now,
            assets: HashMap::from([(
                "pv".to_string(),
                AssetForecastPoint {
                    planned_kw: -1.0,
                    cap_max_import_kw: 0.0,
                    cap_max_export_kw: -3.0,
                },
            )]),
        }];
        let out = compute_headroom_forecast(&frames, &plan, &[], &empty_sim_snapshot());
        assert!(
            (out[0].down_kw - 1.0).abs() < 1e-9,
            "expected 1.0 kW of curtailable current output as down_kw, got {}",
            out[0].down_kw
        );
    }

    #[test]
    fn pv_fully_using_its_ceiling_contributes_zero_up() {
        let now = Utc::now();
        let plan = make_plan(300, 1, now);
        let frames = vec![AssetForecastFrame {
            ts: now,
            assets: HashMap::from([(
                "pv".to_string(),
                AssetForecastPoint {
                    planned_kw: -3.0,
                    cap_max_import_kw: 0.0,
                    cap_max_export_kw: -3.0,
                },
            )]),
        }];
        let out = compute_headroom_forecast(&frames, &plan, &[], &empty_sim_snapshot());
        assert_eq!(
            out[0].up_kw, 0.0,
            "no unused margin left toward the ceiling"
        );
        assert!(
            (out[0].down_kw - 3.0).abs() < 1e-9,
            "all 3.0 kW of current output is still curtailable, got {}",
            out[0].down_kw
        );
    }

    #[test]
    fn pv_idle_at_zero_ceiling_contributes_nothing_either_way() {
        let now = Utc::now();
        let plan = make_plan(300, 1, now);
        let frames = vec![AssetForecastFrame {
            ts: now,
            assets: HashMap::from([(
                "pv".to_string(),
                AssetForecastPoint {
                    planned_kw: 0.0,
                    cap_max_import_kw: 0.0,
                    cap_max_export_kw: 0.0,
                },
            )]),
        }];
        let out = compute_headroom_forecast(&frames, &plan, &[], &empty_sim_snapshot());
        assert_eq!(out[0].up_kw, 0.0);
        assert_eq!(out[0].down_kw, 0.0);
    }

    // ── BaseLoad: contributes nothing ───────────────────────────────────────

    #[test]
    fn base_load_shaped_point_contributes_zero_both_ways() {
        let now = Utc::now();
        let plan = make_plan(300, 1, now);
        let frames = vec![AssetForecastFrame {
            ts: now,
            assets: HashMap::from([(
                "base_load".to_string(),
                AssetForecastPoint {
                    planned_kw: 0.5,
                    cap_max_import_kw: 0.5, // fixed point: ceiling == current
                    cap_max_export_kw: 0.5,
                },
            )]),
        }];
        let out = compute_headroom_forecast(&frames, &plan, &[], &empty_sim_snapshot());
        assert_eq!(out[0].up_kw, 0.0);
        assert_eq!(out[0].down_kw, 0.0);
    }

    // ── Shiftable loads: down (not-yet-run) ─────────────────────────────────

    #[test]
    fn shiftable_load_not_yet_run_contributes_down_when_a_valid_start_remains() {
        let now = Utc::now();
        let plan = make_plan(300, 1, now); // load never appears in planned_kw_by_asset
        let load = make_shiftable_load(
            "wm",
            2.0,
            60,
            now,
            now + Duration::hours(4), // plenty of room for a 60-min run
        );
        let frames = vec![AssetForecastFrame {
            ts: now,
            assets: HashMap::new(),
        }];
        let out = compute_headroom_forecast(&frames, &plan, &[load], &empty_sim_snapshot());
        assert!((out[0].down_kw - 2.0).abs() < 1e-9);
    }

    #[test]
    fn shiftable_load_not_yet_run_contributes_zero_when_no_valid_start_remains_at_that_slot() {
        let now = Utc::now();
        let plan = make_plan(300, 1, now);
        let load = make_shiftable_load(
            "wm",
            2.0,
            60,
            now,
            now + Duration::minutes(30), // window too short to fit a 60-min run
        );
        let frames = vec![AssetForecastFrame {
            ts: now,
            assets: HashMap::new(),
        }];
        let out = compute_headroom_forecast(&frames, &plan, &[load], &empty_sim_snapshot());
        assert_eq!(out[0].down_kw, 0.0);
    }

    #[test]
    fn shiftable_load_already_run_contributes_nothing() {
        let now = Utc::now();
        let plan = make_plan(300, 1, now);
        let load = make_shiftable_load("wm", 2.0, 60, now, now + Duration::hours(4));
        let frames = vec![AssetForecastFrame {
            ts: now,
            assets: HashMap::new(),
        }];
        let out = compute_headroom_forecast(&frames, &plan, &[load], &started_snapshot("wm"));
        assert_eq!(out[0].down_kw, 0.0);
        assert_eq!(out[0].up_kw, 0.0);
    }

    // ── Shiftable loads: up (currently scheduled, deferrable) ──────────────

    #[test]
    fn shiftable_load_currently_scheduled_contributes_up_when_slack_remains() {
        let now = Utc::now();
        let mut plan = make_plan(300, 1, now); // one 5-min slot at `now`
        plan.slots[0]
            .planned_kw_by_asset
            .insert("wm".to_string(), 2.0);
        // Window is 4 hours wide, the run is only 60 min — plenty of slack
        // to defer past the currently-planned start (`now`).
        let load = make_shiftable_load("wm", 2.0, 60, now, now + Duration::hours(4));
        let frames = vec![AssetForecastFrame {
            ts: now,
            assets: HashMap::new(),
        }];
        let out = compute_headroom_forecast(&frames, &plan, &[load], &empty_sim_snapshot());
        assert!(
            (out[0].up_kw - 2.0).abs() < 1e-9,
            "expected 2.0 kW up-flex (deferrable), got {}",
            out[0].up_kw
        );
    }

    #[test]
    fn shiftable_load_currently_scheduled_contributes_zero_up_when_at_last_possible_start() {
        let now = Utc::now();
        let mut plan = make_plan(300, 1, now);
        plan.slots[0]
            .planned_kw_by_asset
            .insert("wm".to_string(), 2.0);
        // 60-min run, window exactly 60 min wide starting at `now` — `now`
        // IS the last possible start; deferring would blow the deadline.
        let load = make_shiftable_load("wm", 2.0, 60, now, now + Duration::minutes(60));
        let frames = vec![AssetForecastFrame {
            ts: now,
            assets: HashMap::new(),
        }];
        let out = compute_headroom_forecast(&frames, &plan, &[load], &empty_sim_snapshot());
        assert_eq!(
            out[0].up_kw, 0.0,
            "a deadline-committed load at its last possible start must show zero up-flex"
        );
    }
}
