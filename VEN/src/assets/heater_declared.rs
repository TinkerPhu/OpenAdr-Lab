//! The heater as the planner sees it: its variables in one solve and every question the solve
//! asks of them (its term in the site balance, its constraints, its cost and friction, its warm
//! start and its readback), plus what it reports about its own plan: the planned tank
//! temperature and the target it is serving. The model maths lives in `heater_milp.rs`; this
//! module only answers the planner's port. It never reads another asset's variables.

use chrono::{DateTime, Utc};
use good_lp::{Constraint, Expression, ProblemVariables, Solution, Variable};

use crate::controller::milp_planner::asset_port::{
    heater_future_state, HeaterMilpContext, HeaterMilpVars, HeaterSolOutput, M_LOW_EUR_PER_KWH,
};
use crate::controller::milp_planner::{
    AllocationBasis, AssetMilpContext, AssetSolution, AssetSolutionDetail, DeclareMode,
    DeclaredAsset, FlexibilityWindow, ObjectivePart,
};
use crate::entities::plan::{PlanWarning, WarningKind, WarningSeverity};

/// Below this the shortfall is solver tolerance, not a missed target [kWh].
const SHORTFALL_TOLERANCE_KWH: f64 = 1e-3;

/// Where this kind stands in claiming PV surplus, lower first: in the plan's allocation
/// order (`AssetMilpContext::surplus_claim_rank`) and in live opportunistic dispatch
/// (`Asset::surplus_claim_rank`), one declaration for both. The heater claims PV surplus after the EV.
pub(crate) const SURPLUS_CLAIM_RANK: u8 = 1;

/// One heater's variables in one solve.
struct DeclaredHeater<'a> {
    ctx: &'a HeaterMilpContext,
    v: HeaterMilpVars,
}

/// The heater's own solution readback, which only a heater solution carries.
fn own_readback(own: &AssetSolution) -> &HeaterSolOutput {
    match &own.detail {
        AssetSolutionDetail::Heater(sol) => sol,
        other => unreachable!("a heater reads its own solution, got {other:?}"),
    }
}

/// The heater's "your target will not be reached in time" warning, read off the solved
/// deadline slack so it cannot disagree with the plan. Typical causes: the thermostat latch
/// holds the heater off for part of the window (R-94), or the window is too short for the
/// heater's power.
fn target_unmet(sol: &HeaterSolOutput) -> Option<PlanWarning> {
    let short_kwh = sol.shortfall_kwh;
    (short_kwh > SHORTFALL_TOLERANCE_KWH).then(|| PlanWarning {
        severity: WarningSeverity::Warning,
        kind: WarningKind::HeaterTargetUnmet,
        message: format!(
            "Heater target not reachable by its ready-by time: {short_kwh:.2} kWh short"
        ),
        suggested_action: Some("allow more time or lower the target temperature".to_string()),
    })
}

impl DeclaredAsset for DeclaredHeater<'_> {
    fn asset_id(&self) -> &str {
        &self.ctx.asset_id
    }

    fn net_power_expr(&self, t: usize) -> Expression {
        self.v.p_step_kw * self.v.y_heat[t]
    }

    fn constraints(&self, n: usize, dt_h: &[f64]) -> Vec<Constraint> {
        self.ctx.constraints(&self.v, n, dt_h)
    }

    /// Cost: the below-minimum penalty and the terminal reward only. The comfort reward stays
    /// out of it: phase 1 has no tier penalty to balance it, so it would be a free bias toward
    /// full stage in phase 1's coarse allocation. Friction: the tier preference, relay
    /// switching and the comfort-curve reward (price and CO2).
    fn objective(&self, part: ObjectivePart<'_>, n: usize, dt_h: &[f64]) -> Expression {
        match part {
            ObjectivePart::Cost(_) => {
                self.ctx
                    .objective(&self.v, 0.0, M_LOW_EUR_PER_KWH, 0.0, 0.0, 0.0, n, dt_h)
            }
            ObjectivePart::Friction => self.ctx.objective(
                &self.v,
                self.ctx.friction.tier_penalty_eur,
                0.0,
                self.ctx.lambda_sw_eur,
                self.ctx.comfort_full_reward_eur_kwh,
                self.ctx.comfort_full_co2_reward_eur_kwh,
                n,
                dt_h,
            ),
        }
    }

    fn warm_start(&self, own: &AssetSolution, n: usize) -> Vec<(Variable, f64)> {
        let v = &self.v;
        let p1 = own_readback(own);
        let mut iv = Vec::new();
        for t in 0..n {
            let y = p1.y_heat[t];
            iv.push((v.y_heat[t], y));
            let e = p1.e_tank_kwh.get(t).copied().unwrap_or(0.0);
            iv.push((v.e_tank[t], e));
            iv.push((v.s_low[t], (-e).max(0.0)));
            let y_prev = if t == 0 {
                self.ctx.initial_y
            } else {
                p1.y_heat[t - 1]
            };
            // Matches C5: sw is the relay-operation count, so a two-stage jump
            // seeds 2 rather than the 1 the old max-of-two-binaries gave.
            iv.push((v.sw[t], (y - y_prev).abs()));
        }
        iv.push((v.z_heat_ready, p1.z_heat_ready));
        // The solved shortfall, so the start satisfies the deadline row exactly.
        iv.push((v.s_short_kwh, p1.shortfall_kwh));
        iv
    }

    fn read(&self, solution: &dyn Solution, n: usize) -> AssetSolution {
        let sol = HeaterMilpContext::read_solution(solution, &self.v, n);
        // e_tank_kwh[t] is the stored energy above temp_min_c.
        let planned_state = sol
            .e_tank_kwh
            .iter()
            .take(n)
            .map(|&e| heater_future_state(e, self.ctx.temp_min_c, self.ctx.thermal_mass_kwh_per_c))
            .collect();
        AssetSolution {
            asset_id: self.ctx.asset_id.clone(),
            net_power_kw: sol.y_heat.iter().map(|&y| y * self.ctx.p_step_kw).collect(),
            planned_state,
            storage: None,
            warnings: target_unmet(&sol).into_iter().collect(),
            detail: AssetSolutionDetail::Heater(sol),
        }
    }
}

