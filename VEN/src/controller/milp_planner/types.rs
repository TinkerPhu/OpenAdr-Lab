use crate::entities::planner_params::{PenaltyRuleParams, PlannerObjective, PlannerParams};

// ── Internal MILP types ──────────────────────────────────────────────────────

/// Map `good_lp`'s coarse solve-quality classification to our persisted
/// `SolveStatus` (GB-31). `good_lp::solvers::SolutionStatus` has no
/// `Infeasible`/`Unbounded` variants of its own — those solves return
/// `Err(ResolutionError)` before a `Solution` (and so a `SolutionStatus`)
/// ever exists, and are handled at the `fallback_plan` call site instead.
pub(crate) fn map_solve_status(
    status: good_lp::solvers::SolutionStatus,
) -> crate::entities::plan::SolveStatus {
    use crate::entities::plan::SolveStatus;
    match status {
        good_lp::solvers::SolutionStatus::Optimal => SolveStatus::Optimal,
        good_lp::solvers::SolutionStatus::TimeLimit => SolveStatus::TimeLimit,
        good_lp::solvers::SolutionStatus::GapLimit => SolveStatus::GapLimit,
    }
}

/// WP6.3 (BL-09) — the penalty rules active for this plan, for UI consumption. Kept here
/// (rather than `results.rs`, its only caller) to stay under that file's 500-production-line
/// cap; `results.rs` pulls it in via `use super::types::*`.
pub(crate) fn active_penalty_rules(
    inputs_penalty_rules: &[PenaltyRuleParams],
) -> Vec<crate::entities::plan::ActivePenaltyRule> {
    inputs_penalty_rules
        .iter()
        .map(|r| crate::entities::plan::ActivePenaltyRule {
            rule_id: r.rule_id.clone(),
            threshold_kw: r.threshold_kw,
        })
        .collect()
}

/// Phase 1 objective coefficients (economic cost). Derived from PlannerConfig / PlannerObjective.
#[derive(Debug, Clone)]
pub(crate) struct Phase1Weights {
    /// Scales C_energy = Σ(c_imp·p_imp − c_exp·p_exp)·Δt
    pub(crate) w_energy: f64,
    /// Monetises GHG emissions [€/kgCO₂]
    pub(crate) w_ghg: f64,
    /// Penalty per kWh of total grid exchange (import + export)
    pub(crate) w_grid: f64,
    /// Penalty per kWh of grid import only (autarky objective)
    pub(crate) w_import: f64,
    /// Scales contractual violation penalties
    pub(crate) w_viol: f64,
    /// Wear per kWh charged or discharged by a storage asset [€/kWh]: the profile's
    /// `c_bat_wear_eur_kwh` under the objectives that price money (MinCost, MaxRevenue, Custom),
    /// 0 under those that do not (see `build_phase1_weights`).
    pub(crate) c_storage_wear_eur_kwh: f64,
    /// Penalty per kWh of battery discharge co-occurring with EV charging when
    /// PV surplus ≥ ev_min_kw [€/kWh]. 0.0 = disabled.
    pub(crate) c_bat_ev_coexist_eur_kwh: f64,
    /// Penalty [€/kWh] on controllable-asset import exceeding free PV surplus.
    /// Covers all assets as a group. 0.0 = disabled.
    pub(crate) c_ctrl_imp_malus_eur_kwh: f64,
}

/// Fully-resolved MILP input parameters for one planning cycle.
/// Created by `build_milp_inputs()`; consumed by `solve_milp_two_phase()` (Phase 3).
/// All per-step `Vec<f64>` fields have length `n`.
#[derive(Debug, Clone)]
pub(crate) struct MilpInputs {
    /// Number of planning steps.
    pub(crate) n: usize,
    /// Per-slot step sizes in hours (len = n). Uniform today; 3-tier variable-step when zones are active.
    pub(crate) dt_h: Vec<f64>,
    /// Cumulative slot boundary offsets in whole seconds from `now` (len = n + 1).
    /// Exact integer arithmetic — must be used instead of re-deriving from `dt_h` via
    /// float seconds, which can truncate a second short for step sizes that don't
    /// round-trip exactly through `step_s as f64 / 3600.0`.
    pub(crate) cum_s: Vec<i64>,

