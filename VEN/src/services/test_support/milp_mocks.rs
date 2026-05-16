//! Lightweight MILP context test doubles — compiled in all builds (not #[cfg(test)]).
//! Implements AssetMilpContext for Battery/EV/Heater without importing from crate::assets.
//! Use these in planner unit tests to avoid a dependency on full asset physics.

use chrono::{DateTime, Utc};
use good_lp::{Constraint, Expression, ProblemVariables};

use crate::controller::milp_interactions::MilpVarPool;
use crate::controller::milp_planner::{
    AssetKind, AssetMilpContext, AssetMilpParams, BatteryScalars, EvScalars, HeaterScalars,
    MilpLoadMode,
};
use crate::controller::milp_planner::asset_port::{
    BatteryMilpContext, EvMilpContext, EvMilpMode,
    HeaterMilpContext, HeaterMilpMode,
};

// ── MockBatteryCtx ───────────────────────────────────────────────────────────

/// Minimal battery context for planner unit tests.
pub struct MockBatteryCtx {
    pub ctx: BatteryMilpContext,
}

impl MockBatteryCtx {
    pub fn new(e_nom_kwh: f64, e_init_kwh: f64, e_min_kwh: f64, p_max_kw: f64, eff: f64) -> Self {
        Self {
            ctx: BatteryMilpContext {
                e_nom_kwh,
                e_init_kwh,
                e_min_kwh,
                e_max_kwh: e_nom_kwh,
                p_ch_max_kw: p_max_kw,
                p_dis_max_kw: p_max_kw,
                eff_ch: eff,
                eff_dis: eff,
            },
        }
    }
}

impl AssetMilpContext for MockBatteryCtx {
    fn asset_id(&self) -> &str {
        "mock_battery"
    }

    fn asset_kind(&self) -> AssetKind {
        AssetKind::Battery
    }

    fn milp_params(&self, _n: usize, _step_s: u64, _now: DateTime<Utc>) -> AssetMilpParams {
        AssetMilpParams::Battery(BatteryScalars {
            e_nom_kwh: self.ctx.e_nom_kwh,
            e_init_kwh: self.ctx.e_init_kwh,
            e_min_kwh: self.ctx.e_min_kwh,
            e_max_kwh: self.ctx.e_max_kwh,
            p_ch_max_kw: self.ctx.p_ch_max_kw,
            p_dis_max_kw: self.ctx.p_dis_max_kw,
            eff_ch: self.ctx.eff_ch,
            eff_dis: self.ctx.eff_dis,
        })
    }

    fn declare_vars_into_pool(
        &self,
        n: usize,
        c_startup_eur: f64,
        c_ramp_eur_kw: f64,
        vars: &mut ProblemVariables,
        pool: &mut MilpVarPool,
    ) {
        pool.bat = Some(self.ctx.declare_vars(n, c_startup_eur, c_ramp_eur_kw, vars));
    }

    fn constraints(&self, pool: &MilpVarPool, n: usize, dt_h: f64) -> Vec<Constraint> {
        BatteryMilpContext::constraints(&self.ctx, pool.bat.as_ref().unwrap(), n, dt_h)
    }

    fn objective(
        &self,
        pool: &MilpVarPool,
        n: usize,
        dt_h: f64,
        c_wear_eur_kwh: f64,
        c_startup_eur: f64,
        c_ramp_eur_kw: f64,
    ) -> Expression {
        BatteryMilpContext::objective(
            pool.bat.as_ref().unwrap(),
            c_wear_eur_kwh,
            c_startup_eur,
            c_ramp_eur_kw,
            n,
            dt_h,
        )
    }
}

// ── MockEvCtx ────────────────────────────────────────────────────────────────

/// Minimal EV context for planner unit tests.
pub struct MockEvCtx {
    pub ctx: EvMilpContext,
}

impl MockEvCtx {
    pub fn must_not_run(n: usize, p_max_kw: f64) -> Self {
        Self {
            ctx: EvMilpContext {
                mode: EvMilpMode::MustNotRun,
                a_ev: vec![false; n],
                t_dead_step: None,
                p_max_kw,
                p_min_kw: 0.0,
                e_core_kwh: 0.0,
                e_extra_max_kwh: 0.0,
                v_extra_eur_kwh: 0.0,
            },
        }
    }

    pub fn must_run(n: usize, p_max_kw: f64, e_core_kwh: f64) -> Self {
        Self {
            ctx: EvMilpContext {
                mode: EvMilpMode::MustRun,
                a_ev: vec![true; n],
                t_dead_step: Some(n - 1),
                p_max_kw,
                p_min_kw: 0.0,
                e_core_kwh,
                e_extra_max_kwh: 0.0,
                v_extra_eur_kwh: 0.05,
            },
        }
    }
}

impl AssetMilpContext for MockEvCtx {
    fn asset_id(&self) -> &str {
        "mock_ev"
    }

    fn asset_kind(&self) -> AssetKind {
        AssetKind::Ev
    }

