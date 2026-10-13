// ── Battery MILP plugin types ─────────────────────────────────────────────────
// Struct definitions live in `controller::milp_planner::asset_port`.
// Method implementations (declare_vars, constraints, objective, read_solution, from_state)
// are in the `impl` blocks below (cross-file inherent impl — valid Rust).

use good_lp::{constraint, variable, Constraint, Expression, ProblemVariables, Solution};

use super::Battery;
use crate::controller::milp_planner::asset_port::{
    pinned_binary, BatteryMilpContext, BatteryMilpVars, BatterySolOutput,
};

impl BatteryMilpContext {
    /// Declare all LP variables for this battery, every decision free: the tests' shorthand for
    /// `declare_vars_with` (the planner declares through `AssetMilpContext::declare`).
    #[cfg(test)]
    pub fn declare_vars(
        &self,
        n: usize,
        c_startup_eur: f64,
        c_ramp_eur_kw: f64,
        vars: &mut ProblemVariables,
    ) -> BatteryMilpVars {
        self.declare_vars_with(n, c_startup_eur, c_ramp_eur_kw, vars, None)
    }

    /// The one battery variable declaration (R-98): the plan and the marginal-cost pass
    /// differ only in how `u_bat` is declared. `pinned_direction` is the winning solution's
    /// direction per slot; a pinned pass prices fixed decisions, so it carries no
    /// startup/ramp auxiliaries.
    pub fn declare_vars_with(
        &self,
        n: usize,
        c_startup_eur: f64,
        c_ramp_eur_kw: f64,
        vars: &mut ProblemVariables,
        pinned_direction: Option<&[f64]>,
    ) -> BatteryMilpVars {
        let (c_startup_eur, c_ramp_eur_kw) = match pinned_direction {
            None => (c_startup_eur, c_ramp_eur_kw),
            Some(_) => (0.0, 0.0),
        };
        let p_ch = (0..n)
            .map(|_| vars.add(variable().min(0.0).max(self.p_ch_max_kw)))
            .collect();
        let p_dis = (0..n)
            .map(|_| vars.add(variable().min(0.0).max(self.p_dis_max_kw)))
            .collect();
        let u_bat = (0..n)
            .map(|t| match pinned_direction {
                None => vars.add(variable().binary()),
                Some(direction) => {
                    let v = pinned_binary(direction.get(t).copied());
                    vars.add(variable().min(v).max(v))
                }
            })
            .collect();
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
    pub fn read_solution(
        sol: &(impl Solution + ?Sized),
        v: &BatteryMilpVars,
        n: usize,
    ) -> BatterySolOutput {
        BatterySolOutput {
            p_ch_kw: (0..n).map(|t| sol.value(v.p_ch[t])).collect(),
            p_dis_kw: (0..n).map(|t| sol.value(v.p_dis[t])).collect(),
            e_kwh: (0..=n).map(|t| sol.value(v.e_bat[t])).collect(),
        }
    }

    /// Construct from a live `AssetState` and the current sim `Battery` config.
    pub fn from_state(
        asset_id: &str,
        state: &super::AssetState,
        cfg: &Battery,
        c_terminal_eur_kwh: f64,
    ) -> Self {
        let live_soc = if let super::AssetState::Battery(s) = state {
            s.soc_frac
        } else {
            0.5
        };
        cfg.build_milp_context(asset_id, live_soc, c_terminal_eur_kwh)
    }
}

impl Battery {
    /// Build the MILP context from the live SoC (not the profile initial_soc).
    pub fn build_milp_context(
        &self,
        asset_id: &str,
        live_soc: f64,
        c_terminal_eur_kwh: f64,
    ) -> BatteryMilpContext {
        let cap = self.capacity_kwh;
        let eff = self.round_trip_efficiency.sqrt();
        BatteryMilpContext {
            asset_id: asset_id.to_string(),
            friction: Default::default(),
            e_nom_kwh: cap,
            e_init_kwh: live_soc * cap,
            e_min_kwh: self.min_soc_frac * cap,
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
    use crate::controller::milp_planner::AssetMilpContext;
    use crate::services::test_support::milp_pool::bound_range;
    use good_lp::variables;

    fn make_ctx() -> BatteryMilpContext {
        BatteryMilpContext {
            asset_id: "battery".into(),
            friction: Default::default(),
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

    #[test]
    fn asset_id_is_battery() {
        assert_eq!(make_ctx().asset_id(), "battery");
    }

    #[test]
    fn declare_vars_without_friction_declares_no_startup_or_ramp_vars() {
        let ctx = make_ctx();
        let n = 4;
        let mut vars = variables!();
        let v = ctx.declare_vars(n, 0.0, 0.0, &mut vars);
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
        let v = ctx.declare_vars(n, 0.0, 0.0, &mut vars);
        let cs = ctx.constraints(&v, n, &dt_h);
        assert!(
            cs.len() > n * 3,
            "expected at least {} constraints, got {}",
            n * 3 + 1,
            cs.len()
        );
    }

    /// R-98: the marginal-cost pass declares the battery through the plan's own
    /// declaration — same variables, with `u_bat` fixed to the winning value.
    #[test]
    fn declare_pinned_vars_is_the_free_declaration_with_u_bat_fixed() {
        let ctx = make_ctx();
        let n = 4;
        let direction = vec![1.0, 0.0, 0.9, 0.2];
        let shape = |pinned: bool| {
            let mut vars = variables!();
            let v = ctx.declare_vars_with(n, 0.0, 0.0, &mut vars, pinned.then_some(&direction[..]));
            (
                v.p_ch.len(),
                v.p_dis.len(),
                v.u_bat.len(),
                v.e_bat.len(),
                v.z_active.len(),
            )
        };
        assert_eq!(shape(true), shape(false));

        let range = bound_range(|vars| {
            ctx.declare_vars_with(n, 0.0, 0.0, vars, Some(&direction))
                .u_bat
        });
        assert_eq!(range, vec![(1.0, 1.0), (0.0, 0.0), (1.0, 1.0), (0.0, 0.0)]);
    }
}
