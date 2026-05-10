#![allow(dead_code)] // infrastructure types used by milp_planner submodules via use super::*
//! Cross-asset MILP interaction infrastructure.
//!
//! Defines [`AssetInteraction`], a trait that encapsulates LP variables,
//! constraints, and objective terms that span multiple assets. Currently
//! implements [`BatEvCoexistInteraction`] (McCormick envelope linearisation).
//!
//! Also defines the shared LP variable pool types ([`MilpVarPool`],
//! [`GlobalMilpInputs`]) used by Step 6 of the asset-plugin refactor.

use good_lp::{constraint, variable, Constraint, Expression, ProblemVariables, Variable};

use crate::controller::milp_planner::asset_port::{BatteryMilpVars, EvMilpVars, HeaterMilpVars};

// ── Grid-level MILP inputs ───────────────────────────────────────────────────

/// Grid-level (non-asset) MILP parameters for one planning cycle.
/// Per-step `Vec<f64>` fields have `len == n`.
/// Built by `build_global_inputs()` in `milp_planner.rs` (Step 6 refactor).
#[derive(Debug, Clone)]
pub struct GlobalMilpInputs {
    pub n: usize,
    pub dt_h: f64,
    /// Import tariff [€/kWh]
    pub c_imp_eur_kwh: Vec<f64>,
    /// Export tariff [€/kWh]
    pub c_exp_eur_kwh: Vec<f64>,
    /// Grid CO₂ intensity [kgCO₂/kWh]
    pub g_imp_kgco2_kwh: Vec<f64>,
    /// PV generation forecast [kW]
    pub p_pv_kw: Vec<f64>,
    /// Non-controllable baseline load [kW]
    pub p_base_kw: Vec<f64>,
    /// Physical import limit [kW]
    pub p_imp_max_phys_kw: Vec<f64>,
    /// Physical export limit [kW]
    pub p_exp_max_phys_kw: Vec<f64>,
    /// Contractual import limit [kW]
    pub p_imp_max_cont_kw: Vec<f64>,
    /// Contractual export limit [kW]
    pub p_exp_max_cont_kw: Vec<f64>,
    pub pen_imp_eur_kwh: f64,
    pub pen_exp_eur_kwh: f64,
}

// ── Typed LP variable pool ───────────────────────────────────────────────────

/// Grid LP variables (import, export, mutual-exclusion binary, slack).
#[derive(Debug, Clone)]
pub struct GridMilpVars {
    pub p_imp: Vec<Variable>,
    pub p_exp: Vec<Variable>,
    pub u_grid: Vec<Variable>,
    pub s_imp_viol: Vec<Variable>,
    pub s_exp_viol: Vec<Variable>,
}

/// Per-shiftable-load LP variables and scheduling metadata.
#[derive(Debug, Clone)]
pub struct ShiftableLoadMilpVars {
    pub asset_id: String,
    pub power_kw: f64,
    pub duration_slots: usize,
    pub valid_start_slots: Vec<usize>,
    /// Binary start-slot indicators — one per entry in `valid_start_slots`.
    pub y_shift: Vec<Variable>,
}

/// All LP variable handles for one planning cycle, keyed by asset.
/// `Option::None` means the asset is absent from the profile.
#[derive(Debug)]
pub struct MilpVarPool {
    pub grid: GridMilpVars,
    pub bat: Option<BatteryMilpVars>,
    pub ev: Option<EvMilpVars>,
    pub heater: Option<HeaterMilpVars>,
    pub shiftable: Vec<ShiftableLoadMilpVars>,
}

// ── Cross-asset interaction trait ────────────────────────────────────────────

/// A cross-asset LP interaction: declares auxiliary variables, adds constraints,
/// and contributes objective terms that couple two or more assets.
pub trait AssetInteraction: Send + Sync {
    /// Return `true` when all required assets are present in `pool`.
    fn applicable(&self, pool: &MilpVarPool) -> bool;

    /// Declare any auxiliary LP variables into `vars` and return typed handles.
    fn declare_vars(
        &self,
        pool: &MilpVarPool,
        global: &GlobalMilpInputs,
        vars: &mut ProblemVariables,
    ) -> InteractionVars;

