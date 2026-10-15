use axum::{
    extract::{Path, Query, State},
    response::IntoResponse,
    Json,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use ts_rs::TS;

use crate::entities::timeline::TimelineSnapshot;

use crate::app_ctx::SimRead;
use crate::state::AppState;

#[derive(Deserialize)]
pub struct TimelineParams {
    pub hours_back: Option<f64>,
    pub hours_forward: Option<f64>,
    /// Grid bucket width in seconds. Takes precedence over max_points.
    pub resolution: Option<u64>,
    /// Deprecated: converted to resolution internally.
    pub max_points: Option<usize>,
}

/// Nice grid bucket widths, in seconds, so chart timestamps land on clean
/// minute/hour boundaries instead of arbitrary values like 24s or 588s.
const NICE_RESOLUTIONS_S: &[u64] = &[
    1, 2, 5, 10, 15, 30, // sub-minute
    60, 120, 300, 600, 900, 1800, // 1m, 2m, 5m, 10m, 15m, 30m
    3600, 7200, 10800, 21600, 43200, // 1h, 2h, 3h, 6h, 12h
    86400, // 1 day
];

/// Snap `raw` up to the smallest value in `NICE_RESOLUTIONS_S` that is >= raw.
/// Beyond the table's max (86400s), snap up to the nearest whole-day multiple.
fn snap_up_to_nice(raw: u64) -> u64 {
    if let Some(&nice) = NICE_RESOLUTIONS_S.iter().find(|&&v| v >= raw) {
        return nice;
    }
    let days = (raw as f64 / 86400.0).ceil() as u64;
    days.max(1) * 86400
}

/// Resolve the grid resolution in seconds from query parameters.
/// Priority: resolution > max_points > auto (~300 points).
/// Result is clamped so the grid has at most 3600 points.
/// The auto-computed value (and the 3600-point clamp fallback) is snapped up
/// to the nearest "nice" bucket width (e.g. 30s, 5m, 1h) so grid timestamps
/// land on clean boundaries instead of arbitrary values like 24s or 588s.
/// Explicit `resolution` values under the clamp, and `max_points`-derived
/// values, are returned as computed without nice-snapping.
pub fn resolve_resolution_s(params: &TimelineParams, total_window_s: f64) -> u64 {
    let total = total_window_s.max(1.0) as u64;

    if let Some(res) = params.resolution {
        let res = res.max(1);
        // Clamp so grid doesn't exceed 3600 points.
        let points = total / res;
        if points > 3600 {
            let raw = (total as f64 / 3600.0).ceil() as u64;
            return snap_up_to_nice(raw.max(1));
        }
        return res;
    }

    if let Some(mp) = params.max_points {
        let mp = mp.max(1);
        let res = (total as f64 / mp as f64).ceil() as u64;
        return res.max(1);
    }

    // Auto: target ~300 points, snapped to a nice bucket width.
    let raw = (total as f64 / 300.0).ceil() as u64;
    snap_up_to_nice(raw.max(1))
}

/// One point of a `/timeline/*` body: `values` is the sparse map of what is known at `ts` (NaN
/// dropped), `null` for a history bucket that holds no sample.
#[derive(Debug, Clone, Serialize, TS)]
pub struct TimelinePointBody {
    pub ts: DateTime<Utc>,
    pub values: Option<BTreeMap<String, f64>>,
}

impl TimelinePointBody {
    fn new(ts: DateTime<Utc>, values: Option<&HashMap<String, f64>>) -> Self {
        let values = values.map(|v| {
            v.iter()
                .filter(|(_, x)| !x.is_nan())
                .map(|(k, x)| (k.clone(), *x))
                .collect()
        });
        Self { ts, values }
    }
}

impl From<&crate::controller::trace::AssetTimelinePoint> for TimelinePointBody {
    fn from(p: &crate::controller::trace::AssetTimelinePoint) -> Self {
        Self::new(p.ts, Some(&p.values))
    }
}

/// One planning zone of the active plan, as a window on the time axis (the chart kit's `ZoneDef`).
#[derive(Debug, Clone, Serialize, TS)]
pub struct TimelineZone {
    pub from: DateTime<Utc>,
    pub to: DateTime<Utc>,
    pub step_s: u64,
}

/// `GET /timeline/all`: every asset's timeline plus `"grid"`, the plan's zones, and the server
/// clock the timestamps were built against (the UI positions its charts by this `now`, not by its
/// own clock).
#[derive(Debug, Clone, Serialize, TS)]
pub struct AllTimelinesResponse {
    pub zones: Vec<TimelineZone>,
    pub timelines: BTreeMap<String, Vec<TimelinePointBody>>,
    pub now: DateTime<Utc>,
}

/// GET /timeline/:asset_id — grid-aligned past+future timeline for one asset.
pub async fn get_timeline(
    State(state): State<AppState>,
    State(sim_read): State<SimRead>,
    Path(asset_id): Path<String>,
    Query(params): Query<TimelineParams>,
) -> impl IntoResponse {
    use crate::controller::timeline::compute_uniform_grid;

    let now = Utc::now();
    let hours_back = params.hours_back.unwrap_or(1.0);
    let hours_forward = params.hours_forward.unwrap_or(1.0);

    let total_window_s = (hours_back + hours_forward) * 3600.0;
    let resolution_s = resolve_resolution_s(&params, total_window_s);

    let window_start = now - chrono::Duration::milliseconds((hours_back * 3_600_000.0) as i64);
    let window_end = now + chrono::Duration::milliseconds((hours_forward * 3_600_000.0) as i64);
    let (history_grid, _future_grid) =
        compute_uniform_grid(window_start, window_end, now, resolution_s);

    let plan = state.active_plan().await;
    let snap = sim_read.timeline_snapshot(now).await;
    let known_assets: std::collections::HashSet<String> = snap.assets.keys().cloned().collect();

    match build_grid_aligned_array(
        &asset_id,
        &known_assets,
        &snap,
        plan.as_ref(),
        now,
        hours_back,
        hours_forward,
        &history_grid,
        resolution_s,
    ) {
        None => crate::routes::error::unknown_asset(&asset_id),
        Some(points) => Json(points).into_response(),
    }
}

/// Build a grid-aligned past + real-plan-slot future timeline array for one asset:
/// [history_grid (resampled)..., now_point, future (real plan slots, verbatim)...]
///
/// The past segment is resampled onto `history_grid` exactly as before. The future
/// segment is NOT resampled: `build_asset_timeline` already emits one real point per
/// real plan slot at its native per-zone step size (5/10/15 min), so it is serialized
/// verbatim — no averaging/blending across slots, and no synthetic grid timestamps.
#[allow(clippy::too_many_arguments)]
pub fn build_grid_aligned_array(
    asset_id: &str,
    known_assets: &std::collections::HashSet<String>,
    snap: &TimelineSnapshot,
    plan: Option<&crate::entities::plan::Plan>,
    now: DateTime<Utc>,
    hours_back: f64,
    hours_forward: f64,
    history_grid: &[DateTime<Utc>],
    resolution_s: u64,
) -> Option<Vec<TimelinePointBody>> {
    use crate::controller::timeline::{build_asset_timeline, build_now_point, resample_to_grid};
    use crate::entities::timeline::TimeWindow;

    let raw = build_asset_timeline(
        asset_id,
        known_assets,
        snap,
        plan,
        now,
        TimeWindow {
            hours_back,
            hours_forward,
        },
    )?;

    // Split raw points into history (ts < now, resampled) and future (ts >= now, verbatim).
    let raw_history: Vec<_> = raw.iter().filter(|p| p.ts < now).cloned().collect();
    let raw_future: Vec<_> = raw.into_iter().filter(|p| p.ts >= now).collect();

    let now_point = build_now_point(asset_id, now, snap);
    let hist_resampled = resample_to_grid(&raw_history, history_grid, resolution_s);

    // Concatenate: history_grid (resampled) + now_point + future (real plan slots).
    let mut out: Vec<TimelinePointBody> = history_grid
        .iter()
        .zip(&hist_resampled)
        .map(|(ts, values)| TimelinePointBody::new(*ts, values.as_ref()))
        .collect();
    out.push((&now_point).into());
    out.extend(raw_future.iter().map(TimelinePointBody::from));

    Some(out)
}

/// Build a zone list from the active plan, iterating all `plan.horizon.zones`.
/// Zones entirely in the past (zone_end <= now) are skipped.
/// Returns an empty list when no plan is active or all zones are expired.
fn zones_from_plan(
    plan: Option<&crate::entities::plan::Plan>,
    now: DateTime<Utc>,
) -> Vec<TimelineZone> {
    let Some(plan) = plan else { return vec![] };
    if plan.horizon.zones.is_empty() {
        return vec![];
    }
    let mut result = Vec::with_capacity(plan.horizon.zones.len());
    let mut cursor = plan.horizon.start_time;
    for zone in &plan.horizon.zones {
        let zone_end = cursor + chrono::Duration::seconds((zone.step_s * zone.slots as u64) as i64);
        if zone_end > now {
            result.push(TimelineZone {
                from: cursor,
                to: zone_end,
                step_s: zone.step_s,
            });
        }
        cursor = zone_end;
    }
    result
}

/// GET /timeline/all — merged timelines for all configured assets + "grid".
pub async fn get_timeline_all(
    State(state): State<AppState>,
    State(sim_read): State<SimRead>,
    Query(params): Query<TimelineParams>,
) -> impl IntoResponse {
    use crate::controller::timeline::compute_uniform_grid;

    let now = Utc::now();
    let hours_back = params.hours_back.unwrap_or(1.0);
    let hours_forward = params.hours_forward.unwrap_or(1.0);

    let total_window_s = (hours_back + hours_forward) * 3600.0;
    let resolution_s = resolve_resolution_s(&params, total_window_s);

    let window_start = now - chrono::Duration::milliseconds((hours_back * 3_600_000.0) as i64);
    let window_end = now + chrono::Duration::milliseconds((hours_forward * 3_600_000.0) as i64);
    let (history_grid, _future_grid) =
        compute_uniform_grid(window_start, window_end, now, resolution_s);

    let plan = state.active_plan().await;
    let snap = sim_read.timeline_snapshot(now).await;
    let known_assets: std::collections::HashSet<String> = snap.assets.keys().cloned().collect();

    let mut timelines = BTreeMap::new();

    // All sim assets
    for asset_id in &known_assets {
        if let Some(arr) = build_grid_aligned_array(
            asset_id,
            &known_assets,
            &snap,
            plan.as_ref(),
            now,
            hours_back,
            hours_forward,
            &history_grid,
            resolution_s,
        ) {
            timelines.insert(asset_id.clone(), arr);
        }
    }

    // Grid virtual asset
    if let Some(arr) = build_grid_aligned_array(
        "grid",
        &known_assets,
        &snap,
        plan.as_ref(),
        now,
        hours_back,
        hours_forward,
        &history_grid,
        resolution_s,
    ) {
        timelines.insert("grid".to_string(), arr);
    }

    let zones = zones_from_plan(plan.as_ref(), now);
    Json(AllTimelinesResponse {
        zones,
        timelines,
        now,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    fn auto_params() -> TimelineParams {
        TimelineParams {
            hours_back: None,
            hours_forward: None,
            resolution: None,
            max_points: None,
        }
    }

    /// The `/timeline/all` body as the VEN UI reads it, pinned field for field: a resampled
    /// history point, an empty history bucket (`values: null`), the now-point with a NaN value
    /// dropped, the real plan slots, and the zones. This JSON is the wire contract of
    /// `api/client.ts`'s `allTimelines`; a refactor of how it is built must leave it unchanged.
    #[test]
    fn all_timelines_body_wire_shape_is_pinned() {
        use crate::entities::timeline::TimelinePoint;
        let now: DateTime<Utc> = "2026-10-12T12:00:00Z".parse().unwrap();
        let plan = make_plan_with_net_import(300, now, &[1.0, 2.0]);
        let snap = TimelineSnapshot {
            assets: std::collections::HashMap::new(),
            grid_history: vec![TimelinePoint {
                ts: now - Duration::seconds(60),
                power_kw: 5.0,
                state_values: std::collections::HashMap::new(),
            }],
            grid_current_kw: f64::NAN,
        };
        let history_grid = vec![now - Duration::seconds(120), now - Duration::seconds(60)];
        let arr = build_grid_aligned_array(
            "grid",
            &std::collections::HashSet::new(),
            &snap,
            Some(&plan),
            now,
            120.0 / 3600.0,
            600.0 / 3600.0,
            &history_grid,
            60,
        )
        .expect("grid asset always resolves");
        let body = serde_json::to_value(AllTimelinesResponse {
            zones: zones_from_plan(Some(&plan), now),
            timelines: BTreeMap::from([("grid".to_string(), arr)]),
            now,
        })
        .unwrap();
        let expected = serde_json::json!({
            "now": "2026-10-12T12:00:00Z",
            "timelines": {
                "grid": [
                    { "ts": "2026-10-12T11:58:00Z", "values": null },
                    { "ts": "2026-10-12T11:59:00Z", "values": { "power_kw": 5.0 } },
                    { "ts": "2026-10-12T12:00:00Z", "values": {} },
                    { "ts": "2026-10-12T12:00:00Z", "values": {
                        "co2_rate_g_h": 300.0, "cost_rate_eur_h": 0.25,
                        "export_limit_kw": 10.0, "import_limit_kw": 25.0, "power_kw": 1.0 } },
                    { "ts": "2026-10-12T12:05:00Z", "values": {
                        "co2_rate_g_h": 600.0, "cost_rate_eur_h": 0.5,
                        "export_limit_kw": 10.0, "import_limit_kw": 25.0, "power_kw": 2.0 } }
                ]
            },
            "zones": [
                { "from": "2026-10-12T12:00:00Z", "to": "2026-10-12T12:10:00Z", "step_s": 300 }
            ]
        });
        assert_eq!(body, expected);
    }

    #[test]
    fn test_resolve_resolution_s_auto_default_view_snaps_to_nice_30s() {
        let params = auto_params();
        // 1h back + 1h forward = 7200s total; raw = ceil(7200/300) = 24 -> snap to 30
        assert_eq!(resolve_resolution_s(&params, 7200.0), 30);
    }

    #[test]
    fn test_resolve_resolution_s_auto_wide_forward_view_snaps_to_nice_600s() {
        let params = auto_params();
        // 1h back + 48h forward = 176400s total; raw = ceil(176400/300) = 588 -> snap to 600
        assert_eq!(resolve_resolution_s(&params, 176_400.0), 600);
    }

    #[test]
    fn test_resolve_resolution_s_auto_minimum_window_is_one() {
        let params = auto_params();
        assert_eq!(resolve_resolution_s(&params, 0.0), 1);
    }

    #[test]
    fn test_resolve_resolution_s_auto_already_nice_value_unchanged() {
        let params = auto_params();
        // total = 270000s; raw = ceil(270000/300) = 900, already nice -> unchanged
        assert_eq!(resolve_resolution_s(&params, 270_000.0), 900);
    }

    #[test]
    fn test_resolve_resolution_s_auto_very_wide_window_snaps_beyond_table() {
        let params = auto_params();
        // total = 34,560,000s (~400 days); raw = ceil(34_560_000/300) = 115200,
        // beyond the nice table's max (86400) -> snaps to next whole-day multiple (172800).
        assert_eq!(resolve_resolution_s(&params, 34_560_000.0), 172_800);
    }

    #[test]
    fn test_resolve_resolution_s_explicit_resolution_clamp_fallback_snaps_to_nice() {
        // Force the 3600-point clamp: explicit tiny resolution over a huge window.
        let params = TimelineParams {
            hours_back: None,
            hours_forward: None,
            resolution: Some(1),
            max_points: None,
        };
        // total = 14,400,000s; points = total/1 > 3600 -> clamp fallback;
        // raw = ceil(14_400_000/3600) = 4000 -> snap to 7200.
        assert_eq!(resolve_resolution_s(&params, 14_400_000.0), 7200);
    }

    #[test]
    fn test_resolve_resolution_s_explicit_resolution_under_cap_unchanged() {
        // resolution given, no clamp triggered -> passed through verbatim (not snapped).
        let params = TimelineParams {
            hours_back: None,
            hours_forward: None,
            resolution: Some(17),
            max_points: None,
        };
        assert_eq!(resolve_resolution_s(&params, 3600.0), 17);
    }

    #[test]
    fn test_resolve_resolution_s_max_points_branch_unchanged() {
        // max_points explicit -> not snapped, preserves existing division-based semantics.
        let params = TimelineParams {
            hours_back: None,
            hours_forward: None,
            resolution: None,
            max_points: Some(100),
        };
        assert_eq!(resolve_resolution_s(&params, 10_000.0), 100);
    }

    fn make_plan_with_step(
        step_s: u64,
        slots: usize,
        now: DateTime<Utc>,
    ) -> crate::entities::plan::Plan {
        use crate::entities::asset::PlanTrigger;
        use crate::entities::plan::{Plan, PlanTimeSlot, PlanZone, PlanningHorizon};
        use crate::entities::planner_params::PlannerObjective;
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
                planned_kw_by_asset: std::collections::HashMap::new(),
                planned_state_by_asset: std::collections::HashMap::new(),
                forecast_by_asset: Default::default(),
            })
            .collect();
        Plan {
            id: uuid::Uuid::new_v4(),
            created_at: now,
            trigger: PlanTrigger::Periodic,
            objective: PlannerObjective::MinCost,
            horizon,
            slots: plan_slots,
            objective_eur: 0.0,
            friction_eur: 0.0,
            cost_breakdown: Default::default(),
            planned_asset_ids: vec![],
            summary: Default::default(),
            session_flexibility: vec![],
            warnings: vec![],
            solve_status: crate::entities::plan::SolveStatus::Optimal,
            phase_report: None,
            penalty_rules_active: vec![],
            solver_ms: None,
            mip_gap_target: None,
        }
    }

    #[test]
    fn test_zones_from_plan_with_no_plan() {
        let now = chrono::Utc::now();
        let zones = zones_from_plan(None, now);
        assert!(zones.is_empty(), "no plan → empty zones list");
    }

    #[test]
    fn test_zones_from_plan_with_active_plan() {
        let now = chrono::Utc::now();
        let plan = make_plan_with_step(600, 288, now);
        let zones = zones_from_plan(Some(&plan), now);
        assert_eq!(zones.len(), 1, "single-zone plan produces one zone entry");
        assert_eq!(zones[0].step_s, 600);
        // from must be the plan's grid origin (horizon.start_time), not the request time.
        assert_eq!(
            zones[0].from, plan.horizon.start_time,
            "zone from must equal plan.horizon.start_time"
        );
    }

    #[test]
    fn test_zones_from_plan_three_zones() {
        use crate::entities::asset::PlanTrigger;
        use crate::entities::plan::{Plan, PlanZone, PlanningHorizon};
        use crate::entities::planner_params::PlannerObjective;
        let now = chrono::Utc::now();
        let zones = vec![
            PlanZone {
                step_s: 300,
                slots: 96,
            },
            PlanZone {
                step_s: 600,
                slots: 96,
            },
            PlanZone {
                step_s: 900,
                slots: 96,
            },
        ];
        let total_s: i64 = zones.iter().map(|z| z.step_s as i64 * z.slots as i64).sum();
        let horizon = PlanningHorizon {
            start_time: now,
            end_time: now + Duration::seconds(total_s),
            step_size_s: 300,
            num_steps: 288,
            far_horizon: now + Duration::seconds(total_s),
            zones: zones.clone(),
        };
        let plan = Plan {
            id: uuid::Uuid::new_v4(),
            created_at: now,
            trigger: PlanTrigger::Periodic,
            objective: PlannerObjective::MinCost,
            horizon,
            slots: vec![],
            objective_eur: 0.0,
            friction_eur: 0.0,
            cost_breakdown: Default::default(),
            planned_asset_ids: vec![],
            summary: Default::default(),
            session_flexibility: vec![],
            warnings: vec![],
            solve_status: crate::entities::plan::SolveStatus::Optimal,
            phase_report: None,
            penalty_rules_active: vec![],
            solver_ms: None,
            mip_gap_target: None,
        };
        let result = zones_from_plan(Some(&plan), now);
        assert_eq!(result.len(), 3, "3-zone plan must produce 3 zone entries");
        assert_eq!(result[0].step_s, 300);
        assert_eq!(result[1].step_s, 600);
        assert_eq!(result[2].step_s, 900);
        // Zone A: [now, now+28800s)
        assert_eq!(result[0].from, now);
        assert_eq!(result[0].to, now + Duration::seconds(300 * 96));
        // Zone B starts where A ends, zone C where B ends
        assert_eq!(result[1].from, result[0].to);
        assert_eq!(result[2].from, result[1].to);
    }

    #[test]
    fn test_zones_from_plan_with_expired_plan() {
        let past = chrono::Utc::now() - Duration::hours(50);
        let plan = make_plan_with_step(600, 288, past); // all slots in the past
        let now = chrono::Utc::now();
        let zones = zones_from_plan(Some(&plan), now);
        assert!(zones.is_empty(), "expired plan → empty zones list");
    }

    // ─── build_grid_aligned_array: future segment is real plan slots, verbatim ──

    /// Build a single-zone plan whose slots carry distinct `net_import_kw` values,
    /// so the "grid" virtual asset's future power_kw differs slot-to-slot.
    fn make_plan_with_net_import(
        step_s: u64,
        now: DateTime<Utc>,
        net_import_kw: &[f64],
    ) -> crate::entities::plan::Plan {
        use crate::entities::asset::PlanTrigger;
        use crate::entities::plan::{Plan, PlanTimeSlot, PlanZone, PlanningHorizon};
        use crate::entities::planner_params::PlannerObjective;
        let slots_n = net_import_kw.len();
        let horizon = PlanningHorizon {
            start_time: now,
            end_time: now + Duration::seconds((step_s * slots_n as u64) as i64),
            step_size_s: step_s,
            num_steps: slots_n,
            far_horizon: now + Duration::seconds((step_s * slots_n as u64) as i64),
            zones: vec![PlanZone {
                step_s,
                slots: slots_n,
            }],
        };
        let plan_slots: Vec<PlanTimeSlot> = net_import_kw
            .iter()
            .enumerate()
            .map(|(i, &imp)| PlanTimeSlot {
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
                net_import_kw: imp,
                net_export_kw: 0.0,
                import_flexibility_kw: 0.0,
                export_flexibility_kw: 0.0,
                planned_kw_by_asset: std::collections::HashMap::new(),
                planned_state_by_asset: std::collections::HashMap::new(),
                forecast_by_asset: Default::default(),
            })
            .collect();
        Plan {
            id: uuid::Uuid::new_v4(),
            created_at: now,
            trigger: PlanTrigger::Periodic,
            objective: PlannerObjective::MinCost,
            horizon,
            slots: plan_slots,
            objective_eur: 0.0,
            friction_eur: 0.0,
            cost_breakdown: Default::default(),
            planned_asset_ids: vec![],
            summary: Default::default(),
            session_flexibility: vec![],
            warnings: vec![],
            solve_status: crate::entities::plan::SolveStatus::Optimal,
            phase_report: None,
            penalty_rules_active: vec![],
            solver_ms: None,
            mip_gap_target: None,
        }
    }

    fn empty_snap() -> TimelineSnapshot {
        TimelineSnapshot {
            assets: std::collections::HashMap::new(),
            grid_history: vec![],
            grid_current_kw: 0.0,
        }
    }

    #[test]
    fn test_build_grid_aligned_array_future_ts_are_real_slot_boundaries() {
        let now = chrono::Utc::now();
        // 5-min-step zone, 4 slots; resolution deliberately coarser (600s) than the
        // real step (300s) — before this fix, that would have resampled/blended future
        // points onto 600s buckets.
        let plan = make_plan_with_net_import(300, now, &[1.0, 2.0, 3.0, 4.0]);
        let snap = empty_snap();
        let known_assets = std::collections::HashSet::new();

        let arr = build_grid_aligned_array(
            "grid",
            &known_assets,
            &snap,
            Some(&plan),
            now,
            0.0,
            4.0 * 300.0 / 3600.0,
            &[],
            600,
        )
        .expect("grid asset always resolves");

        // arr[0] is the now-point (history_grid is empty since hours_back=0.0); the
        // remaining entries must be exactly the 4 real plan slots, not resampled buckets.
        let future = &arr[1..];
        assert_eq!(
            future.len(),
            4,
            "one point per real plan slot, not resampled onto 600s buckets"
        );
        for (i, slot) in plan.slots.iter().enumerate() {
            assert_eq!(
                future[i].ts, slot.start,
                "future point {i} must be the real slot start, not a grid tick"
            );
        }
    }

    #[test]
    fn test_build_grid_aligned_array_future_values_are_not_blended() {
        let now = chrono::Utc::now();
        // Two adjacent 5-min slots with very different net_import_kw. A 600s-bucket
        // resample would have averaged them into one synthetic (1.0+9.0)/2 = 5.0 value;
        // real slot-verbatim output must keep both distinct values.
        let plan = make_plan_with_net_import(300, now, &[1.0, 9.0]);
        let snap = empty_snap();
        let known_assets = std::collections::HashSet::new();

        let arr = build_grid_aligned_array(
            "grid",
            &known_assets,
            &snap,
            Some(&plan),
            now,
            0.0,
            2.0 * 300.0 / 3600.0,
            &[],
            600,
        )
        .expect("grid asset always resolves");

        let future = &arr[1..];
        assert_eq!(future.len(), 2);
        let power_kw = |p: &TimelinePointBody| p.values.as_ref().unwrap()["power_kw"];
        assert_eq!(power_kw(&future[0]), 1.0);
        assert_eq!(power_kw(&future[1]), 9.0);
    }

    #[test]
    fn test_build_grid_aligned_array_future_point_count_independent_of_resolution() {
        let now = chrono::Utc::now();
        let plan = make_plan_with_net_import(300, now, &[1.0, 2.0, 3.0, 4.0]);
        let snap = empty_snap();
        let known_assets = std::collections::HashSet::new();

        // A fine resolution (60s) would previously have produced many LOCF-filled
        // sub-buckets per real slot; future point count must stay exactly the real
        // slot count regardless of resolution_s.
        let arr = build_grid_aligned_array(
            "grid",
            &known_assets,
            &snap,
            Some(&plan),
            now,
            0.0,
            4.0 * 300.0 / 3600.0,
            &[],
            60,
        )
        .expect("grid asset always resolves");

        assert_eq!(
            arr[1..].len(),
            4,
            "future length must equal real slot count, not depend on resolution_s"
        );
    }

    #[test]
    fn test_build_grid_aligned_array_history_still_grid_resampled() {
        use crate::entities::timeline::TimelinePoint;
        let now = chrono::Utc::now();
        let snap = TimelineSnapshot {
            assets: std::collections::HashMap::new(),
            grid_history: vec![
                TimelinePoint {
                    ts: now - Duration::seconds(60),
                    power_kw: 5.0,
                    state_values: std::collections::HashMap::new(),
                },
                TimelinePoint {
                    ts: now - Duration::seconds(30),
                    power_kw: 7.0,
                    state_values: std::collections::HashMap::new(),
                },
            ],
            grid_current_kw: 6.0,
        };
        let known_assets = std::collections::HashSet::new();
        let history_grid = vec![now - Duration::seconds(60), now - Duration::seconds(30)];

        let arr = build_grid_aligned_array(
            "grid",
            &known_assets,
            &snap,
            None,
            now,
            120.0 / 3600.0,
            0.0,
            &history_grid,
            30,
        )
        .expect("grid asset always resolves");

        // History side unchanged: still one grid-resampled entry per history_grid tick.
        assert_eq!(arr[0].ts, history_grid[0]);
        assert_eq!(arr[1].ts, history_grid[1]);
        for point in &arr[..2] {
            assert!(point.values.as_ref().unwrap().contains_key("power_kw"));
        }
    }
}
