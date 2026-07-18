//! WP3.6 (BL-15) — build `AssetForecast`s from the planner's per-slot
//! solution after each plan cycle. Pure Plan → Vec<AssetForecast> mapping;
//! stored in `AppState` by the planning task and served at `GET /forecast`.

use chrono::{DateTime, Utc};
use std::collections::{BTreeSet, HashMap, HashSet};

use crate::controller::simulator_port::SimSnapshot;
use crate::entities::design_vocabulary::{AssetForecast, AssetHeuristics, ForecastSource};
use crate::entities::plan::Plan;
use crate::state::AppState;

/// Everything that happens after a plan cycle resolves, in one call (kept
/// here for the tasks/ file-size cap): WP4.3 new-warning notifications (only
/// when the plan was adopted) followed by envelope/forecast publication.
pub async fn finish_plan_cycle(
    state: &AppState,
    sim: &std::sync::Arc<tokio::sync::Mutex<crate::simulator::SimState>>,
    notifier: &crate::services::notify::Notifier,
    wall_now: DateTime<Utc>,
    prev_plan: Option<&Plan>,
    cycle: &crate::services::planning::PlanCycleResult,
) {
    crate::services::notify::notify_new_plan_warnings(
        notifier,
        state,
        wall_now,
        cycle.adopted,
        prev_plan,
        &cycle.plan,
    )
    .await;
    let sim_snap = sim.lock().await.to_sim_snapshot();
    publish_post_cycle_state(state, &sim_snap, &cycle.plan, wall_now).await;
}

/// Post-plan-cycle state publication: site flexibility envelope and per-asset
/// forecasts (WP3.6, BL-15), both derived from the *adopted* plan — the one
/// actually driving dispatch, never a rejected candidate.
pub async fn publish_post_cycle_state(
    state: &AppState,
    sim_snap: &SimSnapshot,
    adopted_plan: &Plan,
    wall_now: DateTime<Utc>,
) {
    let env = crate::controller::envelope::compute_envelope(sim_snap, wall_now);
    state.set_site_envelope(env).await;

    let mut forecasts = build_asset_forecasts(adopted_plan, wall_now);
    // WP5.2 (BL-14): add heuristic-sourced forecasts for assets that never
    // appear in the plan's own allocations (uncontrollable assets like
    // base_load/site-residual) — no precedence conflict since Optimization
    // and Heuristic never cover the same asset_id in practice.
    let existing_ids: HashSet<String> = forecasts.iter().map(|f| f.asset_id.clone()).collect();
    let heuristics = state.asset_heuristics().await;
    if !heuristics.is_empty() {
        let slot_starts: Vec<DateTime<Utc>> = adopted_plan.slots.iter().map(|s| s.start).collect();
        for hf in build_heuristic_forecasts(&heuristics, &slot_starts, wall_now) {
            if !existing_ids.contains(&hf.asset_id) {
                forecasts.push(hf);
            }
        }
    }
    state.set_asset_forecasts(forecasts).await;
}

/// Build one `AssetForecast` per learned heuristic, tagged
/// `ForecastSource::Heuristic`, sampling `daytime_profile_kw[weekday_bucket][hour]
/// × seasonal_factor` at each of `slot_starts`.
pub fn build_heuristic_forecasts(
    heuristics: &HashMap<String, AssetHeuristics>,
    slot_starts: &[DateTime<Utc>],
    now: DateTime<Utc>,
) -> Vec<AssetForecast> {
    heuristics
        .values()
        .map(|h| {
            let power_kw: Vec<f64> = slot_starts.iter().map(|t| h.sample_kw(*t)).collect();
            AssetForecast {
                asset_id: h.asset_id.clone(),
                updated_at: now,
                source: ForecastSource::Heuristic,
                // Fixed placeholder confidence — WP5.4's quality-metadata work
                // is where this becomes derived from sample count/variance.
                confidence: 0.5,
                power_kw,
                soc: None,
                availability_windows: None,
            }
        })
        .collect()
}