impl AssetMilpContext for HeaterMilpContext {
    fn solution_from_trace(
        &self,
        net_kw: &[f64],
        states: &[std::collections::HashMap<String, f64>],
    ) -> AssetSolution {
        let init_temp_c = crate::entities::asset_params::heater_temp_c_from_energy(
            self.e_init_kwh,
            self.temp_min_c,
            self.thermal_mass_kwh_per_c,
        );
        let e_tank_kwh: Vec<f64> = super::trace_values(states, "temp_c", init_temp_c)
            .into_iter()
            .take(net_kw.len())
            .map(|temp_c| {
                crate::entities::asset_params::heater_energy_above_min_kwh(
                    temp_c,
                    self.temp_min_c,
                    self.thermal_mass_kwh_per_c,
                )
            })
            .collect();
        let planned_state = e_tank_kwh
            .iter()
            .map(|&e| heater_future_state(e, self.temp_min_c, self.thermal_mass_kwh_per_c))
            .collect();
        let y_heat = net_kw
            .iter()
            .map(|&kw| {
                if self.p_step_kw > 0.0 {
                    kw / self.p_step_kw
                } else {
                    0.0
                }
            })
            .collect();
        AssetSolution {
            asset_id: self.asset_id.clone(),
            net_power_kw: net_kw.to_vec(),
            planned_state,
            storage: None,
            warnings: Vec::new(),
            detail: AssetSolutionDetail::Heater(HeaterSolOutput {
                y_heat,
                z_heat_ready: 0.0,
                e_tank_kwh,
                // Nothing is guaranteed without look-ahead, so nothing falls short of a guarantee.
                shortfall_kwh: 0.0,
            }),
        }
    }

    fn asset_id(&self) -> &str {
        &self.asset_id
    }

