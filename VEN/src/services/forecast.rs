//! WP3.6 (BL-15) — build `AssetForecast`s from the planner's per-slot
//! solution after each plan cycle. Pure Plan → Vec<AssetForecast> mapping;
//! stored in `AppState` by the planning task and served at `GET /forecast`.

use chrono::{DateTime, Utc};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::sync::Arc;

use crate::controller::simulator_port::SimSnapshot;
use crate::controller::{HistoryPort, WeatherForecastPort};
use crate::entities::asset_params::PvForecastParams;
use crate::entities::design_vocabulary::{AssetForecast, AssetHeuristics, ForecastSource};
use crate::entities::history::{ForecastAccuracySample, ForecastLeadKind, PlanHistorySample};
use crate::entities::plan::Plan;
use crate::entities::weather::WeatherForecast;
use crate::state::AppState;

/// Everything that happens after a plan cycle resolves, in one call (kept
/// here for the tasks/ file-size cap): WP4.3 new-warning notifications (only
/// when the plan was adopted) followed by envelope/forecast publication.
#[allow(clippy::too_many_arguments)]
pub async fn finish_plan_cycle(
    state: &AppState,
    sim: &std::sync::Arc<tokio::sync::Mutex<crate::simulator::SimState>>,
    notifier: &crate::services::notify::Notifier,
    wall_now: DateTime<Utc>,
    prev_plan: Option<&Plan>,
    cycle: &crate::services::planning::PlanCycleResult,
    weather: &Arc<dyn WeatherForecastPort>,
    weather_pv_params: Option<&PvForecastParams>,
    history: Option<Arc<dyn HistoryPort>>,
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
    // Fetched once and shared: both publish_post_cycle_state and the forecast-accuracy
    // capture below need it, and it's an RwLock read + full HashMap clone.
    let heuristics = state.asset_heuristics().await;
    publish_post_cycle_state(
        state,
        &sim_snap,
        &cycle.plan,
        wall_now,
        weather,
        weather_pv_params,
        &heuristics,
    )
    .await;

    // forecast-accuracy-tracking: best-effort write, log-and-continue on failure — same
    // pattern as history_sampler::write_window. Skipped entirely when history is disabled.
    if let Some(history) = history {
        let samples = record_forecast_accuracy_samples(&cycle.plan, &heuristics, wall_now);
        if !samples.is_empty() {
            let h = history.clone();
            let res =
                tokio::task::spawn_blocking(move || h.append_forecast_samples(&samples)).await;
            match res {
                Ok(Ok(())) => {}
                Ok(Err(e)) => tracing::warn!("forecast-accuracy sample write failed: {e}"),
                Err(e) => tracing::warn!("forecast-accuracy sample write task panicked: {e}"),
            }
        }

        // GB-25: one plan-history row per cycle (adopted or not — this is a solve-quality
        // trend, not a dispatch-history view). Same best-effort, log-and-continue contract.
        let row = build_plan_history_sample(&cycle.plan);
        let res = tokio::task::spawn_blocking(move || history.append_plan_history(&[row])).await;
        match res {
            Ok(Ok(())) => {}
            Ok(Err(e)) => tracing::warn!("plan-history row write failed: {e}"),
            Err(e) => tracing::warn!("plan-history row write task panicked: {e}"),
        }
    }
}

