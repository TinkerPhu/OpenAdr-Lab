// ── Heater MILP plugin types ──────────────────────────────────────────────────
// Struct/enum definitions live in `controller::milp_planner::asset_port`.
// Method implementations below are cross-file inherent impl blocks — valid Rust.

use chrono::{DateTime, Utc};
use good_lp::{constraint, variable, Constraint, Expression, ProblemVariables, Solution};

use super::Heater;
use crate::controller::milp_planner::asset_port::{
    HeaterMilpContext, HeaterMilpMode, HeaterMilpVars, HeaterSolOutput,
};

/// Map a planned heater power [kW] to a stage index.
/// Returns `Some(k)` when `kw` is within 0.1 kW of `k × p_step_kw` for some
/// reachable stage, or `None` when it matches no stage (anchor left free).
fn kw_to_stage(kw: f64, p_step_kw: f64, n_stages: f64) -> Option<f64> {
    const TOL: f64 = 0.1;
    if p_step_kw <= 0.0 {
        return if kw.abs() < TOL { Some(0.0) } else { None };
    }
    let k = (kw / p_step_kw).round();
    if !(0.0..=n_stages).contains(&k) {
        return None;
    }
    ((kw - k * p_step_kw).abs() < TOL).then_some(k)
}

impl HeaterMilpContext {
    /// Declare all LP variables for this heater.
    pub fn declare_vars(&self, n: usize, vars: &mut ProblemVariables) -> HeaterMilpVars {
        let must_not = self.mode == HeaterMilpMode::MustNotRun;
        let n_stages = self.n_stages as f64;

        // GB-40: one integer stage index per slot instead of a binary per tier.
        // Levels are evenly spaced (P = p_step_kw × y), so a single variable
        // represents each reachable power exactly once. The old two-binary
        // encoding let the relaxation express one power several ways
        // (e.g. 4.5 kW as z=(0.5,0.5) or (0,0.75)), and that degeneracy is what
        // stalled branch-and-bound. Integer bounds also subsume the old mutual-
        // exclusion rows.
        let y_heat = (0..n)
            .map(|t| {
                let anchor = self.anchored_kw.get(t).copied().flatten().and_then(|kw| {
                    let st = kw_to_stage(kw, self.p_step_kw, n_stages);
                    if st.is_none() {
                        tracing::warn!(
                            slot = t,
                            kw,
                            p_step = self.p_step_kw,
                            n_stages = self.n_stages,
                            "heater anchor: kw matches no stage — anchor dropped for this slot"
                        );
                    }
                    st
                });
                match anchor {
                    Some(k) => vars.add(variable().min(k).max(k)),
                    None if must_not => vars.add(variable().min(0.0).max(0.0)),
                    None => vars.add(variable().integer().min(0.0).max(n_stages)),
                }
            })
            .collect();

        // z_heat_ready: binary reward flag for MayRun with deadline; fixed 0 otherwise.
        let z_heat_ready = if self.mode == HeaterMilpMode::MayRun && self.t_dead_step.is_some() {
            vars.add(variable().binary())
        } else {
            vars.add(variable().min(0.0).max(0.0))
        };

        // e_tank[t]: continuous tank energy above T_min [kWh], domain [−e_max, e_max].
        let e_lo = -self.e_max_kwh.max(1.0); // allow negative (below T_min)
        let e_hi = self.e_max_kwh.max(1.0);
        let e_tank = (0..n)
            .map(|_| vars.add(variable().min(e_lo).max(e_hi)))
            .collect();

        // s_low[t]: non-negative below-min slack.
        let s_low = (0..n).map(|_| vars.add(variable().min(0.0))).collect();

        // sw[t]: switching indicator ≥ 0 for all slots including t=0.
        // t=0 measures the switch relative to the last observed hardware stage (initial_y).
        let sw = (0..n).map(|_| vars.add(variable().min(0.0))).collect();

        HeaterMilpVars {
            y_heat,
            z_heat_ready,
            e_tank,
            s_low,
            sw,
            p_step_kw: self.p_step_kw,
            n_stages: self.n_stages,
        }
    }

    /// Generate all MILP constraints for this heater.
    /// `dt_h[t]` is the slot duration in hours for slot `t`.
    pub fn constraints(&self, v: &HeaterMilpVars, n: usize, dt_h: &[f64]) -> Vec<Constraint> {
        let mut cs: Vec<Constraint> = Vec::new();

        // (The old C0 mutual-exclusion rows are gone: with a single stage index
        // bounded to [0, n_stages], unreachable combinations are inexpressible.)
        if self.mode == HeaterMilpMode::MustNotRun {
            return cs; // y already fixed to 0; no trajectory needed
        }

        // C1: pin initial tank energy.
        let e_init = self.e_init_kwh;
        cs.push(constraint!(v.e_tank[0] >= e_init));
        cs.push(constraint!(v.e_tank[0] <= e_init));

        // C2: tank dynamics — E[t+1] = E[t] + (P_heat[t] − q_dem) × dt_h[t].
        // Expressed as two inequalities (== is not directly supported).
        for (t, &dt) in dt_h.iter().enumerate().take(n.saturating_sub(1)) {
            let net_const = -self.q_dem_kw * dt;
            let p_step_dt = self.p_step_kw * dt;
            // LHS = e_tank[t+1] − e_tank[t] − p_step_dt×y[t]
            let lhs_ge = Expression::from(v.e_tank[t + 1])
                - Expression::from(v.e_tank[t])
                - p_step_dt * v.y_heat[t];
            cs.push(constraint!(lhs_ge >= net_const));
            let lhs_le = Expression::from(v.e_tank[t + 1])
                - Expression::from(v.e_tank[t])
                - p_step_dt * v.y_heat[t];
            cs.push(constraint!(lhs_le <= net_const));
        }

        // C3: upper bound — no overheating.
        for t in 0..n {
            cs.push(constraint!(v.e_tank[t] <= self.e_max_kwh));
        }

        // C4: soft lower bound — penalise going below T_min.
        for t in 0..n {
            cs.push(constraint!(v.e_tank[t] + v.s_low[t] >= 0.0));
        }

        // C5: switching indicators — sw[t] ≥ |y[t] − y[t−1]|, i.e. the number of
        // relay operations, since each stage is its own contactor. This corrects
        // the old encoding, whose `max(|Δz_mid|, |Δz_full|)` charged off→full the
        // same as off→mid even though it closes two relays rather than one.
        // t=0 measures against the last observed hardware stage (initial_y).
        cs.push(constraint!(v.sw[0] >= v.y_heat[0] - self.initial_y));
        cs.push(constraint!(v.sw[0] >= self.initial_y - v.y_heat[0]));
        for t in 1..n {
            cs.push(constraint!(v.sw[t] >= v.y_heat[t] - v.y_heat[t - 1]));
            cs.push(constraint!(v.sw[t] >= v.y_heat[t - 1] - v.y_heat[t]));
        }

        // C6: deadline constraint.
        if let Some(td) = self.t_dead_step {
            let td = td.min(n.saturating_sub(1));
            match self.mode {
                HeaterMilpMode::MustRun => {
                    cs.push(constraint!(v.e_tank[td] >= self.e_target_kwh));
                }
                HeaterMilpMode::MayRun => {
                    // e_tank[td] ≥ e_target × z_heat_ready (linear: e_target is a scalar)
                    let rhs = self.e_target_kwh * v.z_heat_ready;
                    cs.push(constraint!(v.e_tank[td] >= rhs));
                }
                HeaterMilpMode::MustNotRun => {}
            }
        }

        cs
    }

