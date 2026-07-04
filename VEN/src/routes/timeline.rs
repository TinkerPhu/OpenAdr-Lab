use axum::{
    extract::{Path, Query, State},
    response::IntoResponse,
    Json,
};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use std::collections::HashMap;

use crate::entities::timeline::TimelineSnapshot;

use crate::AppCtx;

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

/// Serialize a grid-aligned timeline: each entry is either a real point or null-valued.
/// Grid timestamps + resampled Option<HashMap> values → JSON array.
/// A `None` values entry serializes as `{"ts": "...", "values": null}`.
pub fn serialize_grid_timeline(
    timestamps: &[DateTime<Utc>],
    values: &[Option<HashMap<String, f64>>],
) -> Vec<serde_json::Value> {
    timestamps
        .iter()
        .zip(values.iter())
        .map(|(ts, opt_vals)| match opt_vals {
            Some(vals) => {
                let map: serde_json::Map<String, serde_json::Value> = vals
                    .iter()
                    .filter(|(_, v)| !v.is_nan())
                    .map(|(k, v)| (k.clone(), serde_json::json!(v)))
                    .collect();
                serde_json::json!({ "ts": ts, "values": map })
            }
            None => serde_json::json!({ "ts": ts, "values": null }),
        })
        .collect()
}

/// Serialize a single AssetTimelinePoint (the now-point) to JSON.
pub fn serialize_now_point(
    point: &crate::controller::trace::AssetTimelinePoint,
) -> serde_json::Value {
    let values: serde_json::Map<String, serde_json::Value> = point
        .values
        .iter()
        .filter(|(_, v)| !v.is_nan())
        .map(|(k, v)| (k.clone(), serde_json::json!(v)))
        .collect();
    serde_json::json!({ "ts": point.ts, "values": values })
}

/// GET /timeline/:asset_id — grid-aligned past+future timeline for one asset.
pub async fn get_timeline(
    State(ctx): State<AppCtx>,
    Path(asset_id): Path<String>,
    Query(params): Query<TimelineParams>,
) -> impl IntoResponse {
    use crate::controller::timeline::compute_uniform_grid;
    use axum::http::StatusCode;

    let now = Utc::now();
    let hours_back = params.hours_back.unwrap_or(1.0);
    let hours_forward = params.hours_forward.unwrap_or(1.0);

    let total_window_s = (hours_back + hours_forward) * 3600.0;
    let resolution_s = resolve_resolution_s(&params, total_window_s);

    let window_start = now - chrono::Duration::milliseconds((hours_back * 3_600_000.0) as i64);
    let window_end = now + chrono::Duration::milliseconds((hours_forward * 3_600_000.0) as i64);
    let (history_grid, future_grid) =
        compute_uniform_grid(window_start, window_end, now, resolution_s);

    let plan = ctx.state.active_plan().await;
    let snap = ctx.sim.lock().await.to_timeline_snapshot();
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
        &future_grid,
        resolution_s,
    ) {
        None => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": format!("unknown asset: {}", asset_id) })),
        )
            .into_response(),
        Some(arr) => Json(serde_json::Value::Array(arr)).into_response(),
    }
}

/// Build a grid-aligned timeline array for one asset:
/// [history_grid..., now_point, future_grid...]
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
    future_grid: &[DateTime<Utc>],
    resolution_s: u64,
) -> Option<Vec<serde_json::Value>> {
    use crate::controller::timeline::{
        build_asset_timeline, build_now_point, locf_fill_nones, resample_to_grid,
    };
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

    // Split raw points into history (ts < now) and future (ts >= now).
    let raw_history: Vec<_> = raw.iter().filter(|p| p.ts < now).cloned().collect();
    let raw_future: Vec<_> = raw.iter().filter(|p| p.ts >= now).cloned().collect();

    // Build now-point before future resampling so we can use it as a LOCF seed.
    // This covers the gap when the currently-active plan slot started before `now` and
    // therefore fell into raw_history — without a seed the leading future buckets are null.
    let now_point = build_now_point(asset_id, now, snap);
    let fut_seed = if now_point.values.is_empty() {
        None
    } else {
        Some(now_point.values.clone())
    };

    // Resample onto grids.
    // Future: apply LOCF fill so plan-slot values extend across all fine-grid buckets
    // within a 5-minute slot rather than leaving sub-bucket gaps that render as needle peaks.
    let hist_resampled = resample_to_grid(&raw_history, history_grid, resolution_s);

    // Plan horizon end: last slot end across all plan slots, or None when no plan is active.
    // Future grid points strictly after this boundary are nulled out — the LOCF seed must not
    // fill an unbounded future with stale values beyond what the plan actually covers.
    let plan_end_opt: Option<DateTime<Utc>> = plan.and_then(|p| p.all_slots().map(|s| s.end).max());

    let fut_resampled: Vec<Option<std::collections::HashMap<String, f64>>> = locf_fill_nones(
        resample_to_grid(&raw_future, future_grid, resolution_s),
        fut_seed,
    )
    .into_iter()
    .zip(future_grid.iter())
    .map(|(v, &ts)| match plan_end_opt {
        Some(end) if ts <= end => v,
        _ => None,
    })
    .collect();

    // Concatenate: history_grid + now_point + future_grid.
    let mut out = serialize_grid_timeline(history_grid, &hist_resampled);
    out.push(serialize_now_point(&now_point));
    out.extend(serialize_grid_timeline(future_grid, &fut_resampled));

    Some(out)
}