/// GB-25: build the one `PlanHistorySample` row for a resolved plan cycle. `created_at`
/// comes from `plan.created_at` (already clock-injected via `services::planning`) — never a
/// fresh `Utc::now()` call, per the determinism rule.
fn build_plan_history_sample(plan: &Plan) -> PlanHistorySample {
    // Serialize via serde (SCREAMING_SNAKE_CASE, e.g. "PERIODIC") rather than Rust's Debug
    // format, so the stored value matches the same wire vocabulary `GET /plan` already uses
    // (dto rule: no separate normalized vocabulary per layer).
    let trigger = serde_json::to_value(plan.trigger.clone())
        .ok()
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_else(|| format!("{:?}", plan.trigger));
    PlanHistorySample {
        plan_id: plan.id,
        created_at: plan.created_at,
        trigger,
        solver_ms: plan.solver_ms,
        solve_status: plan.solve_status,
        objective_eur: plan.objective_eur,
        friction_eur: plan.friction_eur,
        mip_gap_target: plan.mip_gap_target,
        warning_count: plan.warnings.len() as u32,
        warning_kinds: plan.warnings.iter().map(|w| w.kind).collect(),
        c_energy_eur: Some(plan.cost_breakdown.c_energy_eur),
        c_grid_eur: Some(plan.cost_breakdown.c_grid_eur),
        c_wear_eur: Some(plan.cost_breakdown.c_wear_eur),
        c_violations_eur: Some(plan.cost_breakdown.c_violations_eur),
        c_peak_penalty_eur: Some(plan.cost_breakdown.c_peak_penalty_eur),
    }
}

/// forecast-accuracy-tracking: build the plan's near (`slots[1]`) and far (`slots.last()`)
/// forecast samples for PV and base_load. No-op for plans with fewer than two
/// slots — `slots[1]` wouldn't exist yet. See design.md Decisions 1-3.
pub fn record_forecast_accuracy_samples(
    plan: &Plan,
    heuristics: &HashMap<String, AssetHeuristics>,
    now: DateTime<Utc>,
) -> Vec<ForecastAccuracySample> {
    if plan.slots.len() < 2 {
        return Vec::new();
    }
    let Some(far_slot) = plan.slots.last() else {
        return Vec::new();
    };
    let near_slot = &plan.slots[1];

    let mut out = Vec::new();
    for (lead_kind, slot) in [
        (ForecastLeadKind::Near, near_slot),
        (ForecastLeadKind::Far, far_slot),
    ] {
        out.push(ForecastAccuracySample {
            asset_id: crate::ids::ASSET_PV.to_string(),
            lead_kind,
            target_ts: slot.start,
            // Sign convention: slot.pv_forecast_kw (from the solver's p_pv_kw input, see
            // milp_planner/results.rs) is a non-negative generation magnitude, but the
            // asset's actual/tick power_kw is negative-for-export (assets/pv.rs) — negate
            // so predicted_kw and actual_kw share one sign convention, same reasoning as
            // build_weather_pv_forecast below.
            predicted_kw: -slot.pv_forecast_kw,
            predicted_at: now,
            actual_kw: None,
            actual_at: None,
        });
        if let Some(h) = heuristics.get(crate::ids::ASSET_BASE_LOAD) {
            out.push(ForecastAccuracySample {
                asset_id: crate::ids::ASSET_BASE_LOAD.to_string(),
                lead_kind,
                target_ts: slot.start,
                predicted_kw: h.sample_kw(slot.start),
                predicted_at: now,
                actual_kw: None,
                actual_at: None,
            });
        }
    }
    out
}