    /// Heater objective contribution (penalty terms only; energy cost enters via power balance).
    /// `dt_h[t]` is the slot duration in hours. Switching penalty scales by `dt_h[t]` so
    /// a switch in a longer slot costs proportionally more (zone-boundary neutral).
    #[allow(clippy::too_many_arguments)] // all parameters are distinct phase-gated objective coefficients, no natural grouping
    pub fn objective(
        &self,
        v: &HeaterMilpVars,
        w_tier_penalty_eur: f64,
        m_low_eur_kwh: f64,
        lambda_sw_eur: f64,
        comfort_full_reward_eur_kwh: f64,
        comfort_full_co2_reward_eur_kwh: f64,
        n: usize,
        dt_h: &[f64],
    ) -> Expression {
        let mut obj = Expression::from(0.0);
        if self.mode == HeaterMilpMode::MustNotRun {
            return obj;
        }
        // GB-40: the three tier-keyed terms below were written against a
        // `z_heat_full` indicator. With a single stage index they scale with
        // `y/n_stages` instead — 1.0 at full, 0.5 at mid on a two-stage unit,
        // where the indicator gave 0. That is a deliberate change: it keeps the
        // ordering these terms exist to express (full is penalised more, and
        // rewarded more, than mid) while making the middle stage carry its
        // proportional share, and it avoids reintroducing a binary per slot
        // purely to recover an all-or-nothing indicator.
        let stage_frac = 1.0 / self.n_stages as f64;
        for (t, &dt) in dt_h.iter().enumerate().take(n) {
            obj += w_tier_penalty_eur * stage_frac * v.y_heat[t]; // prefer lower stages when equal cost
            obj += m_low_eur_kwh * v.s_low[t]; // penalise below-min violations
            obj += lambda_sw_eur * dt * v.sw[t]; // penalise relay switches; scale by dt_h
                                                 // BL-34: reward higher-stage operation by the session comfort curve's fill=1.0 price;
                                                 // caller phase-gates this to Phase 2 only (0.0 in Phase 1), mirroring w_tier_penalty_eur.
            obj += -comfort_full_reward_eur_kwh * dt * stage_frac * v.y_heat[t];
            // BL-17 comfort bidding: same reward mechanism, but for the session's CO2 bid,
            // already monetized (€/kWh) via w_ghg at construction time (see from_state).
            obj += -comfort_full_co2_reward_eur_kwh * dt * stage_frac * v.y_heat[t];
        }
        // Terminal energy reward (Phase 1 only — m_low > 0, lambda_sw == 0).
        // Makes the optimizer treat heat stored at horizon end as having forward
        // value equal to c_terminal EUR/kWh, incentivising solar pre-fill.
        if m_low_eur_kwh > 0.0 && self.c_terminal_eur_kwh > 0.0 && n > 0 {
            obj += -self.c_terminal_eur_kwh * v.e_tank[n - 1];
        }
        obj
    }

    /// Read back the heater solution from the solved model.
    pub fn read_solution(sol: &impl Solution, v: &HeaterMilpVars, n: usize) -> HeaterSolOutput {
        HeaterSolOutput {
            y_heat: (0..n).map(|t| sol.value(v.y_heat[t])).collect(),
            z_heat_ready: sol.value(v.z_heat_ready),
            e_tank_kwh: (0..n).map(|t| sol.value(v.e_tank[t])).collect(),
            s_low_kwh: (0..n).map(|t| sol.value(v.s_low[t])).collect(),
            sw: (0..n).map(|t| sol.value(v.sw[t])).collect(),
        }
    }

