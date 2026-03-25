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
use crate::simulator::SimState;

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
/// Uses the most recent `HistoryPoint` from the per-asset ring buffer.
/// Returns an empty-values point if no history exists.
pub fn build_now_point(asset_id: &str, now: DateTime<Utc>, sim: &SimState) -> AssetTimelinePoint {
    if let Some((entry, cfg)) = sim.find_asset(asset_id) {
        if let Some(last) = entry.history.latest() {
            let mut values = cfg.state_values(&last.state);
            values.insert("power_kw".into(), last.power_kw);
            return AssetTimelinePoint { ts: now, values };
        }
    }
    AssetTimelinePoint {
        ts: now,
        values: HashMap::new(),
    }
}

/// Time window parameters for `build_asset_timeline`.
pub struct TimeWindow {
    /// Hours of history to include (clamped to ≥ 0).
    pub hours_back: f64,
    /// Hours of future plan to include (clamped to ≥ 0).
    pub hours_forward: f64,
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
    sim: &SimState,
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

    // ── Past: from per-asset history ring buffer ───────────────────────────────

    let mut points: Vec<AssetTimelinePoint> = if let Some((entry, cfg)) = sim.find_asset(asset_id) {
        let back_window = Duration::milliseconds((hours_back * 3_600_000.0) as i64);
        entry
            .history
            .slice(back_window, now)
            .into_iter()
            .filter(|p| p.ts >= past_start)
            .map(|p| {
                let mut values = cfg.state_values(&p.state);
                values.insert("power_kw".into(), p.power_kw);
                AssetTimelinePoint { ts: p.ts, values }
            })
            .collect()
    } else {
        vec![]
    };

