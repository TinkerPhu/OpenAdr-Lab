// ── Battery MILP plugin types ─────────────────────────────────────────────────
// Struct definitions live in `controller::milp_planner::asset_port`.
// Method implementations (declare_vars, constraints, objective, read_solution, from_state)
// are in the `impl` blocks below (cross-file inherent impl — valid Rust).

use good_lp::{constraint, variable, Constraint, Expression, ProblemVariables, Solution};

use super::Battery;
use crate::controller::milp_planner::asset_port::{
    BatteryMilpContext, BatteryMilpVars, BatterySolOutput,
};

impl BatteryMilpContext {
    /// Declare all LP variables for this battery. Context-side canonical implementation;
    /// `Battery::build_milp_context` builds this context, not a delegate call.
    pub fn declare_vars(
        &self,
        n: usize,
        c_startup_eur: f64,
        c_ramp_eur_kw: f64,
        vars: &mut ProblemVariables,
    ) -> BatteryMilpVars {
        let p_ch = (0..n)
            .map(|_| vars.add(variable().min(0.0).max(self.p_ch_max_kw)))
            .collect();
        let p_dis = (0..n)
            .map(|_| vars.add(variable().min(0.0).max(self.p_dis_max_kw)))
            .collect();
        let u_bat = (0..n).map(|_| vars.add(variable().binary())).collect();
        let e_bat = (0..=n)
            .map(|i| {
                if i == 0 {
                    vars.add(variable().min(self.e_init_kwh).max(self.e_init_kwh))
                } else {
                    vars.add(variable().min(self.e_min_kwh).max(self.e_max_kwh))
                }
            })
            .collect();
        let z_active = if n > 1 && c_startup_eur > 0.0 {
            (0..n).map(|_| vars.add(variable().binary())).collect()
        } else {
            vec![]
        };
        let delta_active = if n > 1 && c_startup_eur > 0.0 {
            (0..n - 1).map(|_| vars.add(variable().binary())).collect()
        } else {
            vec![]
        };
        let delta_ramp = if n > 1 && c_ramp_eur_kw > 0.0 {
            (0..n - 1).map(|_| vars.add(variable().min(0.0))).collect()
        } else {
            vec![]
        };
        BatteryMilpVars {
            p_ch,
            p_dis,
            u_bat,
            e_bat,
            z_active,
            delta_active,
            delta_ramp,
            dis_max_kw: self.p_dis_max_kw,
        }
    }

    /// Generate all MILP constraints for this battery. Context-side canonical implementation.
    /// `dt_h[t]` is the slot duration in hours for slot `t`.
    pub fn constraints(&self, v: &BatteryMilpVars, n: usize, dt_h: &[f64]) -> Vec<Constraint> {
        let mut cs: Vec<Constraint> = Vec::new();
        for (t, &dt) in dt_h.iter().enumerate().take(n) {
            cs.push(constraint!(v.p_ch[t] <= self.p_ch_max_kw * v.u_bat[t]));
            cs.push(constraint!(
                v.p_dis[t] <= self.p_dis_max_kw * (1.0 - v.u_bat[t])
            ));
            cs.push(constraint!(
                v.e_bat[t + 1]
                    == v.e_bat[t] + dt * self.eff_ch * v.p_ch[t]
                        - dt * (1.0 / self.eff_dis) * v.p_dis[t]
            ));
            if let Some(&z) = v.z_active.get(t) {
                let big_m = self.p_ch_max_kw + self.p_dis_max_kw;
                cs.push(constraint!(v.p_ch[t] + v.p_dis[t] <= big_m * z));
            }
        }
        for i in 0..v.delta_active.len() {
            let t = i + 1;
            cs.push(constraint!(
                v.delta_active[i] >= v.z_active[t] - v.z_active[t - 1]
            ));
        }
        for i in 0..v.delta_ramp.len() {
            let t = i + 1;
            cs.push(constraint!(
                v.delta_ramp[i] >= (v.p_ch[t] - v.p_dis[t]) - (v.p_ch[t - 1] - v.p_dis[t - 1])
            ));
            cs.push(constraint!(
                v.delta_ramp[i] >= (v.p_ch[t - 1] - v.p_dis[t - 1]) - (v.p_ch[t] - v.p_dis[t])
            ));
        }
        cs.push(constraint!(v.e_bat[n] >= self.e_init_kwh));
        cs
    }