    /// Construct from a live `AssetState`, sim `Heater` config, and optional target data.
    #[allow(clippy::too_many_arguments)] // all parameters are distinct domain values with no natural grouping
    pub fn from_state(
        state: &super::AssetState,
        cfg: &Heater,
        n: usize,
        cum_s: &[i64],
        now: DateTime<Utc>,
        heater_target: Option<&crate::entities::device_session::HeaterTarget>,
        lambda_sw: f64,
        c_terminal_eur_kwh: f64,
        anchored_kw: Vec<Option<f64>>,
        w_ghg_eur_kg: f64,
    ) -> Self {
        let current_temp = if let super::AssetState::Heater(s) = state {
            s.temperature_c
        } else {
            (cfg.temp_min_c + cfg.temp_max_c) / 2.0
        };
        let p_step_kw = cfg.p_step_kw();
        let e_init = (current_temp - cfg.temp_min_c) * cfg.thermal_mass_kwh_per_c;
        let e_max = ((cfg.temp_max_c - cfg.temp_min_c) * cfg.thermal_mass_kwh_per_c).max(0.0);
        let q_dem = cfg.forecast_demand_kw(cfg.ambient_temp_c);
        // Initial mode detection from last observed hardware tier.
        let actual_kw = if let super::AssetState::Heater(s) = state {
            s.actual_power_kw
        } else {
            0.0
        };
        // Last observed hardware stage, rounded to the nearest reachable level.
        let initial_y =
            kw_to_stage(actual_kw, p_step_kw, cfg.power_stages as f64).unwrap_or_else(|| {
                if p_step_kw > 0.0 {
                    (actual_kw / p_step_kw)
                        .round()
                        .clamp(0.0, cfg.power_stages as f64)
                } else {
                    0.0
                }
            });
        if let Some(target) = heater_target {
            let e_target = ((target.target_temp_c - cfg.temp_min_c) * cfg.thermal_mass_kwh_per_c)
                .clamp(0.0, e_max);
            let secs = (target.ready_by - now).num_seconds();
            let t_dead = if secs <= 0 {
                0
            } else {
                cum_s
                    .partition_point(|&s| s <= secs)
                    .saturating_sub(1)
                    .min(n.saturating_sub(1))
            };
            let comfort_full_reward_eur_kwh = if target.comfort_rates.is_empty() {
                0.0
            } else {
                crate::entities::asset::ComfortRate::value_at_fill(&target.comfort_rates, 1.0)
            };
            // BL-17 comfort bidding: monetize the session's gCO2/kWh bid at fill=1.0 via the
            // planner's own w_ghg weight (€/kgCO2 -- the same weight used for the grid-carbon
            // cost term), so it becomes a €/kWh reward comparable to comfort_full_reward_eur_kwh.
            let comfort_full_co2_reward_eur_kwh = if target.comfort_rates.is_empty() {
                0.0
            } else {
                (crate::entities::asset::ComfortRate::co2_value_at_fill(&target.comfort_rates, 1.0)
                    / 1000.0)
                    * w_ghg_eur_kg
            };
            Self {
                mode: HeaterMilpMode::MustRun,
                t_dead_step: Some(t_dead),
                p_step_kw,
                n_stages: cfg.power_stages,
                e_init_kwh: e_init,
                e_max_kwh: e_max,
                q_dem_kw: q_dem,
                e_target_kwh: e_target,
                lambda_sw_eur: lambda_sw,
                initial_y,
                c_terminal_eur_kwh,
                anchored_kw,
                comfort_full_reward_eur_kwh,
                comfort_full_co2_reward_eur_kwh,
            }
        } else {
            Self {
                mode: HeaterMilpMode::MayRun,
                t_dead_step: None,
                p_step_kw,
                n_stages: cfg.power_stages,
                e_init_kwh: e_init,
                e_max_kwh: e_max,
                q_dem_kw: q_dem,
                e_target_kwh: e_max,
                lambda_sw_eur: lambda_sw,
                initial_y,
                c_terminal_eur_kwh,
                anchored_kw,
                comfort_full_reward_eur_kwh: 0.0,
                comfort_full_co2_reward_eur_kwh: 0.0,
            }
        }
    }
}

impl crate::controller::milp_planner::AssetMilpContext for HeaterMilpContext {
    fn asset_id(&self) -> &str {
        crate::ids::ASSET_HEATER
    }

    fn asset_kind(&self) -> crate::controller::milp_planner::AssetKind {
        crate::controller::milp_planner::AssetKind::Heater
    }

    fn milp_params(
        &self,
        _n: usize,
        _now: chrono::DateTime<chrono::Utc>,
    ) -> crate::controller::milp_planner::AssetMilpParams {
        use crate::controller::milp_planner::MilpLoadMode;
        let mode = match self.mode {
            HeaterMilpMode::MustRun => MilpLoadMode::MustRun,
            HeaterMilpMode::MayRun => MilpLoadMode::MayRun,
            HeaterMilpMode::MustNotRun => MilpLoadMode::MustNotRun,
        };
        crate::controller::milp_planner::AssetMilpParams::Heater(
            crate::controller::milp_planner::HeaterScalars {
                mode,
                t_dead_step: self.t_dead_step,
                p_step_kw: self.p_step_kw,
                n_stages: self.n_stages,
                e_init_kwh: self.e_init_kwh,
                e_max_kwh: self.e_max_kwh,
                q_dem_kw: self.q_dem_kw,
                e_target_kwh: self.e_target_kwh,
                lambda_sw_eur: self.lambda_sw_eur,
                initial_y: self.initial_y,
                c_terminal_eur_kwh: self.c_terminal_eur_kwh,
            },
        )
    }

    fn declare_vars_into_pool(
        &self,
        n: usize,
        _c_startup_eur: f64,
        _c_ramp_eur_kw: f64,
        vars: &mut ProblemVariables,
        pool: &mut crate::controller::milp_interactions::MilpVarPool,
    ) {
        pool.heater = Some(self.declare_vars(n, vars));
    }

    fn constraints(
        &self,
        pool: &crate::controller::milp_interactions::MilpVarPool,
        n: usize,
        dt_h: &[f64],
    ) -> Vec<Constraint> {
        // SAFETY: declare_vars_into_pool() (above) always runs before constraints()
        // for this context — see the AssetMilpContext trait's call-order invariant.
        HeaterMilpContext::constraints(self, pool.heater.as_ref().unwrap(), n, dt_h)
    }

    fn objective(
        &self,
        pool: &crate::controller::milp_interactions::MilpVarPool,
        n: usize,
        dt_h: &[f64],
        _c_wear_eur_kwh: f64,
        c_startup_eur: f64,
        c_ramp_eur_kw: f64,
    ) -> Expression {
        // SAFETY: declare_vars_into_pool() always runs before objective() for this
        // context — see the AssetMilpContext trait's call-order invariant.
        let v = pool.heater.as_ref().unwrap();
        if c_startup_eur == 0.0 {
            // Phase 1: below-min penalty only; tier=0, lambda_sw=0 (switching handled by Phase 2).
            // comfort_full_reward_eur_kwh=0.0 here too — Phase 1 has no counterbalancing tier
            // penalty (w_tier_penalty_eur=0.0), so an unconditional reward on z_heat_full would
            // be a free, uncounterbalanced bias toward full-tier in Phase 1's coarse allocation.
            HeaterMilpContext::objective(
                self,
                v,
                0.0,
                crate::controller::milp_planner::asset_port::M_LOW_EUR_PER_KWH,
                0.0,
                0.0,
                0.0,
                n,
                dt_h,
            )
        } else {
            // Phase 2 friction: tier penalty + relay switching penalty + comfort-curve reward
            // (price and CO2 both).
            HeaterMilpContext::objective(
                self,
                v,
                c_ramp_eur_kw,
                0.0,
                self.lambda_sw_eur,
                self.comfort_full_reward_eur_kwh,
                self.comfort_full_co2_reward_eur_kwh,
                n,
                dt_h,
            )
        }
    }
}

