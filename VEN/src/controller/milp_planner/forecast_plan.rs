//! A plan built from the opportunistic forecast instead of a solve (openspec
//! `opportunistic-objective` D6, D8).
//!
//! Builds the same inputs a solve would, hands the slot grid to the forecaster it is given, and
//! turns the trace into a `SolveOutput` whose per-asset parts each asset reads back itself
//! (`AssetMilpContext::solution_from_trace`). From there it is an ordinary plan:
//! `translate_to_plan` with the same contexts, status `Simulated`, no friction. Never solves,
//! and never knows what the forecaster runs.

use crate::controller::forecast_trace::{ForecastGrid, ForecastTrace};
use crate::controller::SolveRequest;
use crate::entities::plan::{Plan, SolveStatus};

use super::asset_port::Participation;
use super::results::translate_to_plan;
use super::types::{MilpInputs, SolveOutput};

/// The plan of what the forecaster says the site will do under the request's objective.
pub fn plan_from_forecast(
    req: SolveRequest,
    forecast: impl FnOnce(&ForecastGrid) -> ForecastTrace,
) -> Plan {
    let objective = req.objective_override.unwrap_or(req.planner.objective);
    let mut asset_contexts = req.asset_contexts;
    let inputs = super::planner_inputs(
        &mut asset_contexts,
        &req.grid,
        &req.planner,
        super::inputs::SiteInputs {
            grid_max_import_kw: req.grid_max_import_kw,
            grid_max_export_kw: req.grid_max_export_kw,
            baseline_override: req.baseline_override.as_ref(),
        },
        &super::inputs::StaleRateRefs {
            import: req.diurnal_import_eur_kwh.as_ref(),
            co2: req.diurnal_co2_g_kwh.as_ref(),
        },
        req.now,
    );
    let trace = forecast(&forecast_grid(&inputs, &asset_contexts, req.now));
    let sol = solve_output(&trace, &inputs, &asset_contexts);
    let marginal_eur_per_kwh = marginal_values_eur_per_kwh(&inputs, &sol);
    let energy_cost_eur = energy_cost_eur(&inputs, &sol);
    let weights = super::build_phase1_weights(&req.planner, objective);
    let mut plan = translate_to_plan(
        &sol,
        &inputs,
        &asset_contexts,
        &weights,
        &req.planner,
        req.now,
        req.trigger,
        objective,
        energy_cost_eur,
        0.0,
        &marginal_eur_per_kwh,
        None,
    );
    plan.solve_status = SolveStatus::Simulated;
    plan
}

/// The planner's slot grid and every forecast participant's series, for the forecaster.
fn forecast_grid(
    inputs: &MilpInputs,
    asset_contexts: &[Box<dyn super::AssetMilpContext>],
    now: chrono::DateTime<chrono::Utc>,
) -> ForecastGrid {
    let n = inputs.n;
    ForecastGrid {
        slot_start: (0..n)
            .map(|t| now + chrono::Duration::seconds(inputs.cum_s[t]))
            .collect(),
        slot_s: (0..n)
            .map(|t| (inputs.cum_s[t + 1] - inputs.cum_s[t]) as f64)
            .collect(),
        forecast_kw_by_asset: asset_contexts
            .iter()
            .filter_map(|c| match c.participation() {
                Participation::Forecast { forecast_kw, .. } => {
                    Some((c.asset_id().to_string(), forecast_kw.to_vec()))
                }
                Participation::Dispatched => None,
            })
            .collect(),
    }
}

/// The trace as a solution: each asset's part read back by the asset, the grid flows from the
/// trace, and the slack those flows would need, so the plan warns about an overrun the
/// forecast foresees exactly as it does for a solved one.
fn solve_output(
    trace: &ForecastTrace,
    inputs: &MilpInputs,
    asset_contexts: &[Box<dyn super::AssetMilpContext>],
) -> SolveOutput {
    let n = inputs.n;
    let idle = vec![0.0; n];
    let slot_series = |series: &[f64]| -> Vec<f64> {
        (0..n)
            .map(|t| series.get(t).copied().unwrap_or(0.0))
            .collect()
    };
    let (p_imp_kw, p_exp_kw) = (slot_series(&trace.import_kw), slot_series(&trace.export_kw));
    let beyond = |flow_kw: &[f64], cap_kw: &[f64]| -> Vec<f64> {
        (0..n).map(|t| (flow_kw[t] - cap_kw[t]).max(0.0)).collect()
    };
    SolveOutput {
        status: good_lp::solvers::SolutionStatus::Optimal,
        objective_eur: 0.0,
        s_imp_viol_kw: beyond(&p_imp_kw, &inputs.p_imp_max_cont_kw),
        s_exp_viol_kw: beyond(&p_exp_kw, &inputs.p_exp_max_cont_kw),
        s_penalty_kw: inputs
            .penalty_rules
            .iter()
            .map(|rule| super::penalty::window_exceedance_kw(&inputs.cum_s, rule, &p_imp_kw))
            .collect(),
        p_imp_kw,
        p_exp_kw,
        assets: asset_contexts
            .iter()
            .map(|c| {
                let id = c.asset_id();
                let net_kw = trace.net_kw_by_asset.get(id).map_or(&idle, |v| v);
                let states = trace.states_by_asset.get(id).map_or(&[][..], |v| v);
                c.solution_from_trace(&slot_series(net_kw), states)
            })
            .collect(),
    }
}

/// D8: a forecast slot's marginal value is the import tariff when the slot imports or is
/// balanced, the export tariff when it exports. Descriptive only: opportunistic dispatch ranks
/// by declaration, not by these.
fn marginal_values_eur_per_kwh(inputs: &MilpInputs, sol: &SolveOutput) -> Vec<f64> {
    (0..inputs.n)
        .map(|t| {
            if sol.p_exp_kw[t] > sol.p_imp_kw[t] {
                inputs.c_exp_eur_kwh[t]
            } else {
                inputs.c_imp_eur_kwh[t]
            }
        })
        .collect()
}

/// What the forecast site pays for energy over the horizon [EUR]: the plan's `objective_eur`.
fn energy_cost_eur(inputs: &MilpInputs, sol: &SolveOutput) -> f64 {
    (0..inputs.n)
        .map(|t| {
            (inputs.c_imp_eur_kwh[t] * sol.p_imp_kw[t] - inputs.c_exp_eur_kwh[t] * sol.p_exp_kw[t])
                * inputs.dt_h[t]
        })
        .sum()
}