    // ── Future: from plan slots ────────────────────────────────────────────────

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
                let cost_rate = net_kw.max(0.0) * slot.import_tariff_eur_kwh;
                values.insert("cost_rate_eur_h".into(), cost_rate);
                let co2_rate = net_kw.max(0.0) * slot.co2_g_kwh;
                values.insert("co2_rate_g_h".into(), co2_rate);
                values.insert("import_limit_kw".into(), slot.import_cap_kw);
                values.insert("export_limit_kw".into(), slot.export_cap_kw);
            } else {
                // Physical asset: use its allocation for this slot, or 0 kW if absent.
                // We always emit a point (even 0 kW) so that all assets share the same
                // set of plan-slot timestamps. Omitting zero-allocation slots causes the
                // stacked chart to fall back to an exact-match miss → false zero spikes.
                let power_kw = slot
                    .allocations
                    .iter()
                    .find(|a| a.asset_id == asset_id)
                    .map(|a| a.power_kw)
                    .unwrap_or(0.0);
                values.insert("power_kw".into(), power_kw);
                let cost_rate = power_kw * slot.import_tariff_eur_kwh;
                values.insert("cost_rate_eur_h".into(), cost_rate);
                let co2_rate = power_kw * slot.co2_g_kwh;
                values.insert("co2_rate_g_h".into(), co2_rate);
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
    use crate::assets::{
        AssetConfig, AssetHistoryBuffer, AssetState, BaseLoad, BaseLoadState, EvCharger, EvState,
        HistoryPoint,
    };
    use crate::entities::asset::PlanTrigger;
    use crate::entities::plan::{
        FirmSummary, FlexibleSummary, PacketAllocation, Plan, PlanTimeSlot, PlanningHorizon,
        SlotType,
    };
    use crate::assets::Grid;
    use crate::simulator::energy::EnergyCounter;
    use crate::simulator::{AssetEntry, GridMeter, SimState};
    use chrono::Utc;
    use uuid::Uuid;

    fn ts(offset_s: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(1_700_000_000 + offset_s, 0).unwrap()
    }

    fn make_known(ids: &[&str]) -> HashSet<String> {
        ids.iter().map(|s| s.to_string()).collect()
    }

    /// Create a BaseLoad AssetEntry + AssetConfig with history rows `(offset_s, power_kw)`.
    fn make_base_entry(id: &str, rows: &[(i64, f64)]) -> (AssetEntry, AssetConfig) {
        let cfg = AssetConfig::BaseLoad(BaseLoad { baseline_kw: 0.0, baseline_kw_profile: 0.0 });
        let mut entry = AssetEntry {
            id: id.to_string(),
            state: AssetState::BaseLoad(BaseLoadState {
                actual_power_kw: 0.0,
            }),
            setpoint_kw: 0.0,
            last_power_kw: 0.0,
            energy: EnergyCounter {
                import_kwh: 0.0,
                export_kwh: 0.0,
            },
            history: AssetHistoryBuffer::new(3600),
        };
        for (offset, power) in rows {
            entry.history.push(HistoryPoint {
                ts: ts(*offset),
                power_kw: *power,
                state: AssetState::BaseLoad(BaseLoadState {
                    actual_power_kw: *power,
                }),
            });
        }
        (entry, cfg)
    }

    /// Create an EV AssetEntry + AssetConfig with history rows `(offset_s, power_kw, soc_pct)`.
    fn make_ev_entry(id: &str, rows: &[(i64, f64, f64)]) -> (AssetEntry, AssetConfig) {
        let cfg = AssetConfig::Ev(EvCharger {
            max_charge_kw: 11.0,
            max_discharge_kw: 0.0,
            battery_kwh: 60.0,
            soc_target: 0.8,
            default_charge_kw: 7.4,
            min_soc: 0.1,
        });
        let mut entry = AssetEntry {
            id: id.to_string(),
            state: AssetState::Ev(EvState {
                soc_pct: 0.5,
                plugged: true,
                actual_power_kw: 0.0,
            }),
            setpoint_kw: 0.0,
            last_power_kw: 0.0,
            energy: EnergyCounter {
                import_kwh: 0.0,
                export_kwh: 0.0,
            },
            history: AssetHistoryBuffer::new(3600),
        };
        for (offset, power, soc) in rows {
            entry.history.push(HistoryPoint {
                ts: ts(*offset),
                power_kw: *power,
                state: AssetState::Ev(EvState {
                    soc_pct: *soc,
                    plugged: true,
                    actual_power_kw: *power,
                }),
            });
        }
        (entry, cfg)
    }

    /// Build a SimState from (AssetEntry, AssetConfig) pairs.
    fn make_sim(entries: Vec<(AssetEntry, AssetConfig)>) -> SimState {
        let (assets, configs): (Vec<_>, Vec<_>) = entries.into_iter().unzip();
        SimState {
            asset_configs: configs,
            assets,
            grid: GridMeter::default(),
            grid_asset: Grid::new(),
            pv_smoothing: crate::simulator::PvSmoothingState::default(),
            last_tick: DateTime::from_timestamp(0, 0).unwrap(),
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
                near_horizon: now + Duration::hours(1),
                far_horizon: now + Duration::hours(2),
            },
            firm_boundary: now + Duration::hours(1),
            firm_slots: vec![],
            firm_summary: FirmSummary::default(),
            flexible_slots: vec![],
            envelopes: vec![],
            flexible_summary: FlexibleSummary::default(),
            packets: vec![],
            warnings: vec![],
            steps: vec![],
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
            slot_type: SlotType::Firm,
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
                vec![PacketAllocation {
                    packet_id: Uuid::new_v4(),
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
        }
    }

    #[test]
    fn unknown_asset_returns_none() {
        let known = make_known(&["ev"]);
        let sim = make_sim(vec![]);
        let now = Utc::now();
        let result = build_asset_timeline(
            "xyz",
            &known,
            &sim,
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
        let sim = make_sim(vec![make_base_entry(
            "ev",
            &[(0, 1.0), (50, 2.0), (100, 3.0)],
        )]);
        let result = build_asset_timeline(
            "ev",
            &known,
            &sim,
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
        let sim = make_sim(vec![]);
        let mut plan = empty_plan(now);
        // Slot starting 60s from now with EV allocation
        plan.firm_slots.push(make_slot(60, "ev", 3.5, now));
        let result = build_asset_timeline(
            "ev",
            &known,
            &sim,
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
        assert!(p.values.contains_key("cost_rate_eur_h"));
        assert!(p.values.contains_key("co2_rate_g_h"));
    }

    #[test]
    fn merged_window_contains_both_past_and_future() {
        let now = ts(3600); // 1 hour into epoch
        let known = make_known(&["ev"]);
        // 2 past rows within 1h back
        let sim = make_sim(vec![make_base_entry("ev", &[(0, 1.0), (1800, 2.0)])]);
        let mut plan = empty_plan(now);
        plan.firm_slots.push(make_slot(60, "ev", 3.0, now));
        let result = build_asset_timeline(
            "ev",
            &known,
            &sim,
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
        let sim = make_sim(vec![]);
        let mut plan = empty_plan(now);
        // Grid slot: net_import_kw = 2.0
        let mut slot = make_slot(60, "", 0.0, now);
        slot.net_import_kw = 2.0;
        slot.net_export_kw = 0.0;
        slot.import_tariff_eur_kwh = 0.25;
        slot.co2_g_kwh = 350.0;
        plan.firm_slots.push(slot);

        let result = build_asset_timeline(
            "grid",
            &known,
            &sim,
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
    fn slot_without_asset_allocation_emits_zero_kw() {
        let now = ts(0);
        let known = make_known(&["ev", "battery"]);
        let sim = make_sim(vec![]);
        let mut plan = empty_plan(now);
        // Slot only has battery allocation, no EV allocation
        plan.firm_slots.push(make_slot(60, "battery", 2.0, now));
        let result = build_asset_timeline(
            "ev",
            &known,
            &sim,
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
        let sim = make_sim(vec![make_base_entry("ev", &[(1800, 1.0), (3600, 2.0)])]);
        let mut plan = empty_plan(now);
        plan.firm_slots.push(make_slot(300, "ev", 3.0, now));
        plan.firm_slots.push(make_slot(600, "ev", 3.5, now));
        let result = build_asset_timeline(
            "ev",
            &known,
            &sim,
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
    fn build_now_point_uses_last_history_row() {
        let now = ts(100);
        let sim = make_sim(vec![make_ev_entry("ev", &[(50, 1.0, 0.5), (99, 2.0, 0.6)])]);

        let np = build_now_point("ev", now, &sim);
        assert_eq!(np.ts, now);
        assert!((np.values["power_kw"] - 2.0).abs() < 1e-9);
        assert!((np.values["soc"] - 0.6).abs() < 1e-9);
    }

    #[test]
    fn build_now_point_empty_history() {
        let now = ts(100);
        let sim = make_sim(vec![]);
        let np = build_now_point("ev", now, &sim);
        assert_eq!(np.ts, now);
        assert!(np.values.is_empty());
    }
}
