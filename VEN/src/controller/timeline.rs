/// Timeline: merged past-history + future-plan data per asset.
///
/// `build_asset_timeline` assembles `Vec<AssetTimelinePoint>` by combining:
///   - Past:   rows from `AssetHistoryBuffer` within the requested back-window.
///   - Future: plan slot allocations for this asset within the forward-window,
///             enriched with cost/CO₂ rates from the slot's tariff values.
///
/// The "grid" virtual asset uses `net_import_kw`/`net_export_kw` from plan slots
/// and tariff interval keys for future; past comes from `history["grid"]` if present.
use chrono::{DateTime, Duration, Utc};
use std::collections::{HashMap, HashSet};

use crate::controller::trace::AssetTimelinePoint;
use crate::entities::plan::Plan;

// Data-carrier types now live in entities/timeline — re-exported for backward compatibility.
pub use crate::entities::timeline::{HeaterPlanTrajectory, TimelineSnapshot, TimeWindow};
#[cfg(test)]
use crate::entities::timeline::{TimelineAssetData, TimelinePoint};

// ─────────────────────────────────────────────────────────────────────────────
// Uniform grid resampling (RF-05c)
// ─────────────────────────────────────────────────────────────────────────────

/// Compute uniform grid timestamps snapped to round boundaries of `resolution_s`.
///
/// Returns `(history_grid, future_grid)` where:
/// - `history_grid` covers `[window_start..now)` snapped to multiples of `resolution_s`
/// - `future_grid` covers `(now..window_end]` snapped to multiples of `resolution_s`
///
/// Grid timestamps are deterministic: same resolution + window = same grid.
pub fn compute_uniform_grid(
    window_start: DateTime<Utc>,
    window_end: DateTime<Utc>,
    now: DateTime<Utc>,
    resolution_s: u64,
) -> (Vec<DateTime<Utc>>, Vec<DateTime<Utc>>) {
    let res = resolution_s.max(1) as i64;

    // Snap window_start up to next grid boundary.
    let start_epoch = window_start.timestamp();
    let first_grid = if start_epoch % res == 0 {
        start_epoch
    } else {
        start_epoch + (res - start_epoch.rem_euclid(res))
    };

    let now_epoch = now.timestamp();
    let end_epoch = window_end.timestamp();

    // Snap window_end down to grid boundary.
    let last_grid = end_epoch - end_epoch.rem_euclid(res);

    // History grid: all grid points in [first_grid, now_epoch)
    let mut history = Vec::new();
    let mut t = first_grid;
    while t < now_epoch {
        if let Some(dt) = DateTime::from_timestamp(t, 0) {
            history.push(dt);
        }
        t += res;
    }

    // Future grid: all grid points in (now_epoch, last_grid]
    // Start from the first grid point strictly after now.
    let future_start = if now_epoch % res == 0 {
        now_epoch + res
    } else {
        now_epoch + (res - now_epoch.rem_euclid(res))
    };
    let mut future = Vec::new();
    t = future_start;
    while t <= last_grid {
        if let Some(dt) = DateTime::from_timestamp(t, 0) {
            future.push(dt);
        }
        t += res;
    }

    (history, future)
}

/// Fill `None` slots in a resampled series by carrying the last `Some` value forward.
///
/// Used for forecast data: plan allocations are valid for the full 5-minute slot duration,
/// so sub-bucket nulls (caused by fine grid resolution between sparse plan points) should
/// hold the previous slot's value rather than rendering as gaps / needle peaks.
///
/// `seed` provides an initial value to carry forward for any leading `None` slots before
/// the first real plan point (covers the gap when the current plan slot started before `now`
/// and therefore landed in history rather than the future raw array).
///
/// History series are intentionally NOT passed through this — real data gaps must show as gaps.
pub fn locf_fill_nones(
    series: Vec<Option<HashMap<String, f64>>>,
    seed: Option<HashMap<String, f64>>,
) -> Vec<Option<HashMap<String, f64>>> {
    let mut last = seed;
    series
        .into_iter()
        .map(|slot| {
            if let Some(v) = slot {
                last = Some(v.clone());
                Some(v)
            } else {
                last.clone()
            }
        })
        .collect()
}

/// Resample raw sorted points onto a uniform grid using LOCF time-weighted mean.
///
/// For each grid timestamp, aggregates all raw points in `[grid_ts, grid_ts + resolution)`.
/// - Multiple rows: LOCF time-weighted average.
/// - Single row: that row's values.
/// - No rows: `None` (will be serialized as `{"ts": "...", "values": null}`).
pub fn resample_to_grid(
    raw: &[AssetTimelinePoint],
    grid: &[DateTime<Utc>],
    resolution_s: u64,
) -> Vec<Option<HashMap<String, f64>>> {
    let res_ms = (resolution_s.max(1) as i64) * 1000;

    grid.iter()
        .map(|&grid_ts| {
            let bucket_start_ms = grid_ts.timestamp_millis();
            let bucket_end_ms = bucket_start_ms + res_ms;

            // Collect rows in [bucket_start, bucket_end)
            let rows: Vec<&AssetTimelinePoint> = raw
                .iter()
                .filter(|p| {
                    let t = p.ts.timestamp_millis();
                    t >= bucket_start_ms && t < bucket_end_ms
                })
                .collect();

            if rows.is_empty() {
                return None;
            }

            // Collect all value keys across rows in this bucket.
            let mut all_keys: HashSet<&String> = HashSet::new();
            for r in &rows {
                for k in r.values.keys() {
                    all_keys.insert(k);
                }
            }

            // LOCF time-weighted mean per key.
            let mut result: HashMap<String, f64> = HashMap::new();
            let mut any_non_nan = false;

            for key in all_keys {
                let weighted_avg = locf_weighted_mean(&rows, key, bucket_start_ms, bucket_end_ms);
                if !weighted_avg.is_nan() {
                    any_non_nan = true;
                }
                result.insert(key.clone(), weighted_avg);
            }

            if any_non_nan {
                Some(result)
            } else {
                None
            }
        })
        .collect()
}

