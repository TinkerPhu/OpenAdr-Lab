//! WP3.6 (BL-15) — build `AssetForecast`s from the planner's per-slot
//! solution after each plan cycle. Pure Plan → Vec<AssetForecast> mapping;
//! stored in `AppState` by the planning task and served at `GET /forecast`.

use chrono::{DateTime, Utc};
use std::collections::BTreeSet;

use crate::entities::design_vocabulary::{AssetForecast, ForecastSource};
use crate::entities::plan::Plan;

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
}