    /// Battery objective contribution. Associated function (no `self` needed — no ctx params used).
    /// `dt_h[t]` is the slot duration in hours for slot `t`.
    pub fn objective(
        v: &BatteryMilpVars,
        c_wear_eur_kwh: f64,
        c_startup_eur: f64,
        c_ramp_eur_kw: f64,
        n: usize,
        dt_h: &[f64],
    ) -> Expression {
        let mut obj = Expression::from(0.0);
        for (t, &dt) in dt_h.iter().enumerate().take(n) {
            obj += (c_wear_eur_kwh * dt) * v.p_ch[t];
            obj += (c_wear_eur_kwh * dt) * v.p_dis[t];
            if t >= 1 {
                if let Some(&d) = v.delta_active.get(t - 1) {
                    obj += c_startup_eur * d;
                }
                if let Some(&d) = v.delta_ramp.get(t - 1) {
                    obj += c_ramp_eur_kw * d;
                }
            }
        }
        obj
    }

    /// Read back the battery solution. Associated function (no `self` needed).
    pub fn read_solution(sol: &impl Solution, v: &BatteryMilpVars, n: usize) -> BatterySolOutput {
        BatterySolOutput {
            p_ch_kw: (0..n).map(|t| sol.value(v.p_ch[t])).collect(),
            p_dis_kw: (0..n).map(|t| sol.value(v.p_dis[t])).collect(),
            e_kwh: (0..=n).map(|t| sol.value(v.e_bat[t])).collect(),
        }
    }

    /// Construct from a live `AssetState` and the current sim `Battery` config.
    pub fn from_state(state: &super::AssetState, cfg: &Battery, c_terminal_eur_kwh: f64) -> Self {
        let live_soc = if let super::AssetState::Battery(s) = state {
            s.soc
        } else {
            0.5
        };
        cfg.build_milp_context(live_soc, c_terminal_eur_kwh)
    }
}

impl crate::controller::milp_planner::AssetMilpContext for BatteryMilpContext {
    fn asset_id(&self) -> &str {
        crate::ids::ASSET_BATTERY
    }

    fn asset_kind(&self) -> crate::controller::milp_planner::AssetKind {
        crate::controller::milp_planner::AssetKind::Battery
    }

    fn milp_params(
        &self,
        _n: usize,
        _now: chrono::DateTime<chrono::Utc>,
    ) -> crate::controller::milp_planner::AssetMilpParams {
        crate::controller::milp_planner::AssetMilpParams::Battery(
            crate::controller::milp_planner::BatteryScalars {
                e_nom_kwh: self.e_nom_kwh,
                e_init_kwh: self.e_init_kwh,
                e_min_kwh: self.e_min_kwh,
                e_max_kwh: self.e_max_kwh,
                p_ch_max_kw: self.p_ch_max_kw,
                p_dis_max_kw: self.p_dis_max_kw,
                eff_ch: self.eff_ch,
                eff_dis: self.eff_dis,
            },
        )
    }

    fn declare_vars_into_pool(
        &self,
        n: usize,
        c_startup_eur: f64,
        c_ramp_eur_kw: f64,
        vars: &mut ProblemVariables,
        pool: &mut crate::controller::milp_interactions::MilpVarPool,
    ) {
        pool.bat = Some(self.declare_vars(n, c_startup_eur, c_ramp_eur_kw, vars));
    }

    fn constraints(
        &self,
        pool: &crate::controller::milp_interactions::MilpVarPool,
        n: usize,
        dt_h: &[f64],
    ) -> Vec<Constraint> {
        BatteryMilpContext::constraints(self, pool.bat.as_ref().unwrap(), n, dt_h)
    }

    fn objective(
        &self,
        pool: &crate::controller::milp_interactions::MilpVarPool,
        n: usize,
        dt_h: &[f64],
        c_wear_eur_kwh: f64,
        c_startup_eur: f64,
        c_ramp_eur_kw: f64,
    ) -> Expression {
        let v = pool.bat.as_ref().unwrap();
        let mut obj =
            BatteryMilpContext::objective(v, c_wear_eur_kwh, c_startup_eur, c_ramp_eur_kw, n, dt_h);
        // Terminal energy reward in Phase 1 only (c_startup_eur == 0.0).
        // e_bat[n] is the SoC trajectory end-state (index n+1 of the n+1 vector).
        if c_startup_eur == 0.0 && self.c_terminal_eur_kwh > 0.0 && n > 0 {
            obj += -self.c_terminal_eur_kwh * v.e_bat[n];
        }
        obj
    }
}