    // ── Grid (per-step arrays, len = n) ──────────────────────────────────────
    /// Import tariff [€/kWh]
    pub(crate) c_imp_eur_kwh: Vec<f64>,
    /// WP4.4 (BL-07): true where `c_imp_eur_kwh[t]` was filled by the
    /// StaleRatePolicy (slot lies beyond tariff coverage).
    pub(crate) rate_stale: Vec<bool>,
    /// WP4.4: stable warning text when any slot is stale (→ PlanWarning →
    /// WP4.3 notification), `None` when the horizon is fully covered.
    pub(crate) stale_rate_warning: Option<String>,
    /// BL-17 closeout: same as `stale_rate_warning`, but for CO2 intensity
    /// coverage — independent, since CO2 and import-tariff coverage can end
    /// at different times.
    pub(crate) co2_stale_rate_warning: Option<String>,
    /// Export tariff [€/kWh]
    pub(crate) c_exp_eur_kwh: Vec<f64>,
    /// Grid CO₂ intensity [kgCO₂/kWh] (÷1000 from stored g/kWh)
    pub(crate) g_imp_kgco2_kwh: Vec<f64>,
    /// PV generation forecast [kW]
    pub(crate) p_pv_kw: Vec<f64>,
    /// Non-controllable baseline load [kW]
    pub(crate) p_base_kw: Vec<f64>,
    /// Physical import limit at meter/breaker [kW]
    pub(crate) p_imp_max_phys_kw: Vec<f64>,
    /// Physical export limit [kW]
    pub(crate) p_exp_max_phys_kw: Vec<f64>,
    /// Contractual import limit [kW] (OpenADR event limit or physical when no event)
    pub(crate) p_imp_max_cont_kw: Vec<f64>,
    /// Contractual export limit [kW]
    pub(crate) p_exp_max_cont_kw: Vec<f64>,
    /// Per-kWh import violation penalty (scalar, from PlannerConfig)
    pub(crate) pen_imp_eur_kwh: f64,
    /// Per-kWh export violation penalty (scalar)
    pub(crate) pen_exp_eur_kwh: f64,
    /// HiGHS optimality-gap tolerance (scalar, from PlannerConfig). Carried
    /// here rather than as a solve parameter because all three solve sites
    /// already take `&MilpInputs` but none take `&PlannerParams`.
    pub(crate) mip_gap_target: f64,
    /// WP6.3 (BL-09) — active peak-demand penalty rules. Empty = feature disabled.
    pub(crate) penalty_rules: Vec<PenaltyRuleParams>,
}

/// Internal MILP descriptor for one shiftable load block. Implements
/// `AssetMilpContext` (impl in `assets/shiftable_load.rs`, matching the
/// Battery/EV/Heater cross-file convention) — `shiftable-load-as-asset`,
/// replacing the pre-Spec-B bespoke `solver_phase1.rs`/`solver_phase2.rs`
/// wiring that built this struct directly from `&[ShiftableLoad]`.
#[derive(Debug, Clone)]
pub(crate) struct ShiftableLoadMilpContext {
    /// Label for allocations (e.g. "wm")
    pub(crate) asset_id: String,
    /// Fixed power level while running [kW]
    pub(crate) power_kw: f64,
    /// Duration in planning slots (ceil)
    pub(crate) duration_slots: usize,
    /// Valid start-slot indices within [0, n). `[0]` when the load has
    /// already started — the decision is made, not a MILP choice.
    pub(crate) valid_start_slots: Vec<usize>,
    /// One full run [min], for the energy the plan's session flexibility reports.
    pub(crate) duration_min: u32,
    /// The request's own window, for the plan's session flexibility.
    pub(crate) earliest_start: chrono::DateTime<chrono::Utc>,
    pub(crate) latest_end: chrono::DateTime<chrono::Utc>,
}

// ── Builder functions ────────────────────────────────────────────────────────