    /// A heater has no friction auxiliaries: its friction is priced on the stage index and the
    /// switching indicators every declaration carries.
    fn declare<'a>(
        &'a self,
        n: usize,
        mode: DeclareMode<'_>,
        vars: &mut ProblemVariables,
    ) -> Box<dyn DeclaredAsset + 'a> {
        let v = match mode {
            DeclareMode::Plan { .. } => self.declare_vars(n, vars),
            DeclareMode::Pinned(own) => self.declare_vars_with(n, vars, Some(own_readback(own))),
        };
        Box::new(DeclaredHeater { ctx: self, v })
    }

    /// The heater claims PV surplus after the EV.
    fn surplus_claim_rank(&self) -> u8 {
        SURPLUS_CLAIM_RANK
    }

    #[cfg(test)]
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn allocation_basis(&self) -> AllocationBasis {
        AllocationBasis::Load
    }

    /// The target this heater serves, from now to its ready-by time: the tank energy the
    /// target needs, at any power from off to every stage on. None without a target, or when
    /// the target needs nothing.
    fn flexibility_window(&self, now: DateTime<Utc>) -> Option<FlexibilityWindow> {
        let window_end = self.target_ready_by?;
        (self.e_target_kwh > 0.0).then_some(FlexibilityWindow {
            energy_needed_kwh: self.e_target_kwh,
            power_min_kw: 0.0,
            power_max_kw: self.p_step_kw * self.n_stages as f64,
            window_start: now,
            window_end,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::controller::milp_planner::asset_port::HeaterMilpMode;
    use crate::controller::milp_planner::{CostWeights, FrictionCosts};
    use good_lp::IntoAffineExpression;

    fn states(key: &str, values: &[f64]) -> Vec<std::collections::HashMap<String, f64>> {
        values
            .iter()
            .map(|&v| std::collections::HashMap::from([(key.to_string(), v)]))
            .collect()
    }

    /// Tank temperatures become tank energy above the minimum, power becomes stages.
    #[test]
    fn solution_from_trace_reads_the_heater_back_from_its_temperatures() {
        let ctx = heater();
        let sol = ctx.solution_from_trace(&[3.0, 0.0], &states("temp_c", &[45.0, 50.0, 49.0]));
        let AssetSolutionDetail::Heater(out) = &sol.detail else {
            panic!("a heater reads back a heater detail")
        };
        assert_eq!(out.y_heat, vec![2.0, 0.0], "3 kW is two 1.5 kW stages");
        assert_eq!(out.e_tank_kwh.len(), 2);
        assert!((out.e_tank_kwh[0] - (45.0 - 40.0) * 0.2).abs() < 1e-12);
        assert!((sol.planned_state[1]["temp_c"] - 50.0).abs() < 1e-9);
    }

    fn heater() -> HeaterMilpContext {
        HeaterMilpContext {
            asset_id: "boiler".into(),
            friction: FrictionCosts {
                tier_penalty_eur: 0.05,
                ..Default::default()
            },
            temp_min_c: 40.0,
            thermal_mass_kwh_per_c: 0.2,
            target_ready_by: None,
            mode: HeaterMilpMode::MayRun,
            t_dead_step: None,
            p_step_kw: 1.5,
            n_stages: 2,
            e_init_kwh: 2.0,
            e_max_kwh: 5.0,
            q_dem_kw: 0.3,
            e_target_kwh: 5.0,
            lambda_sw_eur: 0.02,
            initial_y: 0.0,
            c_terminal_eur_kwh: 0.1,
            anchored_kw: vec![],
            comfort_full_reward_eur_kwh: 0.0,
            comfort_full_co2_reward_eur_kwh: 0.0,
        }
    }

    /// Its term in the site balance: the power of one stage per stage switched on.
    #[test]
    fn net_power_is_the_stage_power() {
        let ctx = heater();
        let mut vars = good_lp::variables!();
        let v = ctx.declare_vars(2, &mut vars);
        let y = v.y_heat[1];
        let declared = DeclaredHeater { ctx: &ctx, v };
        let coefficients: Vec<_> = declared.net_power_expr(1).linear_coefficients().collect();
        assert_eq!(coefficients, vec![(y, 1.5)]);
    }

    /// Cost carries the below-minimum penalty and the terminal value and no switching; friction
    /// carries the switching and the tier preference and no terminal value.
    #[test]
    fn the_cost_part_prices_the_tank_and_the_friction_part_prices_switching() {
        let ctx = heater();
        let n = 3;
        let mut vars = good_lp::variables!();
        let declared = DeclaredHeater {
            ctx: &ctx,
            v: ctx.declare_vars(n, &mut vars),
        };
        let v = &declared.v;
        let vars_of = |e: Expression| -> std::collections::HashSet<Variable> {
            e.linear_coefficients()
                .filter(|(_, c)| *c != 0.0)
                .map(|(var, _)| var)
                .collect()
        };
        let cost = vars_of(declared.objective(
            ObjectivePart::Cost(&CostWeights {
                c_storage_wear_eur_kwh: 0.03,
            }),
            n,
            &[1.0; 3],
        ));
        let friction = vars_of(declared.objective(ObjectivePart::Friction, n, &[1.0; 3]));
        assert!(cost.contains(&v.s_low[0]) && cost.contains(&v.e_tank[n - 1]));
        assert!(!cost.contains(&v.sw[1]) && !cost.contains(&v.y_heat[1]));
        assert!(friction.contains(&v.sw[1]) && friction.contains(&v.y_heat[1]));
        assert!(!friction.contains(&v.e_tank[n - 1]) && !friction.contains(&v.s_low[0]));
    }

    /// The target the heater serves, from now to its ready-by time, at any power up to every
    /// stage on; no target, no window.
    #[test]
    fn the_flexibility_window_is_the_target() {
        let now = chrono::Utc::now();
        assert_eq!(heater().flexibility_window(now), None);
        let ready_by = now + chrono::Duration::hours(2);
        let window = HeaterMilpContext {
            target_ready_by: Some(ready_by),
            ..heater()
        }
        .flexibility_window(now)
        .expect("a target with energy to add");
        assert_eq!((window.window_start, window.window_end), (now, ready_by));
        assert_eq!((window.power_min_kw, window.power_max_kw), (0.0, 3.0));
        assert_eq!(window.energy_needed_kwh, 5.0);
    }
}