/// Post-plan-cycle state publication: site flexibility envelope and per-asset
/// forecasts (WP3.6, BL-15), both derived from the *adopted* plan — the one
/// actually driving dispatch, never a rejected candidate.
#[allow(clippy::too_many_arguments)]
pub async fn publish_post_cycle_state(
    state: &AppState,
    sim_snap: &SimSnapshot,
    adopted_plan: &Plan,
    wall_now: DateTime<Utc>,
    weather: &Arc<dyn WeatherForecastPort>,
    weather_pv_params: Option<&PvForecastParams>,
    heuristics: &HashMap<String, AssetHeuristics>,
) {
    let env = crate::controller::envelope::compute_envelope(sim_snap, wall_now);
    state.set_site_envelope(env).await;

    let mut forecasts = build_asset_forecasts(adopted_plan, wall_now);
    // WP5.2 (BL-14): add heuristic-sourced forecasts for assets that never
    // appear in the plan's own allocations (uncontrollable assets like
    // base_load) — no precedence conflict since Optimization
    // and Heuristic never cover the same asset_id in practice.
    let existing_ids: HashSet<String> = forecasts.iter().map(|f| f.asset_id.clone()).collect();
    if !heuristics.is_empty() {
        let slot_starts: Vec<DateTime<Utc>> = adopted_plan.slots.iter().map(|s| s.start).collect();
        for hf in build_heuristic_forecasts(heuristics, &slot_starts, wall_now) {
            if !existing_ids.contains(&hf.asset_id) {
                forecasts.push(hf);
            }
        }
    }
    // R-50 (remainder): weather-sourced PV forecast. PV has no LP decision
    // variable (see assets/pv.rs), so it never appears in `planned_kw_by_asset`
    // — no precedence conflict with the Optimization-sourced forecasts above,
    // same reasoning as the heuristics block. `None` (no config, no feed, or
    // stale) leaves PV forecast-less here exactly as before this existed.
    if let Some(params) = weather_pv_params {
        if !existing_ids.contains(crate::ids::ASSET_PV) {
            if let Some(forecast) = weather.latest().await {
                if forecast.is_fresh(
                    wall_now,
                    crate::services::planning::WEATHER_STALENESS_THRESHOLD,
                ) {
                    forecasts.push(build_weather_pv_forecast(&forecast, params, wall_now));
                }
            }
        }
    }
    state.set_asset_forecasts(forecasts).await;
}

/// Weather-sourced PV forecast for the API-visible `/forecast` endpoint —
/// the counterpart to R-50's planner-input wiring
/// (`entities::solar::resolve_weather_pv_kw`), built from the same
/// `weather_pv_forecast_series` so the two views can't silently diverge.
///
/// `AssetForecast.confidence` is a single overall scalar (not per-slot), so
/// the per-hour `base_confidence(age_h) × (1 − irradiance_variability)`
/// signal is averaged across the series. `base_confidence` is a starting
/// default (linear decay to a 0.2 floor at the 48h horizon), not a measured
/// curve — tune once real accuracy data exists.
pub fn build_weather_pv_forecast(
    forecast: &WeatherForecast,
    params: &PvForecastParams,
    now: DateTime<Utc>,
) -> AssetForecast {
    let series = crate::entities::solar::weather_pv_forecast_series(params, forecast);
    // Sign convention: AssetForecast.power_kw is positive = import (see
    // entities/design_vocabulary.rs); PV only exports, so negate.
    let power_kw: Vec<f64> = series.iter().map(|s| -s.forecast_ac_kw).collect();
    let confidence = if forecast.samples.is_empty() {
        0.0
    } else {
        forecast.samples.iter().map(slot_confidence).sum::<f64>() / forecast.samples.len() as f64
    };
    AssetForecast {
        asset_id: crate::ids::ASSET_PV.to_string(),
        updated_at: now,
        source: ForecastSource::WeatherModel,
        confidence,
        power_kw,
        soc: None,
        availability_windows: None,
    }
}

/// `base_confidence(age_h) × (1 − irradiance_variability)` for one sample —
/// see `build_weather_pv_forecast`'s doc comment for why this starting
/// formula isn't a measured curve.
fn slot_confidence(sample: &crate::entities::weather::WeatherForecastSample) -> f64 {
    const HORIZON_H: f64 = 48.0;
    const FLOOR: f64 = 0.2;
    let base_confidence = (1.0 - sample.age_h as f64 / HORIZON_H).clamp(FLOOR, 1.0);
    base_confidence * (1.0 - sample.irradiance_variability.unwrap_or(1.0))
}

