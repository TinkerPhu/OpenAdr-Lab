//! Pure translation from the loaded `Profile` into the domain-layer param
//! structs (`SimulatorParams`, `PlannerParams`, `Vec<AssetParams>`) consumed
//! by `main.rs` at startup. Split out of `main.rs` (which stays orchestration
//! only) to keep that file under the `VEN/src/` file-size cap.

use crate::entities::asset_params::AssetParams;
use crate::entities::planner_params::{PlannerParams, SimulatorParams};
use crate::profile::Profile;

pub fn build_domain_params(
    profile: &Profile,
) -> (SimulatorParams, PlannerParams, Vec<AssetParams>) {
    let sim_params = SimulatorParams {
        tick_s: profile.simulator.tick_s,
        persist_every_s: profile.simulator.persist_every_s,
        report_interval_s: profile.simulator.report_interval_s,
    };
    let planner_params = PlannerParams {
        plan_step_s: profile.planner.effective_step_s(),
        plan_horizon_h: profile.planner.effective_horizon_h(),
        replan_interval_s: profile.planner.replan_interval_s,
        w_energy: profile.planner.w_energy,
        w_ghg: profile.planner.w_ghg,
        w_grid: profile.planner.w_grid,
        c_bat_wear_eur_kwh: profile.planner.c_bat_wear_eur_kwh,
        c_ev_startup_eur: profile.planner.c_ev_startup_eur,
        c_bat_startup_eur: profile.planner.c_bat_startup_eur,
        c_ev_ramp_eur_kw: profile.planner.c_ev_ramp_eur_kw,
        c_bat_ramp_eur_kw: profile.planner.c_bat_ramp_eur_kw,
        c_bat_ev_coexist_eur_kwh: profile.planner.c_bat_ev_coexist_eur_kwh,
        w_viol: profile.planner.w_viol,
        pen_imp_eur_kwh: profile.planner.pen_imp_eur_kwh,
        pen_exp_eur_kwh: profile.planner.pen_exp_eur_kwh,
        v_ev_extra_eur_kwh: profile.planner.v_ev_extra_eur_kwh,
        v_ev_core_eur_kwh: profile.planner.v_ev_core_eur_kwh,
        w_tier_penalty_eur: profile.planner.w_tier_penalty_eur,
        c_ctrl_imp_malus_eur_kwh: profile.planner.c_ctrl_imp_malus_eur_kwh,
        objective: profile.planner.objective,
        plan_adoption_threshold_eur: profile.planner.plan_adoption_threshold_eur,
        plan_adoption_decay_s: profile.planner.plan_adoption_decay_s,
        phase2_epsilon_eur: profile.planner.phase2_epsilon_eur,
        solver_timeout_s: profile.planner.solver_timeout_s,
        mip_gap_target: profile.planner.mip_gap_target,
        planning_initial_delay_s: profile.planner.planning_initial_delay_s,
        gate_switch_penalty_eur: profile.planner.gate_switch_penalty_eur,
        simple_level1_import_cap_pct: profile.planner.simple_level1_import_cap_pct,
        asap_lateness_eur_kwh_h: profile.planner.asap_lateness_eur_kwh_h,
        v_ev_free_charge_eur_kwh: profile.planner.v_ev_free_charge_eur_kwh,
        stale_rate_policy: profile.planner.stale_rate_policy.clone(),
        stale_rate_safe_pctl: profile.planner.stale_rate_safe_pctl,
        penalty_rules: profile.planner.penalty_rules.clone(),
        plan_zones: profile.planner.plan_zones.clone().unwrap_or_else(|| {
            let step_s = profile.planner.effective_step_s();
            let total_s = profile.planner.effective_horizon_h() * 3600;
            let slots = (total_s / step_s) as usize;
            vec![crate::entities::plan::PlanZone { step_s, slots }]
        }),
    };
    let asset_params = profile.asset_params();
    (sim_params, planner_params, asset_params)
}