/// LOCF time-weighted mean for a single key across rows in a bucket.
fn locf_weighted_mean(
    rows: &[&AssetTimelinePoint],
    key: &str,
    bucket_start_ms: i64,
    bucket_end_ms: i64,
) -> f64 {
    let total_duration = (bucket_end_ms - bucket_start_ms) as f64;
    if total_duration <= 0.0 {
        return f64::NAN;
    }

    let mut weighted_sum = 0.0;
    let mut total_weight = 0.0;
    let mut last_value = f64::NAN;
    let mut last_time_ms = bucket_start_ms;

    for row in rows {
        let row_time_ms = row.ts.timestamp_millis().max(bucket_start_ms);
        let val = row.values.get(key).copied().unwrap_or(f64::NAN);

        // Weight the previous value (LOCF) for the interval [last_time, row_time)
        if !last_value.is_nan() && row_time_ms > last_time_ms {
            let dt = (row_time_ms - last_time_ms) as f64;
            weighted_sum += last_value * dt;
            total_weight += dt;
        }

        last_value = val;
        last_time_ms = row_time_ms;
    }

    // Carry the last value forward to bucket end.
    if !last_value.is_nan() && bucket_end_ms > last_time_ms {
        let dt = (bucket_end_ms - last_time_ms) as f64;
        weighted_sum += last_value * dt;
        total_weight += dt;
    }

    if total_weight > 0.0 {
        weighted_sum / total_weight
    } else {
        f64::NAN
    }
}

/// Build the now-point for an asset: instantaneous values at exact `now`.
///
/// All power smoothing (60s rolling average) and state value extraction happen
/// in `to_timeline_snapshot()` at the infra boundary; this function reads the
/// pre-computed values from `TimelineAssetData` directly.
/// Returns an empty-values point if the asset is unknown.
pub fn build_now_point(
    asset_id: &str,
    now: DateTime<Utc>,
    snap: &TimelineSnapshot,
) -> AssetTimelinePoint {
    // Regular sim assets — read pre-computed values from the domain snapshot.
    if let Some(data) = snap.assets.get(asset_id) {
        let mut values = data.current_state_values.clone();
        values.insert("power_kw".into(), data.current_power_kw);
        return AssetTimelinePoint { ts: now, values };
    }
    // Grid is stored separately; read its pre-computed scalar directly.
    if asset_id == "grid" {
        let mut values = HashMap::new();
        values.insert("power_kw".into(), snap.grid_current_kw);
        return AssetTimelinePoint { ts: now, values };
    }
    AssetTimelinePoint {
        ts: now,
        values: HashMap::new(),
    }
}

