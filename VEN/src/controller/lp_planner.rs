//! LP-based battery charge pre-planner — Phase 1.
//!
//! Replaces the two-pass greedy `plan_battery_grid_charges` with a linear
//! program that finds the globally-optimal charge schedule over the full
//! planning horizon.  Only battery charge/discharge decisions are LP
//! variables; EV and heater are treated as fixed loads baked into
//! `slot.baseline_kw`.
//!
//! Returns the same `HashMap<usize, f64>` contract as the greedy pre-planner,
//! consumed unchanged by Rule 9 in `planner::rules_choose`.
//!
//! # LP formulation (Phase 1)
//!
//! **Decision variables per slot t:**
//! - `p_bat_c[t]` ∈ [0, max_charge_kw]  (firm slots only; 0 in flexible slots)
//! - `p_bat_d[t]` ∈ [0, max_discharge_kw]  (all slots — enables horizon foresight)
//! - `g_import[t]` ∈ [0, import_cap_kw]   (auxiliary grid import)
//! - `g_export[t]` ∈ [0, export_cap_kw]   (auxiliary grid export)
//! - `soc[t]`      ∈ [min_soc, 1.0]        (state variable, n values for n slots)
//!
//! **Constraints:**
//! - Power balance:   `g_import[t] − g_export[t] = baseline_kw[t] + p_bat_c[t] − p_bat_d[t]`
//! - SoC continuity:  `soc[t] = soc[t−1] + p_bat_c[t]·η_c·dt/cap − p_bat_d[t]·dt/(η_d·cap)`
//!   with `soc[0]` expressed using `initial_soc` as a known constant.
//!
//! **Objective:** minimise `Σ_t [g_import[t]·tariff_import[t] − g_export[t]·tariff_export[t]]·dt`

use std::collections::HashMap;

use good_lp::{constraint, default_solver, variable, Expression, Solution, SolverModel};
use tracing::warn;

use crate::entities::plan::PlanTimeSlot;
use crate::profile::BatteryConfig;

