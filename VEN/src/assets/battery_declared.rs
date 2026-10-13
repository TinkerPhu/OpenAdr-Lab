//! The battery as the planner sees it: its variables in one solve and every question the solve
//! asks of them (its term in the site balance, its constraints, its cost and friction, its warm
//! start and its readback), plus how its plan result is priced. The model maths itself lives in
//! `battery_milp.rs`; this module only answers the planner's port, so nothing outside the
//! battery has to know what a battery's variables are. It never reads another asset's variables.

use good_lp::{Constraint, Expression, ProblemVariables, Solution, Variable};

use crate::controller::milp_planner::asset_port::{
    battery_future_state, BatteryMilpContext, BatteryMilpVars,
};
use crate::controller::milp_planner::{
    AllocationBasis, AssetMilpContext, AssetSolution, AssetSolutionDetail, DeclareMode,
    DeclaredAsset, DischargeLeg, ObjectivePart, StorageLegs,
};

/// One battery's variables in one solve.
struct DeclaredBattery<'a> {
    ctx: &'a BatteryMilpContext,
    v: BatteryMilpVars,
}

/// The battery's own winning direction, which only a battery solution carries.
fn own_direction(own: &AssetSolution) -> &[f64] {
    match &own.detail {
        AssetSolutionDetail::Battery { direction } => direction,
        other => unreachable!("a battery is pinned to its own solution, got {other:?}"),
    }
}

impl DeclaredAsset for DeclaredBattery<'_> {
    fn asset_id(&self) -> &str {
        &self.ctx.asset_id
    }

    fn net_power_expr(&self, t: usize) -> Expression {
        self.v.p_ch[t] - self.v.p_dis[t]
    }

    fn constraints(&self, n: usize, dt_h: &[f64]) -> Vec<Constraint> {
        self.ctx.constraints(&self.v, n, dt_h)
    }

    fn objective(&self, part: ObjectivePart<'_>, n: usize, dt_h: &[f64]) -> Expression {
        match part {
            ObjectivePart::Cost(weights) => {
                let mut obj = BatteryMilpContext::objective(
                    &self.v,
                    weights.c_storage_wear_eur_kwh,
                    0.0,
                    0.0,
                    n,
                    dt_h,
                );
                // Stored energy left at the horizon end has forward value; e_bat[n] is the
                // end state of the n + 1 trajectory.
                if self.ctx.c_terminal_eur_kwh > 0.0 && n > 0 {
                    obj += -self.ctx.c_terminal_eur_kwh * self.v.e_bat[n];
                }
                obj
            }
            ObjectivePart::Friction => BatteryMilpContext::objective(
                &self.v,
                0.0,
                self.ctx.friction.startup_eur,
                self.ctx.friction.ramp_eur_kw,
                n,
                dt_h,
            ),
        }
    }

    fn warm_start(&self, own: &AssetSolution, n: usize) -> Vec<(Variable, f64)> {
        let v = &self.v;
        let Some(legs) = &own.storage else {
            return Vec::new();
        };
        let (ch, dis) = (&legs.charge_kw, &legs.discharge_kw);
        let active = |t: usize| {
            if ch[t] + dis[t] > 1e-6 {
                1.0_f64
            } else {
                0.0
            }
        };
        let mut iv = Vec::new();
        for t in 0..=n {
            if let (Some(&e_var), Some(&e_val)) = (v.e_bat.get(t), legs.energy_kwh.get(t)) {
                iv.push((e_var, e_val));
            }
        }
        for t in 0..n {
            iv.push((v.p_ch[t], ch[t].max(0.0)));
            iv.push((v.p_dis[t], dis[t].max(0.0)));
            // R-97: `u_bat` is the battery's *direction selector*, not an activity
            // flag — `p_ch <= ch_max * u_bat` and `p_dis <= dis_max * (1 - u_bat)`
            // (`battery_milp.rs`). Seeding it from an activity test asserted
            // "charging" in every discharging slot, which makes
            // `p_dis <= dis_max * (1 - 1) = 0` contradict the warm start's own
            // `p_dis > 0`. Phase 2 was therefore handed an infeasible start and had
            // to solve from scratch: survivable on an easy instance, and on
            // heater+battery it returned `NoSolutionFound` on essentially every
            // cycle (ven-5/ven-14/ven-17, 29-33 failures per 3 h in production).
            // Idle slots may take either value; 0 is chosen so an idle battery is
            // not nudged toward charging.
            let charging = if ch[t] > 1e-6 { 1.0 } else { 0.0 };
            iv.push((v.u_bat[t], charging));
            // `z_active` genuinely is an activity flag
            // (`p_ch + p_dis <= big_m * z_active`), so it keeps the activity value.
            if let Some(&za) = v.z_active.get(t) {
                iv.push((za, active(t)));
            }
        }
        for i in 0..v.delta_active.len() {
            iv.push((v.delta_active[i], (active(i + 1) - active(i)).max(0.0)));
        }
        for i in 0..v.delta_ramp.len() {
            let t = i + 1;
            let net_prev = ch[i] - dis[i];
            let net_curr = ch[t] - dis[t];
            iv.push((v.delta_ramp[i], (net_curr - net_prev).abs()));
        }
        iv
    }

    fn read(&self, solution: &dyn Solution, n: usize) -> AssetSolution {
        let sol = BatteryMilpContext::read_solution(solution, &self.v, n);
        AssetSolution {
            asset_id: self.ctx.asset_id.clone(),
            net_power_kw: (0..n).map(|t| sol.p_ch_kw[t] - sol.p_dis_kw[t]).collect(),
            // e_kwh[t] is the stored energy at the start of slot t.
            planned_state: (0..n)
                .map(|t| battery_future_state(sol.e_kwh[t], self.ctx.e_nom_kwh))
                .collect(),
            storage: Some(StorageLegs {
                charge_kw: sol.p_ch_kw,
                discharge_kw: sol.p_dis_kw,
                energy_kwh: sol.e_kwh,
            }),
            warnings: Vec::new(),
            detail: AssetSolutionDetail::Battery {
                direction: self.v.u_bat.iter().map(|&u| solution.value(u)).collect(),
            },
        }
    }

    fn discharge_leg(&self) -> Option<DischargeLeg<'_>> {
        Some(DischargeLeg {
            p_dis: &self.v.p_dis,
            max_kw: self.v.dis_max_kw,
        })
    }
}