/// Build a merged past+future timeline for one asset.
///
/// Returns `None` when `asset_id` is unrecognised: not in `known_assets` and not `"grid"`.
/// An empty `Vec` is a valid return when no data falls within the requested window.
///
/// This function is pure (no I/O) and fully unit-testable.
pub fn build_asset_timeline(
    asset_id: &str,
    known_assets: &HashSet<String>,
    snap: &TimelineSnapshot,
    plan: Option<&Plan>,
    now: DateTime<Utc>,
    window: TimeWindow,
) -> Option<Vec<AssetTimelinePoint>> {
    let is_grid = asset_id == "grid";

    // Validate: must be a known sim asset or the virtual "grid" asset.
    if !is_grid && !known_assets.contains(asset_id) {
        return None;
    }

    let hours_back = window.hours_back.max(0.0);
    let hours_forward = window.hours_forward.max(0.0);

    let past_start = now - Duration::milliseconds((hours_back * 3_600_000.0) as i64);
    let future_end = now + Duration::milliseconds((hours_forward * 3_600_000.0) as i64);

    // ── Past: from pre-computed domain history ────────────────────────────────
    // state_values are pre-computed in to_timeline_snapshot() at the infra boundary.

    let mut points: Vec<AssetTimelinePoint> = if let Some(data) = snap.assets.get(asset_id) {
        data.history
            .iter()
            .filter(|p| p.ts >= past_start)
            .map(|p| {
                let mut values = p.state_values.clone();
                values.insert("power_kw".into(), p.power_kw);
                AssetTimelinePoint { ts: p.ts, values }
            })
            .collect()
    } else if is_grid {
        // Grid history is stored separately in the snapshot.
        snap.grid_history
            .iter()
            .filter(|p| p.ts >= past_start)
            .map(|p| {
                let mut values = HashMap::new();
                values.insert("power_kw".into(), p.power_kw);
                AssetTimelinePoint { ts: p.ts, values }
            })
            .collect()
    } else {
        vec![]
    };

    // ── Future: from plan slots ────────────────────────────────────────────────

    // Use the pre-computed plan trajectory from the domain snapshot (heater only).
    // For battery/EV, planned state comes from planned_state_by_asset in plan slots.
    let mut plan_traj = snap
        .assets
        .get(asset_id)
        .and_then(|d| d.plan_trajectory.clone());

    if let Some(plan) = plan {
        for slot in plan.all_slots() {
            // Only include slots whose start falls in [now, future_end).
            if slot.start < now || slot.start >= future_end {
                continue;
            }

            let mut values: HashMap<String, f64> = HashMap::new();

            if is_grid {
                // Grid virtual asset: net site power and tariff keys.
                let net_kw = slot.net_import_kw - slot.net_export_kw;
                values.insert("power_kw".into(), net_kw);
                // Importing: positive cost. Exporting: negative cost (revenue).
                let cost_rate = if net_kw >= 0.0 {
                    net_kw * slot.import_tariff_eur_kwh
                } else {
                    net_kw * slot.export_tariff_eur_kwh // negative = revenue
                };
                values.insert("cost_rate_eur_h".into(), cost_rate);
                // Negative co2_rate when exporting = displaced grid emissions.
                let co2_rate = net_kw * slot.co2_g_kwh;
                values.insert("co2_rate_g_h".into(), co2_rate);
                values.insert("import_limit_kw".into(), slot.import_cap_kw);
                values.insert("export_limit_kw".into(), slot.export_cap_kw);
            } else {
                // Physical asset: PV uses pv_forecast_kw (negate: generation is export-negative).
                // Base load uses baseline_kw — non-controllable, so no allocation is ever created.
                // All other assets use their packet allocation, or 0 kW if absent.
                // We always emit a point (even 0 kW) so that all assets share the same
                // set of plan-slot timestamps. Omitting zero-allocation slots causes the
                // stacked chart to fall back to an exact-match miss → false zero spikes.
                let power_kw = if asset_id == crate::ids::ASSET_PV {
                    -slot.pv_forecast_kw
                } else if asset_id == crate::ids::ASSET_BASE_LOAD {
                    slot.baseline_kw
                } else {
                    slot.allocations
                        .iter()
                        .find(|a| a.asset_id == asset_id)
                        .map(|a| a.power_kw)
                        .unwrap_or(0.0)
                };
                values.insert("power_kw".into(), power_kw);
                // Derive cost/CO2 rates from the allocation's pre-computed cost_eur / co2_g,
                // which already account for the PV-surplus vs grid split (see alloc_cost_eur).
                // PV has no allocation → both rates are 0 (no import cost for generation).
                let slot_h = (slot.end - slot.start).num_seconds() as f64 / 3600.0;
                let (cost_rate, co2_rate) = if slot_h > 0.0 {
                    slot.allocations
                        .iter()
                        .find(|a| a.asset_id == asset_id)
                        .map(|a| (a.cost_eur / slot_h, a.co2_g / slot_h))
                        .unwrap_or((0.0, 0.0))
                } else {
                    (0.0, 0.0)
                };
                values.insert("cost_rate_eur_h".into(), cost_rate);
                values.insert("co2_rate_g_h".into(), co2_rate);

                // Planned state values: recompute from live initial state when a
                // trajectory is available (heater), otherwise use stored MILP values
                // (battery SoC, EV SoC — replanned frequently enough to stay current).
                if let Some(ref mut traj) = plan_traj {
                    let dt_h = (slot.end - slot.start).num_seconds() as f64 / 3600.0;
                    let p_heat_kw = slot
                        .allocations
                        .iter()
                        .find(|a| a.asset_id == asset_id)
                        .map(|a| a.power_kw)
                        .unwrap_or(0.0);
                    values.extend(traj.next_slot(p_heat_kw, dt_h));
                } else if let Some(state_map) = slot.planned_state_by_asset.get(asset_id) {
                    values.extend(state_map.iter().map(|(k, v)| (k.clone(), *v)));
                }
            }

            points.push(AssetTimelinePoint {
                ts: slot.start,
                values,
            });
        }
    }

    // Sort ascending by timestamp.
    points.sort_by_key(|p| p.ts);

    Some(points)
}

