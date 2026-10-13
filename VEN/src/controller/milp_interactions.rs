//! Cross-asset MILP interaction infrastructure.
//!
//! Defines [`AssetInteraction`], a trait that encapsulates LP variables,
//! constraints, and objective terms that span multiple assets. Currently
//! implements [`BatEvCoexistInteraction`] (McCormick envelope linearisation).
//!
//! Also defines the shared LP variable pool ([`MilpVarPool`]) and the grid-level inputs
//! ([`GlobalMilpInputs`]). The pool holds each asset as the [`DeclaredAsset`] it declared, so
//! nothing here knows an asset's kind: an interaction couples assets by the roles they offer
//! (a discharge leg, an on/off indicator), and the site sums their net power.

use good_lp::{constraint, variable, Constraint, Expression, ProblemVariables, Variable};

use crate::controller::asset_milp_port::DeclaredAsset;

// ── Grid-level MILP inputs ───────────────────────────────────────────────────

/// Grid-level (non-asset) MILP parameters for one planning cycle.
/// Per-step `Vec<f64>` fields have `len == n`.
/// Built by `ModelSkeleton::declare` (`milp_planner/model_skeleton.rs`).
#[derive(Debug, Clone)]
pub struct GlobalMilpInputs {
    pub n: usize,
    pub dt_h: Vec<f64>,
    /// PV generation forecast [kW]
    pub p_pv_kw: Vec<f64>,
    /// Non-controllable baseline load [kW]
    pub p_base_kw: Vec<f64>,
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
    /// PV export decision variable: `0 <= p_pv_used[t] <= p_pv_kw[t]`. The solver curtails
    /// below the forecast only when doing so relieves an export-capacity constraint — see
    /// `openspec/changes/pv-export-curtailment/design.md`.
    pub p_pv_used: Vec<Variable>,
}

/// Tiny per-kWh bias favoring full PV utilization [€/kWh]. Even though every real
/// cost term already favors using PV over curtailing it for free (see
/// `pv-export-curtailment` design), HiGHS's `with_mip_gap` tolerance on `u_grid`
/// (grid mutual-exclusion binary) can still accept a solution that curtails PV
/// by a small, cost-irrelevant amount rather than searching for the true
/// optimum — found via `pv_used_equals_forecast_when_no_export_constraint_binds`
/// (no export cap at all, yet the unbiased solver still curtailed). This bias
/// is small enough that any real constraint (export cap, soft-violation
/// penalty) still dominates and forces genuine curtailment when needed.
pub const PV_USE_TIEBREAK_EUR_PER_KWH: f64 = 0.005;

/// Objective term implementing the full-PV-utilization tie-break: a tiny
/// reward per kWh of `p_pv_used`, for every slot.
pub fn pv_use_tiebreak_expr(grid: &GridMilpVars, dt_h: &[f64]) -> Expression {
    let mut expr = Expression::from(0.0);
    for (t, &v) in grid.p_pv_used.iter().enumerate() {
        expr += -(PV_USE_TIEBREAK_EUR_PER_KWH * dt_h[t]) * v;
    }
    expr
}

/// All LP variable handles of one solve: the grid's, and each asset as it declared itself, in
/// planner input order.
pub struct MilpVarPool<'a> {
    pub grid: GridMilpVars,
    pub assets: Vec<Box<dyn DeclaredAsset + 'a>>,
}

/// The site's net controllable power in slot `t` [kW], positive = import: every asset's net
/// power summed. The power balance and the controllable-import malus both read it, so the two
/// cannot disagree about what an asset draws.
pub fn site_net_power_expr(pool: &MilpVarPool, t: usize) -> Expression {
    let mut e = Expression::from(0.0);
    for asset in &pool.assets {
        e += asset.net_power_expr(t);
    }
    e
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
    /// `dt_h[t]` is the slot duration in hours for slot `t`.
    fn objective(&self, iv: &InteractionVars, dt_h: &[f64]) -> Expression;
}