/// Build the MILP objective weights from the planner configuration.
/// `objective` overrides `planner.objective`; pass `planner.objective`
/// to use the configured default.
pub(crate) fn build_phase1_weights(
    planner: &PlannerParams,
    objective: PlannerObjective,
) -> Phase1Weights {
    let p = planner;
    match objective {
        PlannerObjective::MinCost => Phase1Weights {
            w_energy: 1.0,
            w_ghg: 0.20,
            w_grid: 0.02,
            w_import: 0.0,
            w_viol: p.w_viol,
            c_storage_wear_eur_kwh: p.c_bat_wear_eur_kwh,
            c_bat_ev_coexist_eur_kwh: p.c_bat_ev_coexist_eur_kwh,
            c_ctrl_imp_malus_eur_kwh: p.c_ctrl_imp_malus_eur_kwh,
        },
        PlannerObjective::MinGhg => Phase1Weights {
            w_energy: 0.0,
            w_ghg: 10.0,
            w_grid: 0.0,
            w_import: 0.0,
            w_viol: p.w_viol,
            c_storage_wear_eur_kwh: 0.0,
            c_bat_ev_coexist_eur_kwh: p.c_bat_ev_coexist_eur_kwh,
            c_ctrl_imp_malus_eur_kwh: p.c_ctrl_imp_malus_eur_kwh,
        },
        PlannerObjective::MinGrid => Phase1Weights {
            w_energy: 0.0,
            w_ghg: 0.0,
            w_grid: 1.0,
            w_import: 0.0,
            w_viol: p.w_viol,
            c_storage_wear_eur_kwh: 0.0,
            c_bat_ev_coexist_eur_kwh: p.c_bat_ev_coexist_eur_kwh,
            c_ctrl_imp_malus_eur_kwh: p.c_ctrl_imp_malus_eur_kwh,
        },
        PlannerObjective::MinImport => Phase1Weights {
            w_energy: 0.0,
            w_ghg: 0.0,
            w_grid: 0.0,
            w_import: 1.0,
            w_viol: p.w_viol,
            c_storage_wear_eur_kwh: 0.0,
            c_bat_ev_coexist_eur_kwh: p.c_bat_ev_coexist_eur_kwh,
            c_ctrl_imp_malus_eur_kwh: p.c_ctrl_imp_malus_eur_kwh,
        },
        PlannerObjective::MaxRevenue => Phase1Weights {
            w_energy: 1.0,
            w_ghg: 0.0,
            w_grid: 0.0,
            w_import: 0.0,
            w_viol: p.w_viol,
            c_storage_wear_eur_kwh: p.c_bat_wear_eur_kwh,
            c_bat_ev_coexist_eur_kwh: p.c_bat_ev_coexist_eur_kwh,
            c_ctrl_imp_malus_eur_kwh: p.c_ctrl_imp_malus_eur_kwh,
        },
        PlannerObjective::Custom => Phase1Weights {
            w_energy: p.w_energy,
            w_ghg: p.w_ghg,
            w_grid: p.w_grid,
            w_import: 0.0,
            w_viol: p.w_viol,
            c_storage_wear_eur_kwh: p.c_bat_wear_eur_kwh,
            c_bat_ev_coexist_eur_kwh: p.c_bat_ev_coexist_eur_kwh,
            c_ctrl_imp_malus_eur_kwh: p.c_ctrl_imp_malus_eur_kwh,
        },
    }
}

/// Output from the MILP solver for one planning cycle: the grid's series, and each asset's
/// part as the asset read it back. All per-slot series have `len == n`.
#[derive(Debug, Clone)]
pub(crate) struct SolveOutput {
    /// GB-31 — the real HiGHS termination reason for this solve, read from
    /// `good_lp`'s `Solution::status()`. Map with `map_solve_status` before
    /// persisting onto `Plan.solve_status`.
    pub(crate) status: good_lp::solvers::SolutionStatus,
    pub(crate) objective_eur: f64,
    /// Grid import power per step [kW]
    pub(crate) p_imp_kw: Vec<f64>,
    /// Grid export power per step [kW]
    pub(crate) p_exp_kw: Vec<f64>,
    /// PV export decision variable solution per step [kW]; `<= p_pv_kw[t]` (§ pv-export-curtailment)
    pub(crate) p_pv_used_kw: Vec<f64>,
    /// Import contractual-limit violation slack [kW]
    pub(crate) s_imp_viol_kw: Vec<f64>,
    /// Export contractual-limit violation slack [kW]
    pub(crate) s_exp_viol_kw: Vec<f64>,
    /// WP6.3 (BL-09) — per-rule, per-window peak-demand penalty slack [kW].
    /// Outer len = `inputs.penalty_rules.len()` (same order), inner len = that
    /// rule's window count. `> 0.0` means that window's peak still exceeded
    /// `threshold_kw` after solving (penalty accepted).
    pub(crate) s_penalty_kw: Vec<Vec<f64>>,
    /// Each asset's part of the solution, in planner input order.
    pub(crate) assets: Vec<super::asset_port::AssetSolution>,
}

impl SolveOutput {
    /// The solution of the asset with this id.
    pub(crate) fn asset(&self, asset_id: &str) -> Option<&super::asset_port::AssetSolution> {
        self.assets.iter().find(|a| a.asset_id == asset_id)
    }
}

#[cfg(test)]
mod map_solve_status_tests {
    use super::map_solve_status;
    use crate::entities::plan::SolveStatus;

    // Direct unit coverage of the mapping rather than an integration test
    // that tries to force each real HiGHS termination reason: reproducing
    // TimeLimit/GapLimit via solver knobs (e.g. a near-zero timeout) is
    // solver-timing-dependent and was observed to be flaky (a 0s timeout on
    // this planner's small MIP came back Infeasible/NoSolutionFound, not
    // TimeLimit, in one run) — this test instead pins down the one thing
    // GB-31 actually changed: the mapping itself.
    #[test]
    fn maps_every_good_lp_solution_status_to_the_matching_solve_status() {
        assert_eq!(
            map_solve_status(good_lp::solvers::SolutionStatus::Optimal),
            SolveStatus::Optimal
        );
        assert_eq!(
            map_solve_status(good_lp::solvers::SolutionStatus::TimeLimit),
            SolveStatus::TimeLimit
        );
        assert_eq!(
            map_solve_status(good_lp::solvers::SolutionStatus::GapLimit),
            SolveStatus::GapLimit
        );
    }
}