// ─────────────────────────────────────────────────────────────────────────────
// Unit tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entities::asset::{AssetType, PlanTrigger};
    use crate::entities::plan::{
        AssetAllocation, CostBreakdown, Plan, PlanSummary, PlanTimeSlot, PlanningHorizon,
    };
    use chrono::Utc;
    use uuid::Uuid;

    fn ts(offset_s: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(1_700_000_000 + offset_s, 0).unwrap()
    }

    fn make_known(ids: &[&str]) -> HashSet<String> {
        ids.iter().map(|s| s.to_string()).collect()
    }

    /// Create a `TimelineAssetData` with history rows `(offset_s, power_kw)`.
    fn make_base_snap(id: &str, rows: &[(i64, f64)]) -> (String, TimelineAssetData) {
        let history: Vec<TimelinePoint> = rows
            .iter()
            .map(|(offset, power)| TimelinePoint {
                ts: ts(*offset),
                power_kw: *power,
                state_values: HashMap::from([("baseline_kw".to_string(), 0.0_f64)]),
            })
            .collect();
        let current_power_kw = history.last().map(|p| p.power_kw).unwrap_or(0.0);
        let current_state_values = HashMap::from([("baseline_kw".to_string(), 0.0_f64)]);
        (
            id.to_string(),
            TimelineAssetData {
                asset_id: id.to_string(),
                asset_type: AssetType::GenericConsumer,
                history,
                current_power_kw,
                current_state_values,
                plan_trajectory: None,
            },
        )
    }

    /// Create an EV `TimelineAssetData` with history rows `(offset_s, power_kw, soc)`.
    fn make_ev_snap(id: &str, rows: &[(i64, f64, f64)]) -> (String, TimelineAssetData) {
        let history: Vec<TimelinePoint> = rows
            .iter()
            .map(|(offset, power, soc)| TimelinePoint {
                ts: ts(*offset),
                power_kw: *power,
                state_values: HashMap::from([
                    ("soc".to_string(), *soc),
                    ("plugged".to_string(), 1.0_f64),
                ]),
            })
            .collect();
        let last = history.last();
        let current_power_kw = last.map(|p| p.power_kw).unwrap_or(0.0);
        let current_state_values = last.map(|p| p.state_values.clone()).unwrap_or_default();
        (
            id.to_string(),
            TimelineAssetData {
                asset_id: id.to_string(),
                asset_type: AssetType::Ev,
                history,
                current_power_kw,
                current_state_values,
                plan_trajectory: None,
            },
        )
    }

    /// Build a `TimelineSnapshot` from `(id, TimelineAssetData)` pairs.
    fn make_timeline_snap(entries: Vec<(String, TimelineAssetData)>) -> TimelineSnapshot {
        TimelineSnapshot {
            assets: entries.into_iter().collect(),
            grid_history: vec![],
            grid_current_kw: 0.0,
        }
    }

    fn empty_plan(now: DateTime<Utc>) -> Plan {
        Plan {
            id: Uuid::new_v4(),
            created_at: now,
            trigger: PlanTrigger::Periodic,
            horizon: PlanningHorizon {
                start_time: now,
                end_time: now + Duration::hours(2),
                step_size_s: 300,
                num_steps: 24,
                far_horizon: now + Duration::hours(2),
            },
            slots: vec![],
            summary: PlanSummary::default(),
            envelopes: vec![],
            warnings: vec![],
            soc_trajectory_kwh: vec![],
            objective: Default::default(),
            objective_eur: 0.0,
            friction_eur: 0.0,
            cost_breakdown: CostBreakdown::default(),
        }
    }

    fn make_slot(
        start_offset_s: i64,
        asset_id: &str,
        power_kw: f64,
        now: DateTime<Utc>,
    ) -> PlanTimeSlot {
        let start = now + Duration::seconds(start_offset_s);
        PlanTimeSlot {
            slot_index: 0,
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
            allocations: if asset_id.is_empty() {
                vec![]
            } else {
                vec![AssetAllocation {
                    asset_id: asset_id.to_string(),
                    power_kw,
                    surplus_power_kw: 0.0,
                    grid_power_kw: power_kw,
                    marginal_value: 1.0,
                    cost_eur: power_kw * 0.20 * (300.0 / 3600.0),
                    co2_g: power_kw * 300.0 * (300.0 / 3600.0),
                }]
            },
            net_import_kw: power_kw,
            net_export_kw: 0.0,
            import_flexibility_kw: 0.0,
            export_flexibility_kw: 0.0,
            bat_charge_kw: 0.0,
            bat_discharge_kw: 0.0,
            planned_kw_by_asset: std::collections::HashMap::from([(
                asset_id.to_string(),
                power_kw,
            )]),
            planned_state_by_asset: std::collections::HashMap::new(),
        }
    }

    #[test]
    fn unknown_asset_returns_none() {
        let known = make_known(&["ev"]);
        let snap = make_timeline_snap(vec![]);
        let now = Utc::now();
        let result = build_asset_timeline(
            "xyz",
            &known,
            &snap,
            None,
            now,
            TimeWindow {
                hours_back: 1.0,
                hours_forward: 1.0,
            },
        );
        assert!(result.is_none());
    }

    #[test]
    fn past_only_window_returns_history_points() {
        let now = ts(100);
        let known = make_known(&["ev"]);
        // 3 rows: ts(0), ts(50), ts(100) — all within 1h back from ts(100)
        let snap = make_timeline_snap(vec![make_base_snap(
            "ev",
            &[(0, 1.0), (50, 2.0), (100, 3.0)],
        )]);
        let result = build_asset_timeline(
            "ev",
            &known,
            &snap,
            None,
            now,
            TimeWindow {
                hours_back: 1.0,
                hours_forward: 0.0,
            },
        )
        .unwrap();
        assert_eq!(result.len(), 3);
        // Sorted ascending
        assert!(result[0].ts <= result[1].ts);
    }

    #[test]
    fn future_only_window_returns_plan_points() {
        let now = ts(0);
        let known = make_known(&["ev"]);
        let snap = make_timeline_snap(vec![]);
        let mut plan = empty_plan(now);
        // Slot starting 60s from now with EV allocation
        plan.slots.push(make_slot(60, "ev", 3.5, now));
        let result = build_asset_timeline(
            "ev",
            &known,
            &snap,
            Some(&plan),
            now,
            TimeWindow {
                hours_back: 0.0,
                hours_forward: 1.0,
            },
        )
        .unwrap();
        assert_eq!(result.len(), 1);
        let p = &result[0];
        assert!((p.values["power_kw"] - 3.5).abs() < 1e-9);
        // cost_rate = alloc.cost_eur / slot_h = (3.5 * 0.20 * (300/3600)) / (300/3600) = 3.5 * 0.20
        let expected_cost_rate = 3.5 * 0.20;
        assert!((p.values["cost_rate_eur_h"] - expected_cost_rate).abs() < 1e-9);
        let expected_co2_rate = 3.5 * 300.0;
        assert!((p.values["co2_rate_g_h"] - expected_co2_rate).abs() < 1e-9);
    }

    #[test]
    fn physical_asset_cost_rate_accounts_for_pv_surplus() {
        // Slot where EV allocation uses 1 kW from PV surplus and 2 kW from grid.
        let now = ts(0);
        let known = make_known(&["ev"]);
        let snap = make_timeline_snap(vec![]);
        let mut plan = empty_plan(now);
        let mut slot = make_slot(60, "", 0.0, now); // no default allocation
        let slot_h = 300.0 / 3600.0;
        slot.allocations.push(AssetAllocation {
            asset_id: "ev".to_string(),
            power_kw: 3.0,
            surplus_power_kw: 1.0,
            grid_power_kw: 2.0,
            marginal_value: 0.0,
            // 1 kW surplus at 0.05 + 2 kW grid at 0.20
            cost_eur: 1.0 * 0.05 * slot_h + 2.0 * 0.20 * slot_h,
            co2_g: 2.0 * 300.0 * slot_h,
        });
        plan.slots.push(slot);
        let result = build_asset_timeline(
            "ev",
            &known,
            &snap,
            Some(&plan),
            now,
            TimeWindow {
                hours_back: 0.0,
                hours_forward: 1.0,
            },
        )
        .unwrap();
        assert_eq!(result.len(), 1);
        let p = &result[0];
        let expected_cost_rate = 1.0 * 0.05 + 2.0 * 0.20; // EUR/h
        assert!((p.values["cost_rate_eur_h"] - expected_cost_rate).abs() < 1e-9);
        let expected_co2_rate = 2.0 * 300.0; // g/h (only grid portion)
        assert!((p.values["co2_rate_g_h"] - expected_co2_rate).abs() < 1e-9);
    }

    #[test]
    fn pv_asset_cost_rate_is_zero() {
        // PV has no allocation → cost_rate and co2_rate must be 0.
        let now = ts(0);
        let known = make_known(&["pv"]);
        let snap = make_timeline_snap(vec![]);
        let mut plan = empty_plan(now);
        let mut slot = make_slot(60, "", 0.0, now);
        slot.pv_forecast_kw = 4.0;
        plan.slots.push(slot);
        let result = build_asset_timeline(
            "pv",
            &known,
            &snap,
            Some(&plan),
            now,
            TimeWindow {
                hours_back: 0.0,
                hours_forward: 1.0,
            },
        )
        .unwrap();
        assert_eq!(result.len(), 1);
        let p = &result[0];
        assert!((p.values["power_kw"] - (-4.0)).abs() < 1e-9);
        assert_eq!(p.values["cost_rate_eur_h"], 0.0);
        assert_eq!(p.values["co2_rate_g_h"], 0.0);
    }

    #[test]
    fn merged_window_contains_both_past_and_future() {
        let now = ts(3600); // 1 hour into epoch
        let known = make_known(&["ev"]);
        // 2 past rows within 1h back
        let snap = make_timeline_snap(vec![make_base_snap("ev", &[(0, 1.0), (1800, 2.0)])]);
        let mut plan = empty_plan(now);
        plan.slots.push(make_slot(60, "ev", 3.0, now));
        let result = build_asset_timeline(
            "ev",
            &known,
            &snap,
            Some(&plan),
            now,
            TimeWindow {
                hours_back: 1.0,
                hours_forward: 1.0,
            },
        )
        .unwrap();
        // Both past rows (ts(0), ts(1800)) and 1 future point
        assert!(result.len() >= 3);
        // Sorted ascending
        for w in result.windows(2) {
            assert!(w[0].ts <= w[1].ts);
        }
    }

    #[test]
    fn grid_asset_returns_net_power_and_tariff_keys() {
        let now = ts(0);
        let known = make_known(&["ev"]);
        let snap = make_timeline_snap(vec![]);
        let mut plan = empty_plan(now);
        // Grid slot: net_import_kw = 2.0
        let mut slot = make_slot(60, "", 0.0, now);
        slot.net_import_kw = 2.0;
        slot.net_export_kw = 0.0;
        slot.import_tariff_eur_kwh = 0.25;
        slot.co2_g_kwh = 350.0;
        plan.slots.push(slot);

        let result = build_asset_timeline(
            "grid",
            &known,
            &snap,
            Some(&plan),
            now,
            TimeWindow {
                hours_back: 0.0,
                hours_forward: 1.0,
            },
        )
        .unwrap();
        assert_eq!(result.len(), 1);
        let p = &result[0];
        assert!((p.values["power_kw"] - 2.0).abs() < 1e-9);
        assert!(p.values.contains_key("import_limit_kw"));
    }

    #[test]
    fn grid_asset_net_export_gives_negative_cost_and_co2() {
        // Net export: PV > load → net_kw = -3.0; cost_rate and co2_rate must be negative.
        let now = ts(0);
        let known = make_known(&["pv"]);
        let snap = make_timeline_snap(vec![]);
        let mut plan = empty_plan(now);
        let mut slot = make_slot(60, "", 0.0, now);
        slot.net_import_kw = 0.0;
        slot.net_export_kw = 3.0;
        slot.import_tariff_eur_kwh = 0.25;
        slot.export_tariff_eur_kwh = 0.08;
        slot.co2_g_kwh = 300.0;
        plan.slots.push(slot);

        let result = build_asset_timeline(
            "grid",
            &known,
            &snap,
            Some(&plan),
            now,
            TimeWindow {
                hours_back: 0.0,
                hours_forward: 1.0,
            },
        )
        .unwrap();
        assert_eq!(result.len(), 1);
        let p = &result[0];
        assert!((p.values["power_kw"] - (-3.0)).abs() < 1e-9);
        // Revenue: −3.0 × 0.08 = −0.24
        assert!((p.values["cost_rate_eur_h"] - (-3.0 * 0.08)).abs() < 1e-9);
        // Displaced emissions: −3.0 × 300.0 = −900.0
        assert!((p.values["co2_rate_g_h"] - (-3.0 * 300.0)).abs() < 1e-9);
    }

    #[test]
    fn slot_without_asset_allocation_emits_zero_kw() {
        let now = ts(0);
        let known = make_known(&["ev", "battery"]);
        let snap = make_timeline_snap(vec![]);
        let mut plan = empty_plan(now);
        // Slot only has battery allocation, no EV allocation
        plan.slots.push(make_slot(60, "battery", 2.0, now));
        let result = build_asset_timeline(
            "ev",
            &known,
            &snap,
            Some(&plan),
            now,
            TimeWindow {
                hours_back: 0.0,
                hours_forward: 1.0,
            },
        )
        .unwrap();
        // No EV allocation → still emits a 0 kW point so timestamps stay aligned
        // across all assets (prevents false zero-spikes in the stacked area chart).
        assert_eq!(result.len(), 1);
        assert!((result[0].values["power_kw"]).abs() < 1e-9);
    }

    #[test]
    fn result_is_sorted_ascending() {
        let now = ts(3600);
        let known = make_known(&["ev"]);
        let snap = make_timeline_snap(vec![make_base_snap("ev", &[(1800, 1.0), (3600, 2.0)])]);
        let mut plan = empty_plan(now);
        plan.slots.push(make_slot(300, "ev", 3.0, now));
        plan.slots.push(make_slot(600, "ev", 3.5, now));
        let result = build_asset_timeline(
            "ev",
            &known,
            &snap,
            Some(&plan),
            now,
            TimeWindow {
                hours_back: 1.0,
                hours_forward: 1.0,
            },
        )
        .unwrap();
        for w in result.windows(2) {
            assert!(
                w[0].ts <= w[1].ts,
                "Result is not sorted: {:?} > {:?}",
                w[0].ts,
                w[1].ts
            );
        }
    }

    // ─── Uniform grid tests (RF-05c) ─────────────────────────────────────────

    #[test]
    fn compute_grid_uniform_spacing() {
        // now at epoch 1000, window [900..1100], resolution=10
        let now = DateTime::from_timestamp(1000, 0).unwrap();
        let start = DateTime::from_timestamp(900, 0).unwrap();
        let end = DateTime::from_timestamp(1100, 0).unwrap();
        let (hist, fut) = compute_uniform_grid(start, end, now, 10);

        // History: 900, 910, 920, ..., 990 (10 points)
        assert_eq!(hist.len(), 10);
        for w in hist.windows(2) {
            assert_eq!((w[1] - w[0]).num_seconds(), 10);
        }
        assert_eq!(hist[0].timestamp(), 900);
        assert_eq!(hist.last().unwrap().timestamp(), 990);

        // Future: 1010, 1020, ..., 1100 (10 points)
        assert_eq!(fut.len(), 10);
        for w in fut.windows(2) {
            assert_eq!((w[1] - w[0]).num_seconds(), 10);
        }
        assert_eq!(fut[0].timestamp(), 1010);
        assert_eq!(fut.last().unwrap().timestamp(), 1100);
    }

    #[test]
    fn compute_grid_snaps_to_round_boundaries() {
        // Window start at 903, should snap up to 910
        let now = DateTime::from_timestamp(1000, 0).unwrap();
        let start = DateTime::from_timestamp(903, 0).unwrap();
        let end = DateTime::from_timestamp(1097, 0).unwrap();
        let (hist, fut) = compute_uniform_grid(start, end, now, 10);

        assert_eq!(hist[0].timestamp(), 910);
        // End snaps down to 1090
        assert_eq!(fut.last().unwrap().timestamp(), 1090);
    }

    #[test]
    fn compute_grid_deterministic() {
        let now1 = DateTime::from_timestamp(1000, 0).unwrap();
        let now2 = DateTime::from_timestamp(1000, 0).unwrap();
        let start = DateTime::from_timestamp(900, 0).unwrap();
        let end = DateTime::from_timestamp(1100, 0).unwrap();
        let (h1, f1) = compute_uniform_grid(start, end, now1, 10);
        let (h2, f2) = compute_uniform_grid(start, end, now2, 10);
        assert_eq!(h1, h2);
        assert_eq!(f1, f2);
    }

    #[test]
    fn compute_grid_now_on_boundary() {
        // now is exactly on a grid boundary (1000, resolution=10)
        let now = DateTime::from_timestamp(1000, 0).unwrap();
        let start = DateTime::from_timestamp(980, 0).unwrap();
        let end = DateTime::from_timestamp(1020, 0).unwrap();
        let (hist, fut) = compute_uniform_grid(start, end, now, 10);

        // History should NOT include now itself (strictly < now)
        assert!(hist.iter().all(|t| t.timestamp() < 1000));
        // 980, 990
        assert_eq!(hist.len(), 2);

        // Future should NOT include now either (strictly > now)
        assert!(fut.iter().all(|t| t.timestamp() > 1000));
        // 1010, 1020
        assert_eq!(fut.len(), 2);
    }

    #[test]
    fn resample_multiple_rows_per_bucket() {
        // 3 rows in a single 10-second bucket: ts(0)=1.0, ts(3)=2.0, ts(7)=3.0
        let points = vec![
            AssetTimelinePoint {
                ts: ts(0),
                values: [("power_kw".into(), 1.0)].into(),
            },
            AssetTimelinePoint {
                ts: ts(3),
                values: [("power_kw".into(), 2.0)].into(),
            },
            AssetTimelinePoint {
                ts: ts(7),
                values: [("power_kw".into(), 3.0)].into(),
            },
        ];
        let grid = vec![ts(0)]; // single bucket [0, 10)
        let result = resample_to_grid(&points, &grid, 10);
        assert_eq!(result.len(), 1);
        let vals = result[0].as_ref().unwrap();
        // LOCF weighted mean: 1.0 * 3s + 2.0 * 4s + 3.0 * 3s = 3 + 8 + 9 = 20 / 10 = 2.0
        assert!((vals["power_kw"] - 2.0).abs() < 1e-9);
    }

    #[test]
    fn resample_single_row_per_bucket() {
        let points = vec![AssetTimelinePoint {
            ts: ts(2),
            values: [("power_kw".into(), 5.0)].into(),
        }];
        let grid = vec![ts(0)]; // bucket [0, 10)
        let result = resample_to_grid(&points, &grid, 10);
        let vals = result[0].as_ref().unwrap();
        // Single row at ts(2), LOCF carries to ts(10): 5.0 for 8 out of 10 seconds
        // But since it's the only value, weighted mean = 5.0
        assert!((vals["power_kw"] - 5.0).abs() < 1e-9);
    }

    #[test]
    fn resample_empty_bucket_returns_none() {
        let points: Vec<AssetTimelinePoint> = vec![];
        let grid = vec![ts(0)]; // bucket [0, 10) — no rows
        let result = resample_to_grid(&points, &grid, 10);
        assert!(result[0].is_none());
    }

    #[test]
    fn resample_nan_only_bucket_returns_none() {
        let points = vec![AssetTimelinePoint {
            ts: ts(0),
            values: [("power_kw".into(), f64::NAN)].into(),
        }];
        let grid = vec![ts(0)];
        let result = resample_to_grid(&points, &grid, 10);
        assert!(result[0].is_none());
    }

    #[test]
    fn build_now_point_uses_recent_average_for_power() {
        let now = ts(100);
        // Single point in window — average equals the single reading.
        let snap = make_timeline_snap(vec![make_ev_snap("ev", &[(99, 2.0, 0.6)])]);
        let np = build_now_point("ev", now, &snap);
        assert_eq!(np.ts, now);
        assert!((np.values["power_kw"] - 2.0).abs() < 1e-9);
        assert!((np.values["soc"] - 0.6).abs() < 1e-9);
    }

    #[test]
    fn build_now_point_smooths_oscillating_power() {
        // Smoothing (60s LOCF rolling average) is now computed in to_timeline_snapshot()
        // at the infra boundary. build_now_point reads the pre-computed current_power_kw
        // directly. This test verifies that the pre-computed value is passed through unchanged.
        let now = ts(100);
        let snap = make_timeline_snap(vec![(
            "osc".to_string(),
            TimelineAssetData {
                asset_id: "osc".to_string(),
                asset_type: AssetType::GenericConsumer,
                history: vec![],
                current_power_kw: 1.25, // pre-computed smoothed value (set by infra layer)
                current_state_values: HashMap::new(),
                plan_trajectory: None,
            },
        )]);
        let np = build_now_point("osc", now, &snap);
        // smoothing computed in to_timeline_snapshot(); this test verifies build_now_point reads the pre-computed value
        assert!((np.values["power_kw"] - 1.25).abs() < 1e-9);
    }

    #[test]
    fn build_now_point_empty_history() {
        let now = ts(100);
        let snap = make_timeline_snap(vec![]);
        let np = build_now_point("ev", now, &snap);
        assert_eq!(np.ts, now);
        assert!(np.values.is_empty());
    }

    // ─── LOCF fill tests ─────────────────────────────────────────────────────

    #[test]
    fn locf_fill_nones_fills_gaps() {
        let v1: HashMap<String, f64> = [("power_kw".into(), 7.4)].into();
        let v2: HashMap<String, f64> = [("power_kw".into(), 3.0)].into();
        let input: Vec<Option<HashMap<String, f64>>> =
            vec![Some(v1.clone()), None, None, Some(v2.clone()), None];
        let out = locf_fill_nones(input, None);
        // [7.4, 7.4, 7.4, 3.0, 3.0]
        assert!((out[0].as_ref().unwrap()["power_kw"] - 7.4).abs() < 1e-9);
        assert!((out[1].as_ref().unwrap()["power_kw"] - 7.4).abs() < 1e-9);
        assert!((out[2].as_ref().unwrap()["power_kw"] - 7.4).abs() < 1e-9);
        assert!((out[3].as_ref().unwrap()["power_kw"] - 3.0).abs() < 1e-9);
        assert!((out[4].as_ref().unwrap()["power_kw"] - 3.0).abs() < 1e-9);
    }

    #[test]
    fn locf_fill_nones_leading_nones_stay_none_without_seed() {
        let v: HashMap<String, f64> = [("power_kw".into(), 5.0)].into();
        let input: Vec<Option<HashMap<String, f64>>> = vec![None, None, Some(v.clone()), None];
        let out = locf_fill_nones(input, None);
        // Leading Nones have no predecessor and no seed — remain None
        assert!(out[0].is_none());
        assert!(out[1].is_none());
        assert!((out[2].as_ref().unwrap()["power_kw"] - 5.0).abs() < 1e-9);
        assert!((out[3].as_ref().unwrap()["power_kw"] - 5.0).abs() < 1e-9);
    }

    #[test]
    fn locf_fill_nones_seed_fills_leading_nones() {
        let seed: HashMap<String, f64> = [("power_kw".into(), 2.0)].into();
        let v: HashMap<String, f64> = [("power_kw".into(), 5.0)].into();
        let input: Vec<Option<HashMap<String, f64>>> = vec![None, None, Some(v.clone()), None];
        let out = locf_fill_nones(input, Some(seed));
        // Leading Nones filled from seed; once a real value arrives, carry that instead
        assert!((out[0].as_ref().unwrap()["power_kw"] - 2.0).abs() < 1e-9);
        assert!((out[1].as_ref().unwrap()["power_kw"] - 2.0).abs() < 1e-9);
        assert!((out[2].as_ref().unwrap()["power_kw"] - 5.0).abs() < 1e-9);
        assert!((out[3].as_ref().unwrap()["power_kw"] - 5.0).abs() < 1e-9);
    }

    #[test]
    fn locf_fill_nones_all_some_unchanged() {
        let v: HashMap<String, f64> = [("power_kw".into(), 1.0)].into();
        let input: Vec<Option<HashMap<String, f64>>> = vec![Some(v.clone()), Some(v.clone())];
        let out = locf_fill_nones(input, None);
        assert!(out.iter().all(|s| s.is_some()));
    }

    #[test]
    fn locf_fill_nones_empty_passthrough() {
        let out = locf_fill_nones(vec![], None);
        assert!(out.is_empty());
    }

    // ── PV forecast path ──────────────────────────────────────────────────────

    /// Build a slot whose pv_forecast_kw is set but has no PV allocation.
    fn make_pv_slot(start_offset_s: i64, pv_forecast_kw: f64, now: DateTime<Utc>) -> PlanTimeSlot {
        let start = now + Duration::seconds(start_offset_s);
        PlanTimeSlot {
            slot_index: 0,
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
            pv_forecast_kw,
            surplus_available_kw: pv_forecast_kw.max(0.0),
            allocations: vec![], // intentionally empty — PV should not appear here
            net_import_kw: (0.5_f64 - pv_forecast_kw).max(0.0),
            net_export_kw: (pv_forecast_kw - 0.5_f64).max(0.0),
            import_flexibility_kw: 0.0,
            export_flexibility_kw: 0.0,
            bat_charge_kw: 0.0,
            bat_discharge_kw: 0.0,
            planned_kw_by_asset: std::collections::HashMap::new(),
            planned_state_by_asset: std::collections::HashMap::new(),
        }
    }

    #[test]
    fn pv_future_uses_pv_forecast_kw_not_allocations() {
        // slot has pv_forecast_kw = 5.0 but NO pv allocation entry.
        // build_asset_timeline must return power_kw = -5.0 (negative = export).
        let now = Utc::now();
        let known = make_known(&["pv"]);
        let snap = make_timeline_snap(vec![]);

        let slot = make_pv_slot(60, 5.0, now); // 60 s in future
        let plan = Plan {
            id: Uuid::new_v4(),
            created_at: now,
            trigger: crate::entities::asset::PlanTrigger::Periodic,
            horizon: crate::entities::plan::PlanningHorizon {
                start_time: now,
                end_time: now + Duration::hours(24),
                step_size_s: 300,
                num_steps: 288,
                far_horizon: now + Duration::hours(24),
            },
            slots: vec![slot],
            summary: PlanSummary::default(),
            envelopes: vec![],
            warnings: vec![],
            soc_trajectory_kwh: vec![],
            objective: Default::default(),
            objective_eur: 0.0,
            friction_eur: 0.0,
            cost_breakdown: CostBreakdown::default(),
        };

        let points = build_asset_timeline(
            "pv",
            &known,
            &snap,
            Some(&plan),
            now,
            TimeWindow {
                hours_back: 0.0,
                hours_forward: 1.0,
            },
        )
        .expect("pv is a known asset");

        let future: Vec<_> = points.iter().filter(|p| p.ts > now).collect();
        assert_eq!(future.len(), 1, "expected exactly one future point");
        let power_kw = future[0].values["power_kw"];
        assert!(
            (power_kw - (-5.0)).abs() < 1e-9,
            "expected power_kw = -5.0 (pv export), got {power_kw}"
        );
    }

    #[test]
    fn pv_future_zero_when_forecast_zero() {
        // Night slot: pv_forecast_kw = 0.0 → power_kw = 0.0 (no generation).
        let now = Utc::now();
        let known = make_known(&["pv"]);
        let snap = make_timeline_snap(vec![]);

        let slot = make_pv_slot(60, 0.0, now);
        let plan = Plan {
            id: Uuid::new_v4(),
            created_at: now,
            trigger: crate::entities::asset::PlanTrigger::Periodic,
            horizon: crate::entities::plan::PlanningHorizon {
                start_time: now,
                end_time: now + Duration::hours(24),
                step_size_s: 300,
                num_steps: 288,
                far_horizon: now + Duration::hours(24),
            },
            slots: vec![slot],
            summary: PlanSummary::default(),
            envelopes: vec![],
            warnings: vec![],
            soc_trajectory_kwh: vec![],
            objective: Default::default(),
            objective_eur: 0.0,
            friction_eur: 0.0,
            cost_breakdown: CostBreakdown::default(),
        };

        let points = build_asset_timeline(
            "pv",
            &known,
            &snap,
            Some(&plan),
            now,
            TimeWindow {
                hours_back: 0.0,
                hours_forward: 1.0,
            },
        )
        .expect("pv is a known asset");

        let future: Vec<_> = points.iter().filter(|p| p.ts > now).collect();
        assert_eq!(future.len(), 1);
        let power_kw = future[0].values["power_kw"];
        assert!(
            power_kw.abs() < 1e-9,
            "expected 0.0 at night, got {power_kw}"
        );
    }

    // T010: planned_state_by_asset values are merged into future timeline points.
    #[test]
    fn planned_state_merged_into_future_point_values() {
        let now = ts(0);
        let known = make_known(&["battery-1"]);
        let snap = make_timeline_snap(vec![]);
        let mut plan = empty_plan(now);
        let mut slot = make_slot(60, "battery-1", 2.0, now);
        slot.planned_state_by_asset.insert(
            "battery-1".to_string(),
            std::collections::HashMap::from([("soc".to_string(), 0.75_f64)]),
        );
        plan.slots.push(slot);

        let result = build_asset_timeline(
            "battery-1",
            &known,
            &snap,
            Some(&plan),
            now,
            TimeWindow {
                hours_back: 0.0,
                hours_forward: 1.0,
            },
        )
        .unwrap();

        assert_eq!(result.len(), 1);
        let p = &result[0];
        assert!(p.ts > now);
        let soc = p.values.get("soc").copied().expect("soc key missing");
        assert!((soc - 0.75).abs() < 1e-9, "expected soc=0.75, got {soc}");
        // power_kw must also still be present
        assert!(p.values.contains_key("power_kw"), "power_kw key missing");
    }
}