impl Battery {
    /// Build the MILP context from the live SoC (not the profile initial_soc).
    pub fn build_milp_context(&self, live_soc: f64, c_terminal_eur_kwh: f64) -> BatteryMilpContext {
        let cap = self.capacity_kwh;
        let eff = self.round_trip_efficiency.sqrt();
        BatteryMilpContext {
            e_nom_kwh: cap,
            e_init_kwh: live_soc * cap,
            e_min_kwh: self.min_soc * cap,
            e_max_kwh: cap,
            p_ch_max_kw: self.max_charge_kw,
            p_dis_max_kw: self.max_discharge_kw,
            eff_ch: eff,
            eff_dis: eff,
            c_terminal_eur_kwh,
        }
    }
}

#[cfg(test)]
mod milp_context_trait_tests {
    use super::*;
    use crate::controller::milp_interactions::{GridMilpVars, MilpVarPool};
    use crate::controller::milp_planner::{AssetKind, AssetMilpContext, AssetMilpParams};
    use good_lp::{variable, variables};

    fn make_ctx() -> BatteryMilpContext {
        BatteryMilpContext {
            e_nom_kwh: 10.0,
            e_init_kwh: 5.0,
            e_min_kwh: 1.0,
            e_max_kwh: 10.0,
            p_ch_max_kw: 5.0,
            p_dis_max_kw: 5.0,
            eff_ch: 0.9746794_f64.sqrt(),
            eff_dis: 0.9746794_f64.sqrt(),
            c_terminal_eur_kwh: 0.0,
        }
    }

    fn empty_pool(vars: &mut good_lp::ProblemVariables, n: usize) -> MilpVarPool {
        let grid = GridMilpVars {
            p_imp: (0..n).map(|_| vars.add(variable().min(0.0))).collect(),
            p_exp: (0..n).map(|_| vars.add(variable().min(0.0))).collect(),
            u_grid: (0..n).map(|_| vars.add(variable().binary())).collect(),
            s_imp_viol: (0..n).map(|_| vars.add(variable().min(0.0))).collect(),
            s_exp_viol: (0..n).map(|_| vars.add(variable().min(0.0))).collect(),
        };
        MilpVarPool {
            grid,
            bat: None,
            ev: None,
            heater: None,
            shiftable: vec![],
        }
    }

    #[test]
    fn asset_id_is_battery() {
        assert_eq!(make_ctx().asset_id(), "battery");
    }

    #[test]
    fn asset_kind_is_battery() {
        assert_eq!(make_ctx().asset_kind(), AssetKind::Battery);
    }

    #[test]
    fn milp_params_returns_correct_battery_scalars() {
        let ctx = make_ctx();
        let params = ctx.milp_params(4, chrono::Utc::now());
        match params {
            AssetMilpParams::Battery(b) => {
                assert!((b.e_nom_kwh - 10.0).abs() < 1e-9);
                assert!((b.e_init_kwh - 5.0).abs() < 1e-9);
                assert!((b.e_min_kwh - 1.0).abs() < 1e-9);
                assert!((b.p_ch_max_kw - 5.0).abs() < 1e-9);
                assert!((b.p_dis_max_kw - 5.0).abs() < 1e-9);
            }
            _ => panic!("expected AssetMilpParams::Battery"),
        }
    }

    #[test]
    fn declare_vars_into_pool_fills_bat_slot() {
        let ctx = make_ctx();
        let n = 4;
        let mut vars = variables!();
        let mut pool = empty_pool(&mut vars, n);
        ctx.declare_vars_into_pool(n, 0.0, 0.0, &mut vars, &mut pool);
        let v = pool
            .bat
            .as_ref()
            .expect("pool.bat should be Some after declare");
        assert_eq!(v.p_ch.len(), n);
        assert_eq!(v.p_dis.len(), n);
        assert_eq!(v.e_bat.len(), n + 1);
        assert!(v.z_active.is_empty()); // no startup vars when c_startup=0
        assert!(v.delta_ramp.is_empty()); // no ramp vars when c_ramp=0
    }

    #[test]
    fn constraints_non_empty_for_n4() {
        let ctx = make_ctx();
        let n = 4;
        let dt_h = vec![300.0 / 3600.0; n];
        let mut vars = variables!();
        let mut pool = empty_pool(&mut vars, n);
        ctx.declare_vars_into_pool(n, 0.0, 0.0, &mut vars, &mut pool);
        let cs = AssetMilpContext::constraints(&ctx, &pool, n, &dt_h);
        assert!(
            cs.len() > n * 3,
            "expected at least {} constraints, got {}",
            n * 3 + 1,
            cs.len()
        );
    }
}