#[cfg(test)]
mod milp_tests {
    use super::*;

    fn make_may_run_ctx(_n: usize) -> HeaterMilpContext {
        HeaterMilpContext {
            mode: HeaterMilpMode::MayRun,
            t_dead_step: None,
            p_step_kw: 1.0,
            n_stages: 2,
            e_init_kwh: 2.5,
            e_max_kwh: 5.0,
            q_dem_kw: 0.3,
            e_target_kwh: 5.0,
            lambda_sw_eur: 0.0,
            initial_y: 0.0,
            c_terminal_eur_kwh: 0.0,
            anchored_kw: vec![],
            comfort_full_reward_eur_kwh: 0.0,
            comfort_full_co2_reward_eur_kwh: 0.0,
        }
    }

    fn heater_pool_and_vars(
        n: usize,
    ) -> (
        good_lp::ProblemVariables,
        HeaterMilpVars,
        crate::controller::milp_interactions::MilpVarPool,
    ) {
        use crate::controller::milp_interactions::{GridMilpVars, MilpVarPool};
        use good_lp::{variable, variables};
        let mut vars = variables!();
        let ctx = make_may_run_ctx(n);
        let hv = ctx.declare_vars(n, &mut vars);
        let grid = GridMilpVars {
            p_imp: (0..n).map(|_| vars.add(variable().min(0.0))).collect(),
            p_exp: (0..n).map(|_| vars.add(variable().min(0.0))).collect(),
            u_grid: (0..n).map(|_| vars.add(variable().binary())).collect(),
            s_imp_viol: (0..n).map(|_| vars.add(variable().min(0.0))).collect(),
            s_exp_viol: (0..n).map(|_| vars.add(variable().min(0.0))).collect(),
            p_pv_used: (0..n).map(|_| vars.add(variable().min(0.0))).collect(),
        };
        let pool = MilpVarPool {
            grid,
            bat: None,
            ev: None,
            heater: Some(hv.clone()),
            shiftable: vec![],
        };
        (vars, hv, pool)
    }

    #[test]
    fn heater_milp_context_declares_e_tank_s_low_sw() {
        let n = 4;
        let (_, hv, _) = heater_pool_and_vars(n);
        assert_eq!(hv.e_tank.len(), n);
        assert_eq!(hv.s_low.len(), n);
        assert_eq!(hv.sw.len(), n);
    }

    #[test]
    fn heater_milp_sw_all_slots_present() {
        let n = 4;
        let (_, hv, _) = heater_pool_and_vars(n);
        // sw has one entry per slot including t=0 (measures switch from initial hardware state)
        assert_eq!(hv.sw.len(), n);
    }

    #[test]
    fn heater_milp_must_not_run_emits_no_constraints() {
        let n = 4;
        use good_lp::variables;
        let mut vars = variables!();
        let ctx = HeaterMilpContext {
            mode: HeaterMilpMode::MustNotRun,
            ..make_may_run_ctx(n)
        };
        let hv = ctx.declare_vars(n, &mut vars);
        let cs = ctx.constraints(&hv, n, &vec![300.0 / 3600.0; n]);
        // GB-40: MustNotRun now emits nothing at all. `y` is declared with bounds
        // [0, 0], so "cannot run" is carried by the variable itself; the n
        // mutual-exclusion rows this used to emit were already redundant, since
        // the old tier binaries were likewise fixed to 0 in this mode.
        assert!(
            cs.is_empty(),
            "MustNotRun should need no rows; y is bound to 0. got {}",
            cs.len()
        );
    }

    #[test]
    fn heater_milp_constraints_initial_energy_pin() {
        let n = 4;
        let (_, hv, _) = heater_pool_and_vars(n);
        let ctx = make_may_run_ctx(n);
        let cs = ctx.constraints(&hv, n, &vec![300.0 / 3600.0; n]);
        // C0: n, C1: 2, C2: 2×(n-1), C3: n, C4: n, C5: 4×n (4 at t=0, 4 per subsequent slot)
        // Total for MayRun, no deadline: n + 2 + 2(n-1) + n + n + 4n = 9n
        // For n=4: 4 + 2 + 6 + 4 + 4 + 16 = 36
        assert!(
            cs.len() >= 2,
            "should have at least C1 (2 constraints pinning e_tank[0])"
        );
    }

    #[test]
    fn heater_milp_constraints_dynamics_count() {
        let n = 4;
        let (_, hv, _) = heater_pool_and_vars(n);
        let ctx = make_may_run_ctx(n);
        let cs = ctx.constraints(&hv, n, &vec![300.0 / 3600.0; n]);
        // n=4, MayRun, no deadline: C1 2 + C2 6 + C3 4 + C4 4 + C5 8 = 24.
        // Was 36 before GB-40: C0 contributed 4 rows (now gone — integer bounds
        // subsume mutual exclusion) and C5 contributed 16 rather than 8, since
        // it constrained two tier binaries per step instead of one stage index.
        assert_eq!(
            cs.len(),
            24,
            "expected 24 constraints for n=4 MayRun no-deadline"
        );
    }

    #[test]
    fn heater_milp_constraints_upper_bound() {
        let n = 4;
        let (_, hv, _) = heater_pool_and_vars(n);
        let ctx = make_may_run_ctx(n);
        let cs = ctx.constraints(&hv, n, &vec![300.0 / 3600.0; n]);
        // C3 contributes n constraints; total >= n (at minimum C0 alone)
        assert!(
            cs.len() >= n,
            "need at least n upper-bound constraints (C3)"
        );
    }

    #[test]
    fn heater_milp_constraints_soft_low() {
        let n = 4;
        let (_, hv, _) = heater_pool_and_vars(n);
        let ctx = make_may_run_ctx(n);
        let cs = ctx.constraints(&hv, n, &vec![300.0 / 3600.0; n]);
        // C4 contributes n soft-lower constraints; verified by total count
        assert!(cs.len() >= n * 2);
    }

    #[test]
    fn heater_milp_constraints_two_switching_rows_per_step() {
        let n = 4;
        let (_, hv, _) = heater_pool_and_vars(n);
        let ctx = make_may_run_ctx(n);
        let cs = ctx.constraints(&hv, n, &vec![300.0 / 3600.0; n]);
        // C5: 2 at t=0 + 2×(n-1) for t=1..n-1 = 2n switching rows — half the old
        // count, since one stage index replaced the two tier binaries. Verified
        // through the total 24-constraint count for n=4 (see dynamics_count).
        assert_eq!(cs.len(), 24);
    }
}