    /// Build all constraints that involve `iv` and the main asset variables in `pool`.
    fn constraints(
        &self,
        pool: &MilpVarPool,
        iv: &InteractionVars,
        global: &GlobalMilpInputs,
    ) -> Vec<Constraint>;

    /// Return the objective contribution from interaction variables.
    /// `dt_h` is the slot duration in hours.
    fn objective(&self, iv: &InteractionVars, dt_h: f64) -> Expression;
}

/// Typed LP variable handles returned by each [`AssetInteraction`].
pub enum InteractionVars {
    /// McCormick auxiliary variables for battery-EV coexistence penalty.
    /// `x_coexist[t]` is `None` for slots without PV surplus ≥ ev_min_kw.
    BatEvCoexist { x_coexist: Vec<Option<Variable>> },
}

// ── Battery-EV coexistence interaction ───────────────────────────────────────

/// Penalises simultaneous battery discharge + EV charging in PV-surplus slots.
///
/// Linearises `p_bat_dis[t] × z_ev_on[t]` via McCormick envelopes for slots
/// where `p_pv[t] − p_base[t] ≥ ev.p_min_kw` (i.e. PV could cover the EV on its own).
pub struct BatEvCoexistInteraction {
    /// Penalty coefficient [€/kWh].
    pub c_eur_kwh: f64,
}

impl AssetInteraction for BatEvCoexistInteraction {
    fn applicable(&self, pool: &MilpVarPool) -> bool {
        pool.bat.is_some() && pool.ev.is_some()
    }

    fn declare_vars(
        &self,
        pool: &MilpVarPool,
        global: &GlobalMilpInputs,
        vars: &mut ProblemVariables,
    ) -> InteractionVars {
        let bat = pool.bat.as_ref().unwrap();
        let ev = pool.ev.as_ref().unwrap();
        let x_coexist = (0..global.n)
            .map(|t| {
                let surplus = global.p_pv_kw[t] - global.p_base_kw[t];
                if surplus >= ev.p_min_kw {
                    Some(vars.add(variable().min(0.0).max(bat.dis_max_kw)))
                } else {
                    None
                }
            })
            .collect();
        InteractionVars::BatEvCoexist { x_coexist }
    }

    fn constraints(
        &self,
        pool: &MilpVarPool,
        iv: &InteractionVars,
        global: &GlobalMilpInputs,
    ) -> Vec<Constraint> {
        let InteractionVars::BatEvCoexist { x_coexist } = iv;
        let bat = pool.bat.as_ref().unwrap();
        let ev = pool.ev.as_ref().unwrap();
        let dis_max = bat.dis_max_kw;
        let mut cs = Vec::new();
        for t in 0..global.n {
            if let Some(x) = x_coexist[t] {
                // McCormick envelope for x = p_bat_dis[t] × z_ev_on[t]
                cs.push(constraint!(x <= bat.p_dis[t]));
                cs.push(constraint!(x <= dis_max * ev.z_ev_on[t]));
                cs.push(constraint!(
                    x >= bat.p_dis[t] - dis_max * (1.0 - ev.z_ev_on[t])
                ));
            }
        }
        cs
    }

    fn objective(&self, iv: &InteractionVars, dt_h: f64) -> Expression {
        let InteractionVars::BatEvCoexist { x_coexist } = iv;
        let mut obj = Expression::from(0.0);
        for x_opt in x_coexist {
            if let Some(x) = x_opt {
                obj += (self.c_eur_kwh * dt_h) * *x;
            }
        }
        obj
    }
}

// ── Factory ──────────────────────────────────────────────────────────────────

/// Build the list of active cross-asset interactions for one planning cycle.
/// An interaction is only included when its penalty coefficient is non-zero.
pub fn build_interactions(c_bat_ev_coexist_eur_kwh: f64) -> Vec<Box<dyn AssetInteraction>> {
    let mut v: Vec<Box<dyn AssetInteraction>> = Vec::new();
    if c_bat_ev_coexist_eur_kwh > 0.0 {
        v.push(Box::new(BatEvCoexistInteraction {
            c_eur_kwh: c_bat_ev_coexist_eur_kwh,
        }));
    }
    v
}
