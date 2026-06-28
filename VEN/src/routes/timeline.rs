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

/// Resolve the grid resolution in seconds from query parameters.
/// Priority: resolution > max_points > auto (~300 points).
/// Result is clamped so the grid has at most 3600 points.
pub fn resolve_resolution_s(params: &TimelineParams, total_window_s: f64) -> u64 {
    let total = total_window_s.max(1.0) as u64;

    if let Some(res) = params.resolution {
        let res = res.max(1);
        // Clamp so grid doesn't exceed 3600 points.
        let points = total / res;
        if points > 3600 {
            return (total as f64 / 3600.0).ceil() as u64;
        }
        return res;
    }

    if let Some(mp) = params.max_points {
        let mp = mp.max(1);
        let res = (total as f64 / mp as f64).ceil() as u64;
        return res.max(1);
    }

    // Auto: target ~300 points.
    let res = (total as f64 / 300.0).ceil() as u64;
    res.max(1)
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

/// Build a zone list from the active plan.
/// Returns a single zone covering [horizon.start_time, plan_end) with the plan's step_size_s.
/// Returns an empty list when no plan is active or the plan has no future slots.
fn zones_from_plan(
    plan: Option<&crate::entities::plan::Plan>,
    now: DateTime<Utc>,
) -> Vec<serde_json::Value> {
    let Some(plan) = plan else { return vec![] };
    let step_s = plan.horizon.step_size_s;
    let Some(end) = plan.all_slots().map(|s| s.end).max() else {
        return vec![];
    };
    if end <= now {
        return vec![];
    }
    vec![serde_json::json!({ "from": plan.horizon.start_time, "to": end, "step_s": step_s })]
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
    fn test_zones_from_plan_with_expired_plan() {
        let past = chrono::Utc::now() - Duration::hours(50);
        let plan = make_plan_with_step(600, 288, past); // all slots in the past
        let now = chrono::Utc::now();
        let zones = zones_from_plan(Some(&plan), now);
        assert!(zones.is_empty(), "expired plan → empty zones list");
    }
}