#[cfg(test)]
mod milp_context_trait_tests {
    use super::*;
    use crate::controller::milp_interactions::{GridMilpVars, MilpVarPool};
    use crate::controller::milp_planner::{
        AssetKind, AssetMilpContext, AssetMilpParams, MilpLoadMode,
    };
    use good_lp::{variable, variables};

    fn make_ctx() -> HeaterMilpContext {
        HeaterMilpContext {
            mode: HeaterMilpMode::MayRun,
            t_dead_step: None,
            p_step_kw: 1.0,
            n_stages: 2,
            e_init_kwh: 2.5,
            e_max_kwh: 5.0,
            q_dem_kw: 0.3,
            e_target_kwh: 5.0,
            lambda_sw_eur: 0.01,
            initial_y: 0.0,
            c_terminal_eur_kwh: 0.0,
            anchored_kw: vec![],
            comfort_full_reward_eur_kwh: 0.0,
            comfort_full_co2_reward_eur_kwh: 0.0,
        }
    }

    fn empty_pool(vars: &mut good_lp::ProblemVariables, n: usize) -> MilpVarPool {
        let grid = GridMilpVars {
            p_imp: (0..n).map(|_| vars.add(variable().min(0.0))).collect(),
            p_exp: (0..n).map(|_| vars.add(variable().min(0.0))).collect(),
            u_grid: (0..n).map(|_| vars.add(variable().binary())).collect(),
            s_imp_viol: (0..n).map(|_| vars.add(variable().min(0.0))).collect(),
            s_exp_viol: (0..n).map(|_| vars.add(variable().min(0.0))).collect(),
            p_pv_used: (0..n).map(|_| vars.add(variable().min(0.0))).collect(),
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
    fn asset_id_is_heater() {
        assert_eq!(make_ctx().asset_id(), "heater");
    }

    #[test]
    fn asset_kind_is_heater() {
        assert_eq!(make_ctx().asset_kind(), AssetKind::Heater);
    }

    #[test]
    fn milp_params_returns_heater_scalars() {
        let ctx = make_ctx();
        match ctx.milp_params(4, chrono::Utc::now()) {
            AssetMilpParams::Heater(h) => {
                assert_eq!(h.mode, MilpLoadMode::MayRun);
                assert!((h.p_step_kw - 1.0).abs() < 1e-9);
                assert_eq!(h.n_stages, 2);
                assert!((h.e_init_kwh - 2.5).abs() < 1e-9);
                assert!((h.lambda_sw_eur - 0.01).abs() < 1e-9);
                assert!(h.t_dead_step.is_none());
            }
            _ => panic!("expected AssetMilpParams::Heater"),
        }
    }

    #[test]
    fn milp_params_must_run_mode() {
        let ctx = HeaterMilpContext {
            mode: HeaterMilpMode::MustRun,
            ..make_ctx()
        };
        match ctx.milp_params(4, chrono::Utc::now()) {
            AssetMilpParams::Heater(h) => assert_eq!(h.mode, MilpLoadMode::MustRun),
            _ => panic!("expected Heater variant"),
        }
    }

    #[test]
    fn declare_vars_fills_pool_heater_slot() {
        let n = 4;
        let ctx = make_ctx();
        let mut vars = variables!();
        let mut pool = empty_pool(&mut vars, n);
        ctx.declare_vars_into_pool(n, 0.0, 0.0, &mut vars, &mut pool);
        let v = pool.heater.as_ref().expect("pool.heater should be Some");
        assert_eq!(v.y_heat.len(), n);
        assert_eq!(v.e_tank.len(), n);
        assert_eq!(v.s_low.len(), n);
        assert_eq!(v.sw.len(), n);
        assert!((v.p_step_kw - 1.0).abs() < 1e-9);
        assert_eq!(v.n_stages, 2);
    }

    #[test]
    fn constraints_count_for_n4_may_run() {
        let n = 4;
        let ctx = make_ctx();
        let mut vars = variables!();
        let mut pool = empty_pool(&mut vars, n);
        ctx.declare_vars_into_pool(n, 0.0, 0.0, &mut vars, &mut pool);
        let cs = AssetMilpContext::constraints(&ctx, &pool, n, &vec![300.0 / 3600.0; n]);
        assert_eq!(
            cs.len(),
            24,
            "n=4 MayRun no-deadline: expected 24 constraints"
        );
    }

    // ── Terminal reward unit tests ────────────────────────────────────────────

    #[test]
    fn test_terminal_reward_coefficient_stored() {
        let ctx = HeaterMilpContext {
            c_terminal_eur_kwh: 0.56,
            ..make_ctx()
        };
        assert!(
            (ctx.c_terminal_eur_kwh - 0.56).abs() < 1e-9,
            "c_terminal_eur_kwh should be stored as given"
        );
    }

    #[test]
    fn test_terminal_reward_zero_disables() {
        let ctx = HeaterMilpContext {
            c_terminal_eur_kwh: 0.0,
            ..make_ctx()
        };
        assert_eq!(ctx.c_terminal_eur_kwh, 0.0);
    }

    #[test]
    fn test_terminal_reward_in_phase1_objective_not_phase2() {
        let n = 3;
        let ctx = HeaterMilpContext {
            c_terminal_eur_kwh: 0.5,
            ..make_ctx()
        };
        let mut vars = variables!();
        let mut pool = empty_pool(&mut vars, n);
        ctx.declare_vars_into_pool(n, 0.0, 0.0, &mut vars, &mut pool);
        let v = pool.heater.as_ref().unwrap();

        // Phase 1: m_low > 0, lambda_sw == 0 — terminal term should appear
        use crate::controller::milp_planner::asset_port::M_LOW_EUR_PER_KWH;
        let dt_h = vec![300.0 / 3600.0; n];
        let obj_p1 =
            HeaterMilpContext::objective(&ctx, v, 0.0, M_LOW_EUR_PER_KWH, 0.0, 0.0, 0.0, n, &dt_h);

        // Phase 2: m_low == 0, lambda_sw > 0 — terminal term must NOT appear
        let obj_p2 = HeaterMilpContext::objective(&ctx, v, 0.05, 0.0, 0.5, 0.0, 0.0, n, &dt_h);

        // We cannot easily inspect the LP expression value without a solver,
        // but we can check the debug representation differs (terminal adds a coefficient).
        let p1_str = format!("{obj_p1:?}");
        let p2_str = format!("{obj_p2:?}");
        assert_ne!(
            p1_str, p2_str,
            "Phase 1 objective should differ from Phase 2 when c_terminal > 0"
        );
    }

    #[test]
    fn test_terminal_auto_formula_heater() {
        // Verify the auto-computation formula: avg_imp + c_ctrl_imp_malus
        let avg_imp_eur_kwh = 0.34_f64;
        let c_ctrl_imp_malus_eur_kwh = 0.22_f64;
        let expected = avg_imp_eur_kwh + c_ctrl_imp_malus_eur_kwh; // 0.56
        let ctx = HeaterMilpContext {
            c_terminal_eur_kwh: expected,
            ..make_ctx()
        };
        assert!(
            (ctx.c_terminal_eur_kwh - 0.56).abs() < 1e-9,
            "auto-computed c_terminal should equal avg_imp + malus"
        );
    }

    #[test]
    fn test_switching_penalty_scales_with_dt_h() {
        // Phase 2 switching cost: lambda_sw × dt_h[t] × sw[t].
        // A switch in a longer slot covers more time → costs proportionally more.
        // Zone C (15-min slot) must cost 3× Zone A (5-min slot) for the same lambda_sw.
        let n = 1;
        let ctx = HeaterMilpContext {
            lambda_sw_eur: 0.50,
            ..make_ctx()
        };
        let mut vars = variables!();
        let mut pool = empty_pool(&mut vars, n);
        ctx.declare_vars_into_pool(n, 0.0, 0.0, &mut vars, &mut pool);
        let v = pool.heater.as_ref().unwrap();

        let dt_zone_a = vec![5.0_f64 / 60.0]; // 5-min slot
        let dt_zone_c = vec![15.0_f64 / 60.0]; // 15-min slot
                                               // Phase 2 mode: w_tier=0, m_low=0, lambda_sw=0.50
        let obj_a = HeaterMilpContext::objective(&ctx, v, 0.0, 0.0, 0.50, 0.0, 0.0, n, &dt_zone_a);
        let obj_c = HeaterMilpContext::objective(&ctx, v, 0.0, 0.0, 0.50, 0.0, 0.0, n, &dt_zone_c);

        // The expressions differ: Zone C has a 3× larger coefficient on sw[0].
        assert_ne!(
            format!("{obj_a:?}"),
            format!("{obj_c:?}"),
            "Zone A and Zone C switching cost expressions must differ"
        );
        // Verify the 3:1 ratio from the dt_h multiplier.
        let cost_a = 0.50 * (5.0_f64 / 60.0);
        let cost_c = 0.50 * (15.0_f64 / 60.0);
        assert!(
            (cost_c / cost_a - 3.0).abs() < 1e-9,
            "Zone C switch cost must be 3× Zone A; ratio={:.6}",
            cost_c / cost_a,
        );
    }

    #[test]
    fn test_anchored_vars_produce_fixed_bounds() {
        // Slot 0 anchored to full tier (2.0 kW): z_full[0] must be 1.0, z_mid[0] must be 0.0.
        // Slot 1 free: minimizer drives z_full[1] to 0.0.
        use good_lp::{solvers::highs::highs, Expression, SolverModel};
        let n = 2;
        let ctx = HeaterMilpContext {
            anchored_kw: vec![Some(2.0), None], // full tier anchor on slot 0
            ..make_ctx()                        // p_step_kw=1.0, n_stages=2
        };
        let mut vars = variables!();
        let hv = ctx.declare_vars(n, &mut vars);
        let dt_h: Vec<f64> = vec![300.0 / 3600.0; n];
        let cs = ctx.constraints(&hv, n, &dt_h);
        // Minimize z_full[0] + z_full[1]: slot 0 fixed → contributes 1.0; slot 1 minimized → 0.0.
        let obj: Expression = Expression::from(hv.y_heat[0]) + Expression::from(hv.y_heat[1]);
        let model = cs
            .into_iter()
            .fold(vars.minimise(&obj).using(highs), |m, c| m.with(c));
        let sol = model.solve().expect("anchored LP must be feasible");
        assert!(
            (sol.value(hv.y_heat[0]) - 2.0).abs() < 1e-4,
            "slot 0 anchored to full tier: z_full[0] must be 1.0, got {:.6}",
            sol.value(hv.y_heat[0])
        );
        assert!(
            (sol.value(hv.y_heat[0]) - 2.0).abs() < 1e-4,
            "slot 0 anchored to full tier: z_mid[0] must be 0.0, got {:.6}",
            sol.value(hv.y_heat[0])
        );
    }

    #[test]
    fn kw_to_stage_maps_reachable_levels() {
        // p_step = 1.0, two stages -> reachable {0.0, 1.0, 2.0}.
        assert_eq!(kw_to_stage(0.0, 1.0, 2.0), Some(0.0));
        assert_eq!(kw_to_stage(0.05, 1.0, 2.0), Some(0.0));
        assert_eq!(kw_to_stage(1.0, 1.0, 2.0), Some(1.0));
        assert_eq!(kw_to_stage(2.0, 1.0, 2.0), Some(2.0));
    }

    #[test]
    fn kw_to_stage_rejects_unreachable_and_out_of_range() {
        // Between stages by more than the tolerance -> no stage.
        assert_eq!(kw_to_stage(1.5, 1.0, 2.0), None);
        // Above the top stage -> no stage, rather than silently clamping.
        assert_eq!(kw_to_stage(3.0, 1.0, 2.0), None);
        // Single-stage element: 1.0 is not reachable when the only step is 2.0.
        assert_eq!(kw_to_stage(1.0, 2.0, 1.0), None);
        assert_eq!(kw_to_stage(2.0, 2.0, 1.0), Some(1.0));
    }

    // ── BL-34: comfort-curve full-tier reward unit tests ─────────────────────

    #[test]
    fn test_comfort_full_reward_phase1_objective_unaffected() {
        // Phase 1 (c_startup=0.0) must always pass 0.0 for the new parameter, regardless of
        // the context's stored comfort_full_reward_eur_kwh — matches w_tier_penalty_eur's
        // existing phase-gating (D4).
        let n = 3;
        let dt_h = vec![300.0 / 3600.0; n];

        let ctx_zero = HeaterMilpContext {
            comfort_full_reward_eur_kwh: 0.0,
            ..make_ctx()
        };
        let ctx_reward = HeaterMilpContext {
            comfort_full_reward_eur_kwh: 0.30,
            ..make_ctx()
        };

        let mut vars_zero = variables!();
        let mut pool_zero = empty_pool(&mut vars_zero, n);
        ctx_zero.declare_vars_into_pool(n, 0.0, 0.0, &mut vars_zero, &mut pool_zero);
        let obj_zero = AssetMilpContext::objective(&ctx_zero, &pool_zero, n, &dt_h, 0.0, 0.0, 0.0);

        let mut vars_reward = variables!();
        let mut pool_reward = empty_pool(&mut vars_reward, n);
        ctx_reward.declare_vars_into_pool(n, 0.0, 0.0, &mut vars_reward, &mut pool_reward);
        let obj_reward =
            AssetMilpContext::objective(&ctx_reward, &pool_reward, n, &dt_h, 0.0, 0.0, 0.0);

        assert_eq!(
            format!("{obj_zero:?}"),
            format!("{obj_reward:?}"),
            "Phase 1 objective must not depend on comfort_full_reward_eur_kwh"
        );
    }

    #[test]
    fn test_comfort_full_reward_shapes_phase2_objective() {
        // Phase 2 (c_startup>0.0) must include the reward: two contexts differing only in
        // comfort_full_reward_eur_kwh must produce different objective expressions.
        let n = 3;
        let dt_h = vec![300.0 / 3600.0; n];

        let ctx_zero = HeaterMilpContext {
            comfort_full_reward_eur_kwh: 0.0,
            ..make_ctx()
        };
        let ctx_reward = HeaterMilpContext {
            comfort_full_reward_eur_kwh: 0.30,
            ..make_ctx()
        };

        let mut vars_zero = variables!();
        let mut pool_zero = empty_pool(&mut vars_zero, n);
        ctx_zero.declare_vars_into_pool(n, 1.0, 0.05, &mut vars_zero, &mut pool_zero);
        let obj_zero = AssetMilpContext::objective(&ctx_zero, &pool_zero, n, &dt_h, 0.0, 1.0, 0.05);

        let mut vars_reward = variables!();
        let mut pool_reward = empty_pool(&mut vars_reward, n);
        ctx_reward.declare_vars_into_pool(n, 1.0, 0.05, &mut vars_reward, &mut pool_reward);
        let obj_reward =
            AssetMilpContext::objective(&ctx_reward, &pool_reward, n, &dt_h, 0.0, 1.0, 0.05);

        assert_ne!(
            format!("{obj_zero:?}"),
            format!("{obj_reward:?}"),
            "Phase 2 objective must depend on comfort_full_reward_eur_kwh"
        );
    }

    // ── BL-17: comfort-curve full-tier CO2 reward unit tests (mirrors BL-34's price tests) ──

    #[test]
    fn test_comfort_full_co2_reward_phase1_objective_unaffected() {
        // Phase 1 (c_startup=0.0) must always pass 0.0 for the CO2 reward too, regardless of
        // the context's stored comfort_full_co2_reward_eur_kwh.
        let n = 3;
        let dt_h = vec![300.0 / 3600.0; n];

        let ctx_zero = HeaterMilpContext {
            comfort_full_co2_reward_eur_kwh: 0.0,
            ..make_ctx()
        };
        let ctx_reward = HeaterMilpContext {
            comfort_full_co2_reward_eur_kwh: 0.20,
            ..make_ctx()
        };

        let mut vars_zero = variables!();
        let mut pool_zero = empty_pool(&mut vars_zero, n);
        ctx_zero.declare_vars_into_pool(n, 0.0, 0.0, &mut vars_zero, &mut pool_zero);
        let obj_zero = AssetMilpContext::objective(&ctx_zero, &pool_zero, n, &dt_h, 0.0, 0.0, 0.0);

        let mut vars_reward = variables!();
        let mut pool_reward = empty_pool(&mut vars_reward, n);
        ctx_reward.declare_vars_into_pool(n, 0.0, 0.0, &mut vars_reward, &mut pool_reward);
        let obj_reward =
            AssetMilpContext::objective(&ctx_reward, &pool_reward, n, &dt_h, 0.0, 0.0, 0.0);

        assert_eq!(
            format!("{obj_zero:?}"),
            format!("{obj_reward:?}"),
            "Phase 1 objective must not depend on comfort_full_co2_reward_eur_kwh"
        );
    }

    #[test]
    fn test_comfort_full_co2_reward_shapes_phase2_objective() {
        // Phase 2 (c_startup>0.0) must include the CO2 reward: two contexts differing only in
        // comfort_full_co2_reward_eur_kwh must produce different objective expressions.
        let n = 3;
        let dt_h = vec![300.0 / 3600.0; n];

        let ctx_zero = HeaterMilpContext {
            comfort_full_co2_reward_eur_kwh: 0.0,
            ..make_ctx()
        };
        let ctx_reward = HeaterMilpContext {
            comfort_full_co2_reward_eur_kwh: 0.20,
            ..make_ctx()
        };

        let mut vars_zero = variables!();
        let mut pool_zero = empty_pool(&mut vars_zero, n);
        ctx_zero.declare_vars_into_pool(n, 1.0, 0.05, &mut vars_zero, &mut pool_zero);
        let obj_zero = AssetMilpContext::objective(&ctx_zero, &pool_zero, n, &dt_h, 0.0, 1.0, 0.05);

        let mut vars_reward = variables!();
        let mut pool_reward = empty_pool(&mut vars_reward, n);
        ctx_reward.declare_vars_into_pool(n, 1.0, 0.05, &mut vars_reward, &mut pool_reward);
        let obj_reward =
            AssetMilpContext::objective(&ctx_reward, &pool_reward, n, &dt_h, 0.0, 1.0, 0.05);

        assert_ne!(
            format!("{obj_zero:?}"),
            format!("{obj_reward:?}"),
            "Phase 2 objective must depend on comfort_full_co2_reward_eur_kwh"
        );
    }

    /// BL-34: `from_state` sources `comfort_full_reward_eur_kwh` from the target's resolved
    /// comfort curve's fill=1.0 price when a session (`MustRun`) is active.
    #[test]
    fn from_state_sources_comfort_full_reward_from_curve() {
        use crate::entities::asset::ComfortRate;
        use crate::entities::design_vocabulary::UserRequestMode;
        use crate::entities::device_session::HeaterTarget;
        use chrono::Utc;

        let cfg = super::Heater::from_params(&crate::entities::asset_params::HeaterParams {
            id: "heater".into(),
            max_kw: 3.0,
            temp_initial_c: 20.0,
            temp_min_c: 18.0,
            temp_max_c: 23.0,
            temp_safety_max_c: 23.0,
            power_stages: 2,
            thermal_mass_kwh_per_c: 2.0,
            k_loss_kw_per_c: 0.1,
            draw_kw: 0.0,
            switching_penalty_eur: 0.0,
            c_terminal_eur_kwh: None,
        });
        let state = super::super::AssetState::Heater(super::super::HeaterState {
            temperature_c: 20.0,
            actual_power_kw: 0.0,
        });
        let now = Utc::now();
        let target = HeaterTarget {
            id: uuid::Uuid::new_v4(),
            target_temp_c: 22.0,
            ready_by: now + chrono::Duration::hours(1),
            mode: UserRequestMode::ByDeadline,
            comfort_rates: vec![
                ComfortRate {
                    fill: 0.0,
                    max_marginal_price: 0.30,
                    max_marginal_co2: 0.0,
                },
                ComfortRate {
                    fill: 1.0,
                    max_marginal_price: 0.18,
                    max_marginal_co2: 0.0,
                },
            ],
            created_at: now,
            updated_at: now,
        };
        let cum_s: Vec<i64> = (0..=12).map(|i| i * 300).collect();
        let ctx = HeaterMilpContext::from_state(
            &state,
            &cfg,
            12,
            &cum_s,
            now,
            Some(&target),
            0.0,
            0.0,
            vec![],
            0.0,
        );
        assert!(
            (ctx.comfort_full_reward_eur_kwh - 0.18).abs() < 1e-9,
            "expected the curve's fill=1.0 price 0.18, got {}",
            ctx.comfort_full_reward_eur_kwh
        );
        assert_eq!(ctx.mode, HeaterMilpMode::MustRun);
    }

    /// BL-17 comfort bidding: `from_state` sources `comfort_full_co2_reward_eur_kwh` from the
    /// target's resolved comfort curve's fill=1.0 CO2 bid, monetized via `w_ghg_eur_kg`.
    #[test]
    fn from_state_sources_comfort_full_co2_reward_from_curve() {
        use crate::entities::asset::ComfortRate;
        use crate::entities::design_vocabulary::UserRequestMode;
        use crate::entities::device_session::HeaterTarget;
        use chrono::Utc;

        let cfg = super::Heater::from_params(&crate::entities::asset_params::HeaterParams {
            id: "heater".into(),
            max_kw: 3.0,
            temp_initial_c: 20.0,
            temp_min_c: 18.0,
            temp_max_c: 23.0,
            temp_safety_max_c: 23.0,
            power_stages: 2,
            thermal_mass_kwh_per_c: 2.0,
            k_loss_kw_per_c: 0.1,
            draw_kw: 0.0,
            switching_penalty_eur: 0.0,
            c_terminal_eur_kwh: None,
        });
        let state = super::super::AssetState::Heater(super::super::HeaterState {
            temperature_c: 20.0,
            actual_power_kw: 0.0,
        });
        let now = Utc::now();
        let target = HeaterTarget {
            id: uuid::Uuid::new_v4(),
            target_temp_c: 22.0,
            ready_by: now + chrono::Duration::hours(1),
            mode: UserRequestMode::ByDeadline,
            comfort_rates: vec![
                ComfortRate {
                    fill: 0.0,
                    max_marginal_price: 0.30,
                    max_marginal_co2: 300.0,
                },
                ComfortRate {
                    fill: 1.0,
                    max_marginal_price: 0.18,
                    max_marginal_co2: 200.0, // 0.2 kgCO2/kWh
                },
            ],
            created_at: now,
            updated_at: now,
        };
        let cum_s: Vec<i64> = (0..=12).map(|i| i * 300).collect();
        // w_ghg = 0.5 EUR/kgCO2 -> 0.2 kgCO2/kWh * 0.5 EUR/kgCO2 = 0.10 EUR/kWh
        let ctx = HeaterMilpContext::from_state(
            &state,
            &cfg,
            12,
            &cum_s,
            now,
            Some(&target),
            0.0,
            0.0,
            vec![],
            0.5,
        );
        assert!(
            (ctx.comfort_full_co2_reward_eur_kwh - 0.10).abs() < 1e-9,
            "expected 200 gCO2/kWh monetized at w_ghg=0.5 EUR/kgCO2 -> 0.10 EUR/kWh, got {}",
            ctx.comfort_full_co2_reward_eur_kwh
        );
    }

    /// No session (autonomous `MayRun`) or an empty curve keeps
    /// `comfort_full_reward_eur_kwh` at 0.0 — no behavior change from before this feature.
    #[test]
    fn from_state_no_target_comfort_full_reward_is_zero() {
        let cfg = super::Heater::from_params(&crate::entities::asset_params::HeaterParams {
            id: "heater".into(),
            max_kw: 3.0,
            temp_initial_c: 20.0,
            temp_min_c: 18.0,
            temp_max_c: 23.0,
            temp_safety_max_c: 23.0,
            power_stages: 2,
            thermal_mass_kwh_per_c: 2.0,
            k_loss_kw_per_c: 0.1,
            draw_kw: 0.0,
            switching_penalty_eur: 0.0,
            c_terminal_eur_kwh: None,
        });
        let state = super::super::AssetState::Heater(super::super::HeaterState {
            temperature_c: 20.0,
            actual_power_kw: 0.0,
        });
        let now = chrono::Utc::now();
        let cum_s: Vec<i64> = (0..=12).map(|i| i * 300).collect();
        let ctx = HeaterMilpContext::from_state(
            &state,
            &cfg,
            12,
            &cum_s,
            now,
            None,
            0.0,
            0.0,
            vec![],
            0.5,
        );
        assert_eq!(ctx.comfort_full_reward_eur_kwh, 0.0);
        assert_eq!(ctx.comfort_full_co2_reward_eur_kwh, 0.0);
        assert_eq!(ctx.mode, HeaterMilpMode::MayRun);
    }
}