/// Typed LP variable handles returned by each [`AssetInteraction`].
pub enum InteractionVars {
    /// McCormick auxiliary variables for the coexistence penalty, one set per pair.
    BatEvCoexist { pairs: Vec<CoexistPair> },
    /// One continuous slack per slot: excess controllable load beyond free PV surplus.
    CtrlImportMalus { p_ctrl_imp: Vec<Variable> },
}

// ── Discharge / switched-load coexistence interaction ────────────────────────

/// One (discharging asset, switched load) pair of the coexistence penalty, by pool index.
pub struct CoexistPair {
    discharger: usize,
    switched: usize,
    /// `x_coexist[t]` linearises `p_dis[t] × z_on[t]`; `None` for slots whose PV surplus could
    /// not cover the load at its minimum power.
    x_coexist: Vec<Option<Variable>>,
}

/// Penalises an asset discharging into the site while a switched load charges, in slots where
/// PV surplus alone could cover that load. Couples every asset that offers a discharge leg with
/// every other asset that offers an on/off indicator, whatever their kinds; today that is the
/// battery and the EV, hence the name and its config key `c_bat_ev_coexist_eur_kwh`.
///
/// Linearises `p_dis[t] × z_on[t]` via McCormick envelopes for slots where
/// `p_pv[t] − p_base[t] ≥ p_min_kw` of the switched load.
pub struct BatEvCoexistInteraction {
    /// Penalty coefficient [€/kWh].
    pub c_eur_kwh: f64,
}

/// Every (discharger, switched load) pair of distinct assets, in pool order.
fn coexist_pairs(pool: &MilpVarPool) -> Vec<(usize, usize)> {
    let mut pairs = Vec::new();
    for (d, discharger) in pool.assets.iter().enumerate() {
        if discharger.discharge_leg().is_none() {
            continue;
        }
        for (s, switched) in pool.assets.iter().enumerate() {
            if s != d && switched.on_indicator().is_some() {
                pairs.push((d, s));
            }
        }
    }
    pairs
}

impl AssetInteraction for BatEvCoexistInteraction {
    fn applicable(&self, pool: &MilpVarPool) -> bool {
        !coexist_pairs(pool).is_empty()
    }

    fn declare_vars(
        &self,
        pool: &MilpVarPool,
        global: &GlobalMilpInputs,
        vars: &mut ProblemVariables,
    ) -> InteractionVars {
        let pairs = coexist_pairs(pool)
            .into_iter()
            .filter_map(|(d, s)| {
                let leg = pool.assets[d].discharge_leg()?;
                let on = pool.assets[s].on_indicator()?;
                let x_coexist = (0..global.n)
                    .map(|t| {
                        let surplus = global.p_pv_kw[t] - global.p_base_kw[t];
                        (surplus >= on.p_min_kw)
                            .then(|| vars.add(variable().min(0.0).max(leg.max_kw)))
                    })
                    .collect();
                Some(CoexistPair {
                    discharger: d,
                    switched: s,
                    x_coexist,
                })
            })
            .collect();
        InteractionVars::BatEvCoexist { pairs }
    }

    fn constraints(
        &self,
        pool: &MilpVarPool,
        iv: &InteractionVars,
        global: &GlobalMilpInputs,
    ) -> Vec<Constraint> {
        let InteractionVars::BatEvCoexist { pairs } = iv else {
            return vec![];
        };
        let mut cs = Vec::new();
        for pair in pairs {
            let (Some(leg), Some(on)) = (
                pool.assets[pair.discharger].discharge_leg(),
                pool.assets[pair.switched].on_indicator(),
            ) else {
                continue;
            };
            let dis_max = leg.max_kw;
            for t in 0..global.n {
                if let Some(x) = pair.x_coexist[t] {
                    // McCormick envelope for x = p_dis[t] × z_on[t]
                    cs.push(constraint!(x <= leg.p_dis[t]));
                    cs.push(constraint!(x <= dis_max * on.z_on[t]));
                    cs.push(constraint!(
                        x >= leg.p_dis[t] - dis_max * (1.0 - on.z_on[t])
                    ));
                }
            }
        }
        cs
    }

