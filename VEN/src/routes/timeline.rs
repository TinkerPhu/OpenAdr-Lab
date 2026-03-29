use axum::{
    extract::{Path, Query, State},
    response::IntoResponse,
    Json,
};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use std::collections::HashMap;

use crate::simulator::SimState;

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
    use std::collections::HashSet;

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
    let sim_guard = ctx.sim.lock().await;
    let known_assets: HashSet<String> = sim_guard.assets.iter().map(|e| e.id.clone()).collect();

    match build_grid_aligned_array(
        &asset_id,
        &known_assets,
        &*sim_guard,
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
pub fn build_grid_aligned_array(
    asset_id: &str,
    known_assets: &std::collections::HashSet<String>,
    sim: &SimState,
    plan: Option<&crate::entities::plan::Plan>,
    now: DateTime<Utc>,
    hours_back: f64,
    hours_forward: f64,
    history_grid: &[DateTime<Utc>],
    future_grid: &[DateTime<Utc>],
    resolution_s: u64,
) -> Option<Vec<serde_json::Value>> {
    use crate::controller::timeline::{
        build_asset_timeline, build_now_point, locf_fill_nones, resample_to_grid, TimeWindow,
    };

    let raw = build_asset_timeline(
        asset_id,
        known_assets,
        sim,
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

    // Resample onto grids.
    // Future: apply LOCF fill so plan-slot values extend across all fine-grid buckets
    // within a 5-minute slot rather than leaving sub-bucket gaps that render as needle peaks.
    let hist_resampled = resample_to_grid(&raw_history, history_grid, resolution_s);
    let fut_resampled = locf_fill_nones(resample_to_grid(&raw_future, future_grid, resolution_s));

    // Build now-point.
    let now_point = build_now_point(asset_id, now, sim);

    // Concatenate: history_grid + now_point + future_grid.
    let mut out = serialize_grid_timeline(history_grid, &hist_resampled);
    out.push(serialize_now_point(&now_point));
    out.extend(serialize_grid_timeline(future_grid, &fut_resampled));

    Some(out)
}

/// GET /timeline/all — merged timelines for all configured assets + "grid".
pub async fn get_timeline_all(
    State(ctx): State<AppCtx>,
    Query(params): Query<TimelineParams>,
) -> impl IntoResponse {
    use crate::controller::timeline::compute_uniform_grid;
    use std::collections::HashSet;

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
    let sim_guard = ctx.sim.lock().await;
    let known_assets: HashSet<String> = sim_guard.assets.iter().map(|e| e.id.clone()).collect();

    let mut result: serde_json::Map<String, serde_json::Value> = serde_json::Map::new();

    // All sim assets
    for asset_id in &known_assets {
        if let Some(arr) = build_grid_aligned_array(
            asset_id,
            &known_assets,
            &*sim_guard,
            plan.as_ref(),
            now,
            hours_back,
            hours_forward,
            &history_grid,
            &future_grid,
            resolution_s,
        ) {
            result.insert(asset_id.clone(), serde_json::Value::Array(arr));
        }
    }

    // Grid virtual asset
    if let Some(arr) = build_grid_aligned_array(
        "grid",
        &known_assets,
        &*sim_guard,
        plan.as_ref(),
        now,
        hours_back,
        hours_forward,
        &history_grid,
        &future_grid,
        resolution_s,
    ) {
        result.insert("grid".to_string(), serde_json::Value::Array(arr));
    }

    Json(serde_json::Value::Object(result))
}