/// Build one `AssetForecast` per asset that appears in the plan's slot
/// allocations (`planned_kw_by_asset`), tagged `ForecastSource::Optimization`.
///
/// `power_kw[t]` is the planned power in slot `t` (0.0 where the asset has no
/// allocation in that slot). `soc` is included only for assets whose
/// `planned_state_by_asset` carries a `"soc"` value in at least one slot;
/// missing per-slot values carry the last known one forward (never NaN —
/// serde_json cannot serialize NaN).
pub fn build_asset_forecasts(plan: &Plan, now: DateTime<Utc>) -> Vec<AssetForecast> {
    let mut asset_ids: BTreeSet<&str> = BTreeSet::new();
    for slot in &plan.slots {
        asset_ids.extend(slot.planned_kw_by_asset.keys().map(String::as_str));
        asset_ids.extend(slot.planned_state_by_asset.keys().map(String::as_str));
    }

    asset_ids
        .into_iter()
        .map(|asset_id| {
            let power_kw: Vec<f64> = plan
                .slots
                .iter()
                .map(|s| s.planned_kw_by_asset.get(asset_id).copied().unwrap_or(0.0))
                .collect();

            let has_soc = plan.slots.iter().any(|s| {
                s.planned_state_by_asset
                    .get(asset_id)
                    .is_some_and(|m| m.contains_key("soc"))
            });
            let soc = has_soc.then(|| {
                let mut last = 0.0;
                plan.slots
                    .iter()
                    .map(|s| {
                        if let Some(v) = s
                            .planned_state_by_asset
                            .get(asset_id)
                            .and_then(|m| m.get("soc"))
                        {
                            last = *v;
                        }
                        last
                    })
                    .collect()
            });

            AssetForecast {
                asset_id: asset_id.to_string(),
                updated_at: now,
                source: ForecastSource::Optimization,
                confidence: 1.0,
                power_kw,
                soc,
                availability_windows: None,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entities::plan::{
        CostBreakdown, Plan, PlanSummary, PlanTimeSlot, PlanZone, PlanningHorizon,
    };
    use chrono::{Duration, TimeZone};
    use std::collections::HashMap;
    use uuid::Uuid;

    fn ts(secs: i64) -> DateTime<Utc> {
        Utc.timestamp_opt(1_700_000_000 + secs, 0).unwrap()
    }

    fn make_plan_with_slots(slots: Vec<PlanTimeSlot>) -> Plan {
        Plan {
            id: Uuid::new_v4(),
            created_at: ts(0),
            trigger: crate::entities::asset::PlanTrigger::Periodic,
            horizon: PlanningHorizon {
                start_time: ts(0),
                end_time: ts(300 * slots.len().max(1) as i64),
                step_size_s: 300,
                num_steps: slots.len(),
                far_horizon: ts(300 * slots.len().max(1) as i64),
                zones: vec![PlanZone {
                    step_s: 300,
                    slots: slots.len().max(1),
                }],
            },
            slots,
            summary: PlanSummary::default(),
            envelopes: vec![],
            warnings: vec![],
            soc_trajectory_kwh: vec![],
            objective: crate::entities::PlannerObjective::MinCost,
            objective_eur: 0.0,
            friction_eur: 0.0,
            cost_breakdown: CostBreakdown::default(),
            solve_status: crate::entities::plan::SolveStatus::Optimal,
        }
    }

    fn slot(idx: usize, kw_by_asset: &[(&str, f64)], soc_by_asset: &[(&str, f64)]) -> PlanTimeSlot {
        let start = ts(idx as i64 * 300);
        PlanTimeSlot {
            slot_index: idx,
            start,
            end: start + Duration::seconds(300),
            import_tariff_eur_kwh: 0.20,
            export_tariff_eur_kwh: 0.05,
            co2_g_kwh: 300.0,
            grid_effective_cost: 0.26,
            rate_estimated: false,
            import_cap_kw: 10.0,
            export_cap_kw: 5.0,
            baseline_kw: 0.5,
            pv_forecast_kw: 0.0,
            surplus_available_kw: 0.0,
            allocations: vec![],
            net_import_kw: 0.0,
            net_export_kw: 0.0,
            import_flexibility_kw: 0.0,
            export_flexibility_kw: 0.0,
            bat_charge_kw: 0.0,
            bat_discharge_kw: 0.0,
            planned_kw_by_asset: kw_by_asset
                .iter()
                .map(|(k, v)| (k.to_string(), *v))
                .collect(),
            planned_state_by_asset: soc_by_asset
                .iter()
                .map(|(k, v)| {
                    let mut m = HashMap::new();
                    m.insert("soc".to_string(), *v);
                    (k.to_string(), m)
                })
                .collect(),
        }
    }

    #[test]
    fn test_build_asset_forecasts_power_matches_planned_state() {
        // BL-15 verify clause: forecast power matches the planner's
        // planned_kw_by_asset for the same asset/horizon.
        let plan = make_plan_with_slots(vec![
            slot(0, &[("battery", 1.5)], &[("battery", 0.5)]),
            slot(1, &[("battery", -2.0)], &[("battery", 0.6)]),
        ]);
        let forecasts = build_asset_forecasts(&plan, ts(0));

        assert_eq!(forecasts.len(), 1);
        let f = &forecasts[0];
        assert_eq!(f.asset_id, "battery");
        assert_eq!(f.source, ForecastSource::Optimization);
        assert_eq!(f.power_kw, vec![1.5, -2.0]);
        assert_eq!(f.soc.as_deref(), Some(&[0.5, 0.6][..]));
    }

    #[test]
    fn test_build_asset_forecasts_missing_slot_values_default_and_carry() {
        let plan = make_plan_with_slots(vec![
            slot(0, &[("ev", 7.0)], &[("ev", 0.4)]),
            slot(1, &[], &[]), // ev absent this slot
        ]);
        let forecasts = build_asset_forecasts(&plan, ts(0));

        let f = &forecasts[0];
        assert_eq!(f.power_kw, vec![7.0, 0.0], "missing power defaults to 0");
        assert_eq!(
            f.soc.as_deref(),
            Some(&[0.4, 0.4][..]),
            "missing soc carries the last known value forward"
        );
    }

    #[test]
    fn test_build_asset_forecasts_no_soc_for_non_storage() {
        let plan = make_plan_with_slots(vec![slot(0, &[("heater", 3.0)], &[])]);
        let forecasts = build_asset_forecasts(&plan, ts(0));
        assert_eq!(forecasts[0].soc, None);
    }

    #[test]
    fn test_build_asset_forecasts_empty_plan_yields_empty() {
        let plan = make_plan_with_slots(vec![]);
        assert!(build_asset_forecasts(&plan, ts(0)).is_empty());
    }

    #[test]
    fn test_build_heuristic_forecasts_samples_hour_and_weekday() {
        // 2023-01-02 was a Monday (weekday index 0).
        let monday_8am = chrono::Utc.with_ymd_and_hms(2023, 1, 2, 8, 0, 0).unwrap();
        let monday_3am = chrono::Utc.with_ymd_and_hms(2023, 1, 2, 3, 0, 0).unwrap();

        let mut weekday_profile = vec![0.3; 24];
        weekday_profile[8] = 1.5;
        let mut heuristics = HashMap::new();
        heuristics.insert(
            "base_load".to_string(),
            AssetHeuristics {
                asset_id: "base_load".to_string(),
                daytime_profile_kw: [weekday_profile, vec![0.3; 24]],
                seasonal_factor: 1.0,
                last_updated: Some(ts(0)),
            },
        );

        let forecasts = build_heuristic_forecasts(&heuristics, &[monday_3am, monday_8am], ts(0));
        assert_eq!(forecasts.len(), 1);
        let f = &forecasts[0];
        assert_eq!(f.source, ForecastSource::Heuristic);
        assert_eq!(f.power_kw, vec![0.3, 1.5]);
    }

    #[test]
    fn test_build_heuristic_forecasts_applies_seasonal_factor() {
        let now = ts(0);
        let mut heuristics = HashMap::new();
        heuristics.insert(
            "base_load".to_string(),
            AssetHeuristics {
                asset_id: "base_load".to_string(),
                daytime_profile_kw: [vec![1.0; 24], vec![1.0; 24]],
                seasonal_factor: 1.5,
                last_updated: Some(now),
            },
        );
        let monday = chrono::Utc.with_ymd_and_hms(2023, 1, 2, 0, 0, 0).unwrap();
        let forecasts = build_heuristic_forecasts(&heuristics, &[monday], now);
        assert!(
            (forecasts[0].power_kw[0] - 1.5).abs() < 1e-9,
            "1.0 * 1.5 = 1.5"
        );
    }

    #[test]
    fn test_build_heuristic_forecasts_picks_weekend_bucket_on_saturday() {
        // 2023-01-02 was a Monday, 2023-01-07 a Saturday.
        let saturday_10am = chrono::Utc.with_ymd_and_hms(2023, 1, 7, 10, 0, 0).unwrap();
        let mut weekend_profile = vec![0.3; 24];
        weekend_profile[10] = 2.2; // brunch peak
        let mut heuristics = HashMap::new();
        heuristics.insert(
            "base_load".to_string(),
            AssetHeuristics {
                asset_id: "base_load".to_string(),
                daytime_profile_kw: [vec![0.3; 24], weekend_profile],
                seasonal_factor: 1.0,
                last_updated: Some(ts(0)),
            },
        );

        let forecasts = build_heuristic_forecasts(&heuristics, &[saturday_10am], ts(0));
        assert!(
            (forecasts[0].power_kw[0] - 2.2).abs() < 1e-9,
            "a Saturday slot must sample the weekend bucket"
        );
    }
}