    fn milp_params(&self, _n: usize, _step_s: u64, _now: DateTime<Utc>) -> AssetMilpParams {
        let mode = match self.ctx.mode {
            EvMilpMode::MustRun => MilpLoadMode::MustRun,
            EvMilpMode::MayRun => MilpLoadMode::MayRun,
            EvMilpMode::MustNotRun => MilpLoadMode::MustNotRun,
        };
        AssetMilpParams::Ev(EvScalars {
            mode,
            a_ev: self.ctx.a_ev.clone(),
            t_dead_step: self.ctx.t_dead_step,
            p_max_kw: self.ctx.p_max_kw,
            p_min_kw: self.ctx.p_min_kw,
            e_core_kwh: self.ctx.e_core_kwh,
            e_extra_max_kwh: self.ctx.e_extra_max_kwh,
            v_extra_eur_kwh: self.ctx.v_extra_eur_kwh,
        })
    }

    fn declare_vars_into_pool(
        &self,
        n: usize,
        c_startup_eur: f64,
        c_ramp_eur_kw: f64,
        vars: &mut ProblemVariables,
        pool: &mut MilpVarPool,
    ) {
        pool.ev = Some(self.ctx.declare_vars(n, c_startup_eur, c_ramp_eur_kw, vars));
    }

    fn constraints(&self, pool: &MilpVarPool, n: usize, dt_h: f64) -> Vec<Constraint> {
        EvMilpContext::constraints(&self.ctx, pool.ev.as_ref().unwrap(), n, dt_h)
    }

    fn objective(
        &self,
        pool: &MilpVarPool,
        n: usize,
        _dt_h: f64,
        _c_wear_eur_kwh: f64,
        c_startup_eur: f64,
        c_ramp_eur_kw: f64,
    ) -> Expression {
        let (startup, ramp, w_services) = if c_startup_eur == 0.0 {
            (0.0_f64, 0.0_f64, 1.0_f64)
        } else {
            (c_startup_eur, c_ramp_eur_kw, 0.0_f64)
        };
        EvMilpContext::objective(&self.ctx, pool.ev.as_ref().unwrap(), startup, ramp, w_services, n)
    }
}

// ── MockHeaterCtx ────────────────────────────────────────────────────────────

/// Minimal heater context for planner unit tests.
pub struct MockHeaterCtx {
    pub ctx: HeaterMilpContext,
}

impl MockHeaterCtx {
    pub fn may_run(p_full_kw: f64, p_mid_kw: f64, e_init_kwh: f64, e_max_kwh: f64) -> Self {
        Self {
            ctx: HeaterMilpContext {
                mode: HeaterMilpMode::MayRun,
                t_dead_step: None,
                p_mid_kw,
                p_full_kw,
                e_init_kwh,
                e_max_kwh,
                q_dem_kw: 0.3,
                e_target_kwh: e_max_kwh,
                lambda_sw_eur: 0.0,
                initial_z_mid: 0.0,
                initial_z_full: 0.0,
            },
        }
    }
}

impl AssetMilpContext for MockHeaterCtx {
    fn asset_id(&self) -> &str {
        "mock_heater"
    }

    fn asset_kind(&self) -> AssetKind {
        AssetKind::Heater
    }

    fn milp_params(&self, _n: usize, _step_s: u64, _now: DateTime<Utc>) -> AssetMilpParams {
        let mode = match self.ctx.mode {
            HeaterMilpMode::MustRun => MilpLoadMode::MustRun,
            HeaterMilpMode::MayRun => MilpLoadMode::MayRun,
            HeaterMilpMode::MustNotRun => MilpLoadMode::MustNotRun,
        };
        AssetMilpParams::Heater(HeaterScalars {
            mode,
            t_dead_step: self.ctx.t_dead_step,
            p_mid_kw: self.ctx.p_mid_kw,
            p_full_kw: self.ctx.p_full_kw,
            e_init_kwh: self.ctx.e_init_kwh,
            e_max_kwh: self.ctx.e_max_kwh,
            q_dem_kw: self.ctx.q_dem_kw,
            e_target_kwh: self.ctx.e_target_kwh,
            lambda_sw_eur: self.ctx.lambda_sw_eur,
            initial_z_mid: self.ctx.initial_z_mid,
            initial_z_full: self.ctx.initial_z_full,
        })
    }

    fn declare_vars_into_pool(
        &self,
        n: usize,
        _c_startup_eur: f64,
        _c_ramp_eur_kw: f64,
        vars: &mut ProblemVariables,
        pool: &mut MilpVarPool,
    ) {
        pool.heater = Some(self.ctx.declare_vars(n, vars));
    }

    fn constraints(&self, pool: &MilpVarPool, n: usize, dt_h: f64) -> Vec<Constraint> {
        HeaterMilpContext::constraints(&self.ctx, pool.heater.as_ref().unwrap(), n, dt_h)
    }

    fn objective(
        &self,
        pool: &MilpVarPool,
        n: usize,
        _dt_h: f64,
        _c_wear_eur_kwh: f64,
        c_startup_eur: f64,
        c_ramp_eur_kw: f64,
    ) -> Expression {
        use crate::controller::milp_planner::asset_port::M_LOW_EUR_PER_KWH;
        let v = pool.heater.as_ref().unwrap();
        if c_startup_eur == 0.0 {
            HeaterMilpContext::objective(&self.ctx, v, 0.0, M_LOW_EUR_PER_KWH, n)
        } else {
            HeaterMilpContext::objective(&self.ctx, v, c_ramp_eur_kw, 0.0, n)
        }
    }
}