/// Build a zone list from the active plan, iterating all `plan.horizon.zones`.
/// Zones entirely in the past (zone_end <= now) are skipped.
/// Returns an empty list when no plan is active or all zones are expired.
fn zones_from_plan(
    plan: Option<&crate::entities::plan::Plan>,
    now: DateTime<Utc>,
) -> Vec<serde_json::Value> {
    let Some(plan) = plan else { return vec![] };
    if plan.horizon.zones.is_empty() {
        return vec![];
    }
    let mut result = Vec::with_capacity(plan.horizon.zones.len());
    let mut cursor = plan.horizon.start_time;
    for zone in &plan.horizon.zones {
        let zone_end = cursor + chrono::Duration::seconds((zone.step_s * zone.slots as u64) as i64);
        if zone_end > now {
            result.push(serde_json::json!({
                "from": cursor,
                "to": zone_end,
                "step_s": zone.step_s,
            }));
        }
        cursor = zone_end;
    }
    result
}

/// GET /timeline/all — merged timelines for all configured assets + "grid".
pub async fn get_timeline_all(
    State(ctx): State<AppCtx>,
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
    let (history_grid, future_grid) =
        compute_uniform_grid(window_start, window_end, now, resolution_s);

    let plan = ctx.state.active_plan().await;
    let snap = ctx.sim.lock().await.to_timeline_snapshot();
    let known_assets: std::collections::HashSet<String> = snap.assets.keys().cloned().collect();

    let mut timelines: serde_json::Map<String, serde_json::Value> = serde_json::Map::new();

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
            &future_grid,
            resolution_s,
        ) {
            timelines.insert(asset_id.clone(), serde_json::Value::Array(arr));
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
        &future_grid,
        resolution_s,
    ) {
        timelines.insert("grid".to_string(), serde_json::Value::Array(arr));
    }

    let zones = zones_from_plan(plan.as_ref(), now);
    Json(serde_json::json!({ "zones": zones, "timelines": timelines }))
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
                rate_estimated: false,
                import_cap_kw: 25.0,
                export_cap_kw: 10.0,
                allocations: vec![],
                pv_forecast_kw: 0.0,
                baseline_kw: 0.0,
                surplus_available_kw: 0.0,
                net_import_kw: 0.0,
                net_export_kw: 0.0,
                import_flexibility_kw: 0.0,
                export_flexibility_kw: 0.0,
                planned_kw_by_asset: std::collections::HashMap::new(),
                planned_state_by_asset: std::collections::HashMap::new(),
                bat_charge_kw: 0.0,
                bat_discharge_kw: 0.0,
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
            soc_trajectory_kwh: vec![],
            summary: Default::default(),
            envelopes: vec![],
            warnings: vec![],
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
        assert_eq!(zones[0]["step_s"], 600);
        // from must be the plan's grid origin (horizon.start_time), not the request time.
        // Parse both sides to DateTime to avoid Z vs +00:00 format differences.
        let zone_from: chrono::DateTime<chrono::Utc> = zones[0]["from"]
            .as_str()
            .unwrap()
            .parse()
            .expect("zone from must be a valid RFC 3339 timestamp");
        assert_eq!(
            zone_from, plan.horizon.start_time,
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
            soc_trajectory_kwh: vec![],
            summary: Default::default(),
            envelopes: vec![],
            warnings: vec![],
        };
        let result = zones_from_plan(Some(&plan), now);
        assert_eq!(result.len(), 3, "3-zone plan must produce 3 zone entries");
        assert_eq!(result[0]["step_s"], 300);
        assert_eq!(result[1]["step_s"], 600);
        assert_eq!(result[2]["step_s"], 900);
        // Zone A: [now, now+28800s)
        let z0_from: chrono::DateTime<chrono::Utc> =
            result[0]["from"].as_str().unwrap().parse().unwrap();
        let z0_to: chrono::DateTime<chrono::Utc> =
            result[0]["to"].as_str().unwrap().parse().unwrap();
        assert_eq!(z0_from, now);
        assert_eq!(z0_to, now + Duration::seconds(300 * 96));
        // Zone B starts where A ends
        let z1_from: chrono::DateTime<chrono::Utc> =
            result[1]["from"].as_str().unwrap().parse().unwrap();
        assert_eq!(z1_from, z0_to);
        // Zone C starts where B ends
        let z1_to: chrono::DateTime<chrono::Utc> =
            result[1]["to"].as_str().unwrap().parse().unwrap();
        let z2_from: chrono::DateTime<chrono::Utc> =
            result[2]["from"].as_str().unwrap().parse().unwrap();
        assert_eq!(z2_from, z1_to);
    }

    #[test]
    fn test_zones_from_plan_with_expired_plan() {
        let past = chrono::Utc::now() - Duration::hours(50);
        let plan = make_plan_with_step(600, 288, past); // all slots in the past
        let now = chrono::Utc::now();
        let zones = zones_from_plan(Some(&plan), now);
        assert!(zones.is_empty(), "expired plan → empty zones list");
    }
}