/// LP-based battery charge pre-planner.
///
/// `all_slots` covers the full planning horizon (firm + flexible).
/// Only `firm_count` leading slots are eligible for scheduled charges.
///
/// On solver failure returns an empty map (safe — Rule 9 simply won't fire
/// and the rules engine falls back to its own discharge-only behaviour).
pub fn plan_battery_lp(
    bat: &BatteryConfig,
    initial_soc: f64,
    all_slots: &[&PlanTimeSlot],
    firm_count: usize,
    slot_h: f64,
) -> HashMap<usize, f64> {
    let n = all_slots.len();
    if n == 0 || firm_count == 0 {
        return HashMap::new();
    }

    // η_c = η_d = √(round_trip_efficiency).  Charging multiplies stored energy
    // by η_c; discharging divides delivered energy by η_d.
    let eta = bat.round_trip_efficiency.sqrt();
    let charge_factor = eta * slot_h / bat.capacity_kwh;
    let discharge_factor = slot_h / (eta * bat.capacity_kwh);

    // Clamp to valid SoC range so the initial-state constraint is always feasible.
    let soc0 = initial_soc.clamp(bat.min_soc, 1.0);

    // Compute median import tariff across all slots — same reference as the
    // greedy two-pass algorithm and Rule 10.
    let mut sorted_tariffs: Vec<f64> = all_slots
        .iter()
        .map(|s| s.import_tariff_eur_kwh)
        .collect();
    sorted_tariffs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let median_tariff = if sorted_tariffs.is_empty() {
        0.20
    } else {
        sorted_tariffs[sorted_tariffs.len() / 2]
    };
    // Only allow discharge in slots where the tariff is genuinely expensive
    // (same threshold as Rule 10: tariff > median / √rte).  This prevents the
    // LP from doing speculative arbitrage between "cheap" and "normal" slots
    // when the two-pass algorithm would predict no depletion and stay idle.
    let expensive_threshold = median_tariff / eta;

    // ── Variable declarations ─────────────────────────────────────────────────
    let mut vars = good_lp::variables!();

    // Battery charge: allowed only in firm slots.
    let p_bat_c: Vec<_> = (0..n)
        .map(|t| {
            let max = if t < firm_count { bat.max_charge_kw } else { 0.0 };
            vars.add(variable().min(0.0).max(max))
        })
        .collect();

    // Discharge: only allowed in slots where tariff exceeds the expensive
    // threshold (mirrors Rule 10's firing condition).  Without this gate, the
    // LP would charge speculatively from cheap slots to offset "normal" tariff
    // slots, even though the greedy algorithm stays idle when no depletion is
    // predicted.
    let p_bat_d: Vec<_> = (0..n)
        .map(|t| {
            let max_d = if all_slots[t].import_tariff_eur_kwh > expensive_threshold {
                bat.max_discharge_kw
            } else {
                0.0
            };
            vars.add(variable().min(0.0).max(max_d))
        })
        .collect();

    // Auxiliary grid variables with conservative upper bounds.
    // Use max(cap, baseline + asset_power) to guarantee the power-balance
    // constraint is always feasible even when baseline alone exceeds the cap.
    let g_import: Vec<_> = (0..n)
        .map(|t| {
            let slot = all_slots[t];
            let min_needed = slot.baseline_kw.max(0.0) + bat.max_charge_kw;
            let ub = slot.import_cap_kw.max(min_needed);
            vars.add(variable().min(0.0).max(ub))
        })
        .collect();

    let g_export: Vec<_> = (0..n)
        .map(|t| {
            let slot = all_slots[t];
            let min_needed = (-slot.baseline_kw).max(0.0) + bat.max_discharge_kw;
            // Guard against f64::MAX / very large sentinel values.
            let cap = slot.export_cap_kw.min(1_000.0);
            let ub = cap.max(min_needed);
            vars.add(variable().min(0.0).max(ub))
        })
        .collect();

    // SoC at *end* of slot t (= start of slot t+1).  n variables for n slots.
    // The initial SoC soc0 is a known constant, not a variable.
    let soc: Vec<_> = (0..n)
        .map(|_| vars.add(variable().min(bat.min_soc).max(1.0)))
        .collect();

    // ── Objective: minimise net grid cost ────────────────────────────────────
    let objective: Expression = (0..n)
        .map(|t| {
            let slot = all_slots[t];
            g_import[t] * (slot.import_tariff_eur_kwh * slot_h)
                - g_export[t] * (slot.export_tariff_eur_kwh * slot_h)
        })
        .fold(Expression::from(0.0), |acc, e| acc + e);

    let mut model = vars.minimise(objective).using(default_solver);

    // ── Constraints ───────────────────────────────────────────────────────────
    for t in 0..n {
        let slot = all_slots[t];

        // Power balance (one equality per slot).
        model = model.with(constraint!(
            g_import[t] - g_export[t] - p_bat_c[t] + p_bat_d[t]
                == slot.baseline_kw
        ));

        // SoC continuity.  For t=0 the previous state is soc0 (constant).
        if t == 0 {
            model = model.with(constraint!(
                soc[0] - p_bat_c[0] * charge_factor + p_bat_d[0] * discharge_factor
                    == soc0
            ));
        } else {
            model = model.with(constraint!(
                soc[t] - soc[t - 1] - p_bat_c[t] * charge_factor
                    + p_bat_d[t] * discharge_factor
                    == 0.0
            ));
        }
    }

    // ── Solve ─────────────────────────────────────────────────────────────────
    match model.solve() {
        Ok(sol) => {
            let mut charge_plan = HashMap::new();
            for t in 0..firm_count {
                let kw = sol.value(p_bat_c[t]);
                if kw > 0.01 {
                    charge_plan.insert(t, kw);
                }
            }
            charge_plan
        }
        Err(e) => {
            warn!("LP battery planner failed ({e:?}); returning empty charge plan");
            HashMap::new()
        }
    }
}

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use crate::entities::plan::{PlanTimeSlot, SlotType};

    fn make_slot(import_tariff_eur_kwh: f64, baseline_kw: f64) -> PlanTimeSlot {
        let now = Utc::now();
        PlanTimeSlot {
            slot_index: 0,
            start: now,
            end: now,
            slot_type: SlotType::Firm,
            import_tariff_eur_kwh,
            export_tariff_eur_kwh: 0.05,
            co2_g_kwh: 200.0,
            grid_effective_cost: import_tariff_eur_kwh,
            rate_estimated: false,
            import_cap_kw: 20.0,
            export_cap_kw: 10.0,
            baseline_kw,
            pv_forecast_kw: 0.0,
            surplus_available_kw: (-baseline_kw).max(0.0),
            allocations: vec![],
            net_import_kw: 0.0,
            net_export_kw: 0.0,
            import_flexibility_kw: 0.0,
            export_flexibility_kw: 0.0,
        }
    }

    fn bat() -> BatteryConfig {
        BatteryConfig {
            id: "battery".into(),
            capacity_kwh: 10.0,
            max_charge_kw: 5.0,
            max_discharge_kw: 5.0,
            initial_soc: 0.5,
            round_trip_efficiency: 0.92,
            min_soc: 0.10,
        }
    }

    #[test]
    fn charges_at_cheap_slots_before_expensive_period() {
        // 4 cheap (0.05) + 8 expensive (0.40) + 16 neutral (0.20) slots.
        // Median = 0.20; threshold = 0.20/√0.92 ≈ 0.208.  The 0.40 slots
        // exceed the threshold, making it worthwhile to pre-charge at 0.05.
        let mut slots: Vec<PlanTimeSlot> = (0..4)
            .map(|_| make_slot(0.05, 1.0))
            .chain((0..8).map(|_| make_slot(0.40, 1.0)))
            .chain((0..16).map(|_| make_slot(0.20, 1.0)))
            .collect();
        for (i, s) in slots.iter_mut().enumerate() {
            s.slot_index = i;
        }

        let all_refs: Vec<&PlanTimeSlot> = slots.iter().collect();
        // initial_soc = min_soc: battery cannot discharge at all without first
        // pre-charging.  This forces the LP to use the cheap slots.
        let plan = plan_battery_lp(&bat(), 0.10, &all_refs, 28, 5.0 / 60.0);

        let cheap_charge_kw: f64 = (0..4).filter_map(|i| plan.get(&i)).sum();
        assert!(
            cheap_charge_kw > 0.01,
            "LP should charge during cheap slots; scheduled {cheap_charge_kw:.3} kW"
        );
    }

    #[test]
    fn idle_when_uniform_cheap_tariff_no_future_discharge() {
        // Uniform cheap tariff — no expensive period to arbitrage against.
        // No discharge load at all (baseline = 0.0, SoC already high).
        let mut slots: Vec<PlanTimeSlot> = (0..12).map(|_| make_slot(0.05, 0.0)).collect();
        for (i, s) in slots.iter_mut().enumerate() {
            s.slot_index = i;
        }

        let all_refs: Vec<&PlanTimeSlot> = slots.iter().collect();
        let plan = plan_battery_lp(&bat(), 0.8, &all_refs, 12, 5.0 / 60.0);

        let total_kw: f64 = plan.values().sum();
        assert!(
            total_kw < 0.01,
            "LP should not charge when there is no arbitrage benefit; scheduled {total_kw:.3} kW"
        );
    }
}
