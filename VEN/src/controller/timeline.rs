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

use crate::controller::trace::{AssetHistoryBuffer, AssetTimelinePoint};
use crate::entities::plan::Plan;

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
    history: &HashMap<String, AssetHistoryBuffer>,
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

    // ── Past: from history buffer ──────────────────────────────────────────────

    let mut points: Vec<AssetTimelinePoint> = history
        .get(asset_id)
        .map(|buf| {
            buf.to_timeline(Some((past_start, now)))
                .into_iter()
                .filter(|p| {
                    // Exclude rows that are entirely NaN (no data recorded yet).
                    p.values.values().any(|v| !v.is_nan())
                })
                .collect()
        })
        .unwrap_or_default();

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
                let cost_rate = net_kw.max(0.0) * slot.import_price_eur_kwh;
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
                let cost_rate = power_kw * slot.import_price_eur_kwh;
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
    use crate::controller::trace::AssetHistoryBuffer;
    use crate::entities::plan::{
        PacketAllocation, Plan, PlanTimeSlot, PlanningHorizon, SlotType, FirmSummary,
        FlexibleSummary,
    };
    use crate::entities::asset::PlanTrigger;
    use chrono::Utc;
    use uuid::Uuid;

    fn ts(offset_s: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(1_700_000_000 + offset_s, 0).unwrap()
    }

    fn make_known(ids: &[&str]) -> HashSet<String> {
        ids.iter().map(|s| s.to_string()).collect()
    }

    fn make_history(asset_id: &str, rows: &[(i64, f64)]) -> HashMap<String, AssetHistoryBuffer> {
        let mut hist = HashMap::new();
        let mut buf = AssetHistoryBuffer::new(3600);
        for (offset, power) in rows {
            buf.push(
                ts(*offset),
                [("power_kw".to_string(), *power)].into(),
            );
        }
        hist.insert(asset_id.to_string(), buf);
        hist
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
        }
    }

    fn make_slot(start_offset_s: i64, asset_id: &str, power_kw: f64, now: DateTime<Utc>) -> PlanTimeSlot {
        let start = now + Duration::seconds(start_offset_s);
        PlanTimeSlot {
            slot_index: 0,
            start,
            end: start + Duration::seconds(300),
            slot_type: SlotType::Firm,
            import_price_eur_kwh: 0.20,
            export_price_eur_kwh: 0.05,
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
        let history = HashMap::new();
        let now = Utc::now();
        let result = build_asset_timeline(
            "xyz",
            &known,
            &history,
            None,
            now,
            TimeWindow { hours_back: 1.0, hours_forward: 1.0 },
        );
        assert!(result.is_none());
    }

    #[test]
    fn past_only_window_returns_history_points() {
        let now = ts(100);
        let known = make_known(&["ev"]);
        // 3 rows: ts(0), ts(50), ts(100) — all within 1h back from ts(100)
        let history = make_history("ev", &[(0, 1.0), (50, 2.0), (100, 3.0)]);
        let result = build_asset_timeline(
            "ev",
            &known,
            &history,
            None,
            now,
            TimeWindow { hours_back: 1.0, hours_forward: 0.0 },
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
        let history = HashMap::new();
        let mut plan = empty_plan(now);
        // Slot starting 60s from now with EV allocation
        plan.firm_slots.push(make_slot(60, "ev", 3.5, now));
        let result = build_asset_timeline(
            "ev",
            &known,
            &history,
            Some(&plan),
            now,
            TimeWindow { hours_back: 0.0, hours_forward: 1.0 },
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
        let history = make_history("ev", &[(0, 1.0), (1800, 2.0)]);
        let mut plan = empty_plan(now);
        plan.firm_slots.push(make_slot(60, "ev", 3.0, now));
        let result = build_asset_timeline(
            "ev",
            &known,
            &history,
            Some(&plan),
            now,
            TimeWindow { hours_back: 1.0, hours_forward: 1.0 },
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
        let history = HashMap::new();
        let mut plan = empty_plan(now);
        // Grid slot: net_import_kw = 2.0
        let mut slot = make_slot(60, "", 0.0, now);
        slot.net_import_kw = 2.0;
        slot.net_export_kw = 0.0;
        slot.import_price_eur_kwh = 0.25;
        slot.co2_g_kwh = 350.0;
        plan.firm_slots.push(slot);

        let result = build_asset_timeline(
            "grid",
            &known,
            &history,
            Some(&plan),
            now,
            TimeWindow { hours_back: 0.0, hours_forward: 1.0 },
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
        let history = HashMap::new();
        let mut plan = empty_plan(now);
        // Slot only has battery allocation, no EV allocation
        plan.firm_slots.push(make_slot(60, "battery", 2.0, now));
        let result = build_asset_timeline(
            "ev",
            &known,
            &history,
            Some(&plan),
            now,
            TimeWindow { hours_back: 0.0, hours_forward: 1.0 },
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
        let history = make_history("ev", &[(1800, 1.0), (3600, 2.0)]);
        let mut plan = empty_plan(now);
        plan.firm_slots.push(make_slot(300, "ev", 3.0, now));
        plan.firm_slots.push(make_slot(600, "ev", 3.5, now));
        let result = build_asset_timeline(
            "ev",
            &known,
            &history,
            Some(&plan),
            now,
            TimeWindow { hours_back: 1.0, hours_forward: 1.0 },
        )
        .unwrap();
        for w in result.windows(2) {
            assert!(w[0].ts <= w[1].ts, "Result is not sorted: {:?} > {:?}", w[0].ts, w[1].ts);
        }
    }
}