impl AssetMilpContext for BatteryMilpContext {
    fn asset_id(&self) -> &str {
        &self.asset_id
    }

    fn declare<'a>(
        &'a self,
        n: usize,
        mode: DeclareMode<'_>,
        vars: &mut ProblemVariables,
    ) -> Box<dyn DeclaredAsset + 'a> {
        let priced = mode.friction(&self.friction);
        let pinned_direction = match mode {
            DeclareMode::Plan { .. } => None,
            DeclareMode::Pinned(own) => Some(own_direction(own)),
        };
        let v = self.declare_vars_with(
            n,
            priced.startup_eur,
            priced.ramp_eur_kw,
            vars,
            pinned_direction,
        );
        Box::new(DeclaredBattery { ctx: self, v })
    }

    /// Storage takes the surplus the loads leave: it claims last.
    fn surplus_claim_rank(&self) -> u8 {
        3
    }

    #[cfg(test)]
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn allocation_basis(&self) -> AllocationBasis {
        AllocationBasis::Storage
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::controller::milp_planner::FrictionCosts;

    fn battery() -> BatteryMilpContext {
        BatteryMilpContext {
            asset_id: "battery".into(),
            friction: FrictionCosts {
                startup_eur: 0.01,
                ramp_eur_kw: 0.005,
                tier_penalty_eur: 0.0,
            },
            e_nom_kwh: 11.0,
            e_init_kwh: 9.9,
            e_min_kwh: 1.1,
            e_max_kwh: 11.0,
            p_ch_max_kw: 5.5,
            p_dis_max_kw: 5.5,
            eff_ch: 0.96,
            eff_dis: 0.96,
            c_terminal_eur_kwh: 0.0,
        }
    }

    /// R-97: `u_bat` is the battery's *direction selector*, not an activity flag
    /// (`p_ch <= ch_max * u_bat`, `p_dis <= dis_max * (1 - u_bat)`). A warm start that set it
    /// from an activity test asserted "charging" in every discharging slot, which contradicts
    /// its own `p_dis > 0`: phase 2 was handed an infeasible start, and on the heater+battery
    /// VENs (ven-5, ven-14, ven-17) reported `NoSolutionFound` on essentially every cycle.
    /// `z_active` is a genuine activity flag and keeps the activity value.
    #[test]
    fn warm_start_seeds_the_direction_selector_from_the_direction() {
        let ctx = battery();
        let n = 3;
        let mut vars = good_lp::variables!();
        let declared = DeclaredBattery {
            ctx: &ctx,
            v: ctx.declare_vars(n, 0.01, 0.005, &mut vars),
        };
        // Phase 1 discharges, then charges, then idles.
        let own = AssetSolution {
            asset_id: "battery".into(),
            net_power_kw: vec![-3.0, 2.0, 0.0],
            planned_state: vec![],
            storage: Some(StorageLegs {
                charge_kw: vec![0.0, 2.0, 0.0],
                discharge_kw: vec![3.0, 0.0, 0.0],
                energy_kwh: vec![9.9, 6.8, 8.7, 8.7],
            }),
            warnings: vec![],
            detail: AssetSolutionDetail::Battery {
                direction: vec![0.0, 1.0, 0.0],
            },
        };
        let start: std::collections::HashMap<_, _> =
            declared.warm_start(&own, n).into_iter().collect();
        let direction: Vec<f64> = declared.v.u_bat.iter().map(|u| start[u]).collect();
        let active: Vec<f64> = declared.v.z_active.iter().map(|z| start[z]).collect();
        assert_eq!(
            direction,
            vec![0.0, 1.0, 0.0],
            "discharging must seed u_bat = 0"
        );
        assert_eq!(active, vec![1.0, 1.0, 0.0]);
    }

    fn variables_of(e: Expression) -> std::collections::HashSet<Variable> {
        good_lp::IntoAffineExpression::linear_coefficients(e)
            .filter(|(_, c)| *c != 0.0)
            .map(|(v, _)| v)
            .collect()
    }

    /// Its term in the site balance: charging draws, discharging feeds.
    #[test]
    fn net_power_is_charge_minus_discharge() {
        let ctx = battery();
        let mut vars = good_lp::variables!();
        let v = ctx.declare_vars(2, 0.0, 0.0, &mut vars);
        let (ch, dis) = (v.p_ch[1], v.p_dis[1]);
        let declared = DeclaredBattery { ctx: &ctx, v };
        let coefficients: std::collections::HashMap<Variable, f64> =
            good_lp::IntoAffineExpression::linear_coefficients(declared.net_power_expr(1))
                .collect();
        let expected = std::collections::HashMap::from([(ch, 1.0), (dis, -1.0)]);
        assert_eq!(coefficients, expected);
    }

    /// Cost carries wear and the terminal value and no friction; friction carries the startups
    /// and ramps and no wear.
    #[test]
    fn the_cost_part_prices_wear_and_the_friction_part_prices_starts_and_ramps() {
        let ctx = BatteryMilpContext {
            c_terminal_eur_kwh: 0.2,
            ..battery()
        };
        let n = 3;
        let mut vars = good_lp::variables!();
        let declared = DeclaredBattery {
            ctx: &ctx,
            v: ctx.declare_vars(n, 0.01, 0.005, &mut vars),
        };
        let v = &declared.v;
        let cost = variables_of(declared.objective(
            ObjectivePart::Cost(&crate::controller::milp_planner::CostWeights {
                c_storage_wear_eur_kwh: 0.03,
            }),
            n,
            &[1.0; 3],
        ));
        let friction = variables_of(declared.objective(ObjectivePart::Friction, n, &[1.0; 3]));
        let friction_vars: Vec<Variable> = v
            .delta_active
            .iter()
            .chain(&v.delta_ramp)
            .copied()
            .collect();
        assert!(!friction_vars.is_empty());
        assert!(
            cost.contains(&v.p_ch[0]) && cost.contains(&v.p_dis[0]),
            "wear"
        );
        assert!(cost.contains(&v.e_bat[n]), "terminal value");
        assert!(friction_vars.iter().all(|d| !cost.contains(d)));
        assert!(friction_vars.iter().all(|d| friction.contains(d)));
        assert!(!friction.contains(&v.p_ch[0]) && !friction.contains(&v.e_bat[n]));
    }
}