    fn objective(&self, iv: &InteractionVars, dt_h: &[f64]) -> Expression {
        let InteractionVars::BatEvCoexist { pairs } = iv else {
            return Expression::from(0.0);
        };
        let mut obj = Expression::from(0.0);
        for pair in pairs {
            for (t, x) in pair
                .x_coexist
                .iter()
                .enumerate()
                .filter_map(|(t, x)| x.as_ref().map(|v| (t, v)))
            {
                obj += (self.c_eur_kwh * dt_h[t]) * *x;
            }
        }
        obj
    }
}

// ── Controllable-asset import malus interaction ──────────────────────────────

/// Penalises any controllable-asset import that exceeds the free PV surplus.
///
/// For each slot `t`, takes the site's net controllable power `p_ctrl[t]` (every asset's net
/// power, `site_net_power_expr`) and enforces a continuous slack
/// `p_ctrl_imp[t] ≥ p_ctrl[t] − surplus_pv[t]` (≥ 0).
/// `surplus_pv[t] = max(0, p_pv[t] − p_base[t])` is a per-slot constant.
/// The slack is penalised at `c_eur_kwh` in the Phase 1 objective, discouraging assets
/// from importing beyond what PV provides for free — without disabling thermal pre-storage
/// when the future-heating saving genuinely exceeds the malus.
pub struct CtrlImportMalusInteraction {
    pub c_eur_kwh: f64,
}

impl AssetInteraction for CtrlImportMalusInteraction {
    fn applicable(&self, _pool: &MilpVarPool) -> bool {
        true
    }

    fn declare_vars(
        &self,
        _pool: &MilpVarPool,
        global: &GlobalMilpInputs,
        vars: &mut ProblemVariables,
    ) -> InteractionVars {
        let p_ctrl_imp = (0..global.n)
            .map(|_| vars.add(variable().min(0.0)))
            .collect();
        InteractionVars::CtrlImportMalus { p_ctrl_imp }
    }

    fn constraints(
        &self,
        pool: &MilpVarPool,
        iv: &InteractionVars,
        global: &GlobalMilpInputs,
    ) -> Vec<Constraint> {
        let InteractionVars::CtrlImportMalus { p_ctrl_imp } = iv else {
            return vec![];
        };
        let mut cs = Vec::new();
        #[allow(clippy::needless_range_loop)]
        // t indexes multiple arrays (p_ctrl_imp, p_pv_kw, p_base_kw)
        for t in 0..global.n {
            let surplus = (global.p_pv_kw[t] - global.p_base_kw[t]).max(0.0);
            let p_ctrl_t = site_net_power_expr(pool, t);
            // p_ctrl_imp[t] ≥ p_ctrl[t] − surplus_pv[t], combined with min(0.0) bound
            cs.push(constraint!(p_ctrl_imp[t] >= p_ctrl_t - surplus));
        }
        cs
    }

    fn objective(&self, iv: &InteractionVars, dt_h: &[f64]) -> Expression {
        let InteractionVars::CtrlImportMalus { p_ctrl_imp } = iv else {
            return Expression::from(0.0);
        };
        let mut obj = Expression::from(0.0);
        for (t, &x) in p_ctrl_imp.iter().enumerate() {
            obj += (self.c_eur_kwh * dt_h[t]) * x;
        }
        obj
    }
}

// ── Factory ──────────────────────────────────────────────────────────────────

/// Build the list of active cross-asset interactions for one planning cycle.
/// An interaction is only included when its penalty coefficient is non-zero.
pub fn build_interactions(
    c_bat_ev_coexist_eur_kwh: f64,
    c_ctrl_imp_malus_eur_kwh: f64,
) -> Vec<Box<dyn AssetInteraction>> {
    let mut v: Vec<Box<dyn AssetInteraction>> = Vec::new();
    if c_bat_ev_coexist_eur_kwh > 0.0 {
        v.push(Box::new(BatEvCoexistInteraction {
            c_eur_kwh: c_bat_ev_coexist_eur_kwh,
        }));
    }
    if c_ctrl_imp_malus_eur_kwh > 0.0 {
        v.push(Box::new(CtrlImportMalusInteraction {
            c_eur_kwh: c_ctrl_imp_malus_eur_kwh,
        }));
    }
    v
}