/// Build one `AssetForecast` per learned heuristic, tagged
/// `ForecastSource::Heuristic`, sampling `daytime_profile_kw[day_of_week_bucket][hour]
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
            penalty_rules_active: vec![],
            solver_ms: None,
            mip_gap_target: None,
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
            marginal_cost_import_eur_per_kwh: 0.20,
            marginal_cost_export_eur_per_kwh: 0.20,
            rate_estimated: false,
            import_cap_kw: 10.0,
            export_cap_kw: 5.0,
            baseline_kw: 0.5,
            pv_forecast_kw: 0.0,
            pv_used_kw: 0.0,
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

    // ── build_plan_history_sample (GB-25) ──────────────────────────────────

    #[test]
    fn build_plan_history_sample_carries_solver_ms_status_and_warning_kinds() {
        use crate::entities::plan::{PlanWarning, SolveStatus, WarningKind, WarningSeverity};
        let mut plan = make_plan_with_slots(vec![]);
        plan.solver_ms = Some(77);
        plan.solve_status = SolveStatus::Optimal;
        plan.mip_gap_target = Some(0.02);
        plan.warnings = vec![
            PlanWarning {
                severity: WarningSeverity::Warning,
                kind: WarningKind::StaleRateEstimate,
                message: "stale".to_string(),
                suggested_action: None,
            },
            PlanWarning {
                severity: WarningSeverity::Critical,
                kind: WarningKind::SolverInfeasible,
                message: "infeasible".to_string(),
                suggested_action: None,
            },
        ];
        plan.cost_breakdown.c_energy_eur = 1.5;
        plan.cost_breakdown.c_peak_penalty_eur = 0.3;

        let row = build_plan_history_sample(&plan);

        assert_eq!(row.plan_id, plan.id);
        assert_eq!(row.created_at, plan.created_at);
        assert_eq!(row.trigger, "PERIODIC");
        assert_eq!(row.solver_ms, Some(77));
        assert_eq!(row.solve_status, SolveStatus::Optimal);
        assert_eq!(row.mip_gap_target, Some(0.02));
        assert_eq!(row.warning_count, 2);
        assert_eq!(
            row.warning_kinds,
            vec![
                WarningKind::StaleRateEstimate,
                WarningKind::SolverInfeasible
            ]
        );
        assert_eq!(row.c_energy_eur, Some(1.5));
        assert_eq!(row.c_peak_penalty_eur, Some(0.3));
    }

    #[test]
    fn build_plan_history_sample_none_solver_ms_on_a_fallback_plan() {
        // fallback_plan (results.rs) never sets solver_ms — must stay None, not a synthesized 0.
        let mut plan = make_plan_with_slots(vec![]);
        plan.solver_ms = None;
        let row = build_plan_history_sample(&plan);
        assert_eq!(row.solver_ms, None);
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

        let mut monday_profile = vec![0.3; 24];
        monday_profile[8] = 1.5;
        let mut heuristics = HashMap::new();
        heuristics.insert(
            "base_load".to_string(),
            AssetHeuristics {
                asset_id: "base_load".to_string(),
                daytime_profile_kw: std::array::from_fn(|bucket| {
                    if bucket == 0 {
                        monday_profile.clone()
                    } else {
                        vec![0.3; 24]
                    }
                }),
                seasonal_factor: 1.0,
                last_updated: Some(ts(0)),
                recent_mean_abs_error_kw: None,
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
                daytime_profile_kw: std::array::from_fn(|_| vec![1.0; 24]),
                seasonal_factor: 1.5,
                last_updated: Some(now),
                recent_mean_abs_error_kw: None,
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
        let mut saturday_profile = vec![0.3; 24];
        saturday_profile[10] = 2.2; // brunch peak
        let mut heuristics = HashMap::new();
        heuristics.insert(
            "base_load".to_string(),
            AssetHeuristics {
                asset_id: "base_load".to_string(),
                daytime_profile_kw: std::array::from_fn(|bucket| {
                    if bucket == 5 {
                        saturday_profile.clone()
                    } else {
                        vec![0.3; 24]
                    }
                }),
                seasonal_factor: 1.0,
                last_updated: Some(ts(0)),
                recent_mean_abs_error_kw: None,
            },
        );

        let forecasts = build_heuristic_forecasts(&heuristics, &[saturday_10am], ts(0));
        assert!(
            (forecasts[0].power_kw[0] - 2.2).abs() < 1e-9,
            "a Saturday slot must sample the Saturday bucket"
        );
    }

    // ── build_weather_pv_forecast / slot_confidence (R-50, task 8.4/8.5) ────

    use crate::entities::asset_params::{PvArrayGeometry, PvForecastParams, PvSnowParams};
    use crate::entities::weather::{GeoPosition, SkyCondition, WeatherForecastSample};

    fn weather_sample(age_h: u32, irradiance_variability: Option<f64>) -> WeatherForecastSample {
        WeatherForecastSample {
            valid_at: ts(age_h as i64 * 3600),
            age_h,
            temperature_c: 20.0,
            ghi_w_m2: 400.0,
            wind_speed_kmh: None,
            rain_prob_pct: None,
            new_snowfall_cm: None,
            sky_condition: Some(SkyCondition::Clear),
            irradiance_variability,
        }
    }

    fn weather_forecast(
        samples: Vec<WeatherForecastSample>,
    ) -> crate::entities::weather::WeatherForecast {
        crate::entities::weather::WeatherForecast {
            source_id: "test".into(),
            location: GeoPosition {
                latitude_deg: 47.4491,
                longitude_deg: 7.8081,
            },
            fetched_at: ts(0),
            samples,
        }
    }

    fn pv_forecast_params() -> PvForecastParams {
        PvForecastParams {
            rated_kwp: 10.0,
            geometry: PvArrayGeometry {
                location: GeoPosition {
                    latitude_deg: 47.4491,
                    longitude_deg: 7.8081,
                },
                tilt_deg: 30.0,
                azimuth_deg: 180.0,
            },
            performance_ratio: 0.87,
            temp_coeff_pct_per_c: -0.35,
            noct_c: 45.0,
            ac_limit_kw: None,
            snow: PvSnowParams::default(),
        }
    }

    #[test]
    fn slot_confidence_uniform_sky_is_not_reduced() {
        let sample = weather_sample(1, Some(0.0)); // uniform sky (clear or overcast)
        let base = (1.0 - 1.0 / 48.0_f64).clamp(0.2, 1.0);
        assert!((slot_confidence(&sample) - base).abs() < 1e-9);
    }

    #[test]
    fn slot_confidence_broken_sky_is_maximally_reduced() {
        let sample = weather_sample(1, Some(1.0)); // maximally broken sky
        assert_eq!(slot_confidence(&sample), 0.0);
    }

    #[test]
    fn slot_confidence_missing_variability_treated_as_maximal_uncertainty() {
        let sample = weather_sample(1, None);
        assert_eq!(
            slot_confidence(&sample),
            0.0,
            "missing irradiance_variability must be treated as maximum uncertainty, not stable"
        );
    }

    #[test]
    fn slot_confidence_decays_with_age_h() {
        let near = weather_sample(1, Some(0.0));
        let far = weather_sample(40, Some(0.0));
        assert!(
            slot_confidence(&near) > slot_confidence(&far),
            "a same-quality forecast further out must have lower confidence"
        );
    }

    #[test]
    fn build_weather_pv_forecast_tags_source_and_negates_power() {
        let params = pv_forecast_params();
        let noon = chrono::Utc.with_ymd_and_hms(2026, 6, 21, 11, 0, 0).unwrap();
        let forecast = weather_forecast(vec![WeatherForecastSample {
            valid_at: noon,
            ..weather_sample(1, Some(0.0))
        }]);
        let af = build_weather_pv_forecast(&forecast, &params, noon);
        assert_eq!(af.asset_id, crate::ids::ASSET_PV);
        assert_eq!(af.source, ForecastSource::WeatherModel);
        assert_eq!(af.power_kw.len(), 1);
        assert!(
            af.power_kw[0] <= 0.0,
            "PV forecast power must be non-positive (export), got {}",
            af.power_kw[0]
        );
    }

    #[test]
    fn build_weather_pv_forecast_empty_samples_zero_confidence() {
        let params = pv_forecast_params();
        let forecast = weather_forecast(vec![]);
        let af = build_weather_pv_forecast(&forecast, &params, ts(0));
        assert_eq!(af.confidence, 0.0);
        assert!(af.power_kw.is_empty());
    }

    // ── record_forecast_accuracy_samples (forecast-accuracy-tracking, task 3.3) ────

    fn slot_with_pv(idx: usize, pv_forecast_kw: f64) -> PlanTimeSlot {
        let mut s = slot(idx, &[], &[]);
        s.pv_forecast_kw = pv_forecast_kw;
        s
    }

    fn base_load_heuristic() -> AssetHeuristics {
        AssetHeuristics {
            asset_id: crate::ids::ASSET_BASE_LOAD.to_string(),
            daytime_profile_kw: std::array::from_fn(|_| vec![0.7; 24]),
            seasonal_factor: 1.0,
            last_updated: Some(ts(0)),
            recent_mean_abs_error_kw: None,
        }
    }

    #[test]
    fn record_forecast_accuracy_samples_two_slot_plan_yields_four_samples() {
        let plan = make_plan_with_slots(vec![slot_with_pv(0, 1.0), slot_with_pv(1, 2.0)]);
        let mut heuristics = HashMap::new();
        heuristics.insert(
            crate::ids::ASSET_BASE_LOAD.to_string(),
            base_load_heuristic(),
        );

        let samples = record_forecast_accuracy_samples(&plan, &heuristics, ts(0));
        assert_eq!(samples.len(), 4, "2 assets x 2 lead kinds (near, far)");
    }

    #[test]
    fn record_forecast_accuracy_samples_single_slot_plan_yields_zero() {
        let plan = make_plan_with_slots(vec![slot_with_pv(0, 1.0)]);
        let samples = record_forecast_accuracy_samples(&plan, &HashMap::new(), ts(0));
        assert!(samples.is_empty());
    }

    #[test]
    fn record_forecast_accuracy_samples_near_point_never_uses_slot_zero() {
        // slots[0] and slots[1] deliberately carry different pv_forecast_kw — the near
        // sample must come from slots[1], never slots[0].
        let plan = make_plan_with_slots(vec![slot_with_pv(0, 99.0), slot_with_pv(1, 2.0)]);
        let samples = record_forecast_accuracy_samples(&plan, &HashMap::new(), ts(0));
        let near_pv = samples
            .iter()
            .find(|s| s.asset_id == crate::ids::ASSET_PV && s.lead_kind == ForecastLeadKind::Near)
            .unwrap();
        // pv_forecast_kw is a non-negative generation magnitude (2.0); predicted_kw must be
        // negated to match the actual/tick sign convention (negative = export).
        assert_eq!(near_pv.predicted_kw, -2.0);
        assert_eq!(near_pv.target_ts, ts(300));
    }

    #[test]
    fn record_forecast_accuracy_samples_missing_heuristic_omits_only_that_asset() {
        let plan = make_plan_with_slots(vec![slot_with_pv(0, 1.0), slot_with_pv(1, 2.0)]);
        // No base_load heuristic present — PV must still be sampled.
        let heuristics = HashMap::new();

        let samples = record_forecast_accuracy_samples(&plan, &heuristics, ts(0));
        // PV (2) only; base_load omitted entirely.
        assert_eq!(samples.len(), 2);
        assert!(samples
            .iter()
            .all(|s| s.asset_id != crate::ids::ASSET_BASE_LOAD));
        assert!(samples.iter().any(|s| s.asset_id == crate::ids::ASSET_PV));
    }
}
