//! The EV charger as the planner sees it: its variables in one solve and every question the
//! solve asks of them (its term in the site balance, its constraints, its cost and friction, its
//! warm start and its readback), plus what it reports about its own plan: the planned state of
//! charge, a firm promise it cannot keep, a budget too small for its target, and the session it
//! is serving. The model maths lives in `ev_milp.rs`; this module only answers the planner's
//! port. It never reads another asset's variables.

use chrono::{DateTime, Utc};
use good_lp::{Constraint, Expression, ProblemVariables, Solution, Variable};

use crate::controller::milp_planner::asset_port::{
    ev_future_state_at, EvMilpContext, EvMilpVars, EvSolOutput,
};
use crate::controller::milp_planner::{
    AllocationBasis, AssetMilpContext, AssetSolution, AssetSolutionDetail, DeclareMode,
    DeclaredAsset, FlexibilityWindow, ObjectivePart, OnIndicator,
};
use crate::entities::plan::{PlanWarning, WarningKind, WarningSeverity};

/// One EV's variables in one solve.
struct DeclaredEv<'a> {
    ctx: &'a EvMilpContext,
    v: EvMilpVars,
}

/// The EV's own solution readback, which only an EV solution carries.
fn own_readback(own: &AssetSolution) -> &EvSolOutput {
    match &own.detail {
        AssetSolutionDetail::Ev(sol) => sol,
        other => unreachable!("an EV reads its own solution, got {other:?}"),
    }
}

impl EvMilpContext {
    /// One warning per obligation whose target is above the charge limit: the plan holds it to
    /// the limit (`reachable_target_frac`), and the user learns why the car will not reach the
    /// number they asked for. Known before the solve (R-140).
    fn targets_above_the_charge_limit(&self) -> Vec<PlanWarning> {
        let limit_frac = self.soc_ceiling();
        self.obligations
            .iter()
            .filter(|ob| ob.target_soc_frac > limit_frac + 1e-9)
            .map(|ob| {
                let whose = match ob.session_id {
                    Some(id) => format!(" for request {id}"),
                    None => String::new(),
                };
                PlanWarning {
                    severity: WarningSeverity::Warning,
                    kind: WarningKind::TargetAboveLimit,
                    message: format!(
                        "EV target{whose} of {:.0} % is above the charge limit of {:.0} % \
                         — the plan charges to {:.0} %",
                        ob.target_soc_frac * 100.0,
                        limit_frac * 100.0,
                        limit_frac * 100.0
                    ),
                    suggested_action: Some(
                        "raise the charge limit or lower the target".to_string(),
                    ),
                }
            })
            .collect()
    }

    /// A firm obligation the plan could not meet: the window is too short, the car is
    /// away for too much of it, or the charger cannot deliver it in time.
    ///
    /// `ev-comfort-piecewise-core` narrowed this to one meaning: a **firm** request whose
    /// guaranteed energy the plan cannot deliver in the window it has. It deliberately does
    /// not fire for a soft request that charges less than its target: under the comfort curve
    /// a soft target is a preference priced per kWh, so charging less because the bid stopped
    /// covering the cost is the model working as asked (GB-41).
    ///
    /// R-93: the gap is the solved shortfall slack, not a comparison re-derived here, and the
    /// slack is per obligation, so the warning can say which request fell short.
    fn firm_shortfall(&self, sol: &EvSolOutput) -> Option<PlanWarning> {
        let (idx, short) = sol
            .shortfall_kwh
            .iter()
            .enumerate()
            .filter(|(_, &kwh)| kwh > 1e-3)
            .max_by(|a, b| a.1.total_cmp(b.1))
            .map(|(i, &kwh)| (i, kwh))?;
        let ob = self.obligations.get(idx)?;
        let required = self.required_kwh(ob);
        let delivered = (required - short).max(0.0);
        let whose = match ob.session_id {
            Some(id) => format!(" for request {id}"),
            None => String::new(),
        };
        Some(PlanWarning {
            severity: WarningSeverity::Warning,
            kind: WarningKind::EvCoreEnergyUnmet,
            message: format!(
                "EV charging{whose} falls {short:.1} kWh short of its guaranteed {required:.1} kWh \
                 before the deadline — the plan delivers {delivered:.1} kWh, which is all the \
                 available window allows"
            ),
            suggested_action: Some(
                "move the deadline later, lower the target, or check whether the car is predicted \
                 away for part of the window"
                    .to_string(),
            ),
        })
    }
}

impl DeclaredAsset for DeclaredEv<'_> {
    fn asset_id(&self) -> &str {
        &self.ctx.asset_id
    }

    fn net_power_expr(&self, t: usize) -> Expression {
        Expression::from(self.v.p_ev[t])
    }

    fn constraints(&self, n: usize, dt_h: &[f64]) -> Vec<Constraint> {
        self.ctx.constraints(&self.v, n, dt_h)
    }

    /// Cost: the service reward on charged energy. Friction: startups and ramps, with the
    /// service reward off. The lateness tie-break and the two integrity slacks are in both.
    fn objective(&self, part: ObjectivePart<'_>, n: usize, dt_h: &[f64]) -> Expression {
        let (startup_eur, ramp_eur_kw, w_services) = match part {
            ObjectivePart::Cost(_) => (0.0, 0.0, 1.0),
            ObjectivePart::Friction => (
                self.ctx.friction.startup_eur,
                self.ctx.friction.ramp_eur_kw,
                0.0,
            ),
        };
        self.ctx
            .objective(&self.v, startup_eur, ramp_eur_kw, w_services, n, dt_h)
    }

    fn warm_start(&self, own: &AssetSolution, n: usize) -> Vec<(Variable, f64)> {
        let v = &self.v;
        let p1 = own_readback(own);
        let mut iv = Vec::new();
        for t in 0..n {
            iv.push((v.p_ev[t], p1.p_ev_kw[t].max(0.0)));
            iv.push((v.z_ev_on[t], p1.z_ev_on[t]));
        }
        for i in 0..v.delta_ev.len() {
            let t = i + 1;
            let delta = (p1.z_ev_on[t] - p1.z_ev_on[i]).max(0.0);
            iv.push((v.delta_ev[i], delta));
        }
        for i in 0..v.delta_ev_ramp.len() {
            let t = i + 1;
            iv.push((v.delta_ev_ramp[i], (p1.p_ev_kw[t] - p1.p_ev_kw[i]).abs()));
        }
        iv.push((v.e_ev_extra, p1.e_ev_extra_kwh.max(0.0)));
        iv
    }

    fn read(&self, solution: &dyn Solution, n: usize) -> AssetSolution {
        let sol = EvMilpContext::read_solution(solution, &self.v, n);
        // R-93: the plan's SoC curve is read straight off the solved `soc_ev`; predicted
        // presence is `a_ev[t]`, the mask the solve ran under, so a slot missing from it was
        // never chargeable, which is what "not plugged" means to the plan.
        let planned_state = (0..n.min(sol.soc_ev.len()))
            .map(|t| {
                let plugged = self.ctx.a_ev.get(t).copied().unwrap_or(false);
                ev_future_state_at(sol.soc_ev[t], plugged)
            })
            .collect();
        AssetSolution {
            asset_id: self.ctx.asset_id.clone(),
            net_power_kw: sol.p_ev_kw.clone(),
            planned_state,
            storage: None,
            warnings: self.ctx.firm_shortfall(&sol).into_iter().collect(),
            detail: AssetSolutionDetail::Ev(sol),
        }
    }

    fn on_indicator(&self) -> Option<OnIndicator<'_>> {
        Some(OnIndicator {
            z_on: &self.v.z_ev_on,
            p_min_kw: self.v.p_min_kw,
        })
    }
}

impl AssetMilpContext for EvMilpContext {
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
        let pinned_on = match mode {
            DeclareMode::Plan { .. } => None,
            DeclareMode::Pinned(own) => Some(&own_readback(own).z_ev_on[..]),
        };
        let v = self.declare_vars_with(n, priced.startup_eur, priced.ramp_eur_kw, vars, pinned_on);
        Box::new(DeclaredEv { ctx: self, v })
    }

    /// The EV claims PV surplus first.
    fn surplus_claim_rank(&self) -> u8 {
        0
    }

    #[cfg(test)]
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn allocation_basis(&self) -> AllocationBasis {
        AllocationBasis::Load
    }

    /// The session this EV serves next, from now to its departure. The energy is the firm
    /// requirement when there is one, else what the comfort bands' bids cover
    /// (`ev-comfort-piecewise-core`: there is no single "core" block any more); none when
    /// that is nothing.
    fn flexibility_window(&self, now: DateTime<Utc>) -> Option<FlexibilityWindow> {
        let window_end = self.session_departure?;
        let firm_kwh = self.firm_required_kwh();
        let energy_needed_kwh = if firm_kwh > 0.0 {
            firm_kwh
        } else {
            self.segments.iter().map(|s| s.kwh).sum()
        };
        (energy_needed_kwh > 0.0).then_some(FlexibilityWindow {
            energy_needed_kwh,
            power_min_kw: self.p_min_kw,
            power_max_kw: self.p_max_kw,
            window_start: now,
            window_end,
        })
    }

    /// WP4.1-c: even at the cheapest slot rate the target energy (all "extra" headroom in
    /// MAX_COST mode) exceeds the budget, so charging will stop early. Stable text: WP4.3's
    /// notification dedup keys on it.
    fn input_warnings(&self, c_imp_eur_kwh: &[f64]) -> Vec<PlanWarning> {
        let mut warnings = self.targets_above_the_charge_limit();
        let Some(budget_eur) = self.budget_eur else {
            return warnings;
        };
        let min_rate = c_imp_eur_kwh.iter().cloned().fold(f64::INFINITY, f64::min);
        if self.e_extra_max_kwh * min_rate <= budget_eur {
            return warnings;
        }
        warnings.push(PlanWarning {
            severity: WarningSeverity::Warning,
            kind: WarningKind::BudgetShortfall,
            message: "EV charging budget too low to reach the session target — \
                      charging stops at the budget (MAX_COST)"
                .to_string(),
            suggested_action: Some("raise the budget or lower the target SoC".to_string()),
        });
        warnings
    }

    fn inject_grid_slots(&mut self, c_imp_eur_kwh: &[f64], p_pv_kw: &[f64], p_base_kw: &[f64]) {
        // WP4.1-c MAX_COST: keep the per-slot import rates for the budget constraint.
        if self.budget_eur.is_some() {
            self.c_imp_eur_kwh = Some(c_imp_eur_kwh.to_vec());
        }
        if !self.free_only {
            return;
        }
        // Free energy per slot: forecast PV surplus over the baseline load,
        // opened fully when the grid pays (or charges nothing) for import.
        let cap: Vec<f64> = c_imp_eur_kwh
            .iter()
            .zip(p_pv_kw)
            .zip(p_base_kw)
            .map(|((&c_imp, &pv), &base)| {
                if c_imp <= 0.0 {
                    self.p_max_kw
                } else {
                    (pv - base).max(0.0).min(self.p_max_kw)
                }
            })
            .collect();
        self.p_free_cap_kw = Some(cap);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::controller::milp_planner::{CostWeights, FrictionCosts};
    use crate::entities::device_session::{EvSession, EvSessionOrigin};
    use chrono::{Duration, TimeZone};
    use good_lp::IntoAffineExpression;

    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 4, 11, 6, 0, 0).unwrap()
    }

    fn charger() -> crate::assets::EvCharger {
        crate::assets::EvCharger {
            max_charge_kw: 7.4,
            max_discharge_kw: 0.0,
            v2g_capable: false,
            battery_kwh: 60.0,
            consumption_kwh_per_km: 0.18,
            soc_target_frac: 0.8,
            soc_target_profile: 0.8,
            default_charge_kw: 7.4,
            min_soc_frac: 0.0,
            min_charge_kw: 1.4,
            response_delay_s: 0.0,
            departure_time: None,
            usage_sim: None,
            usage_sim_seed_tag: 0,
        }
    }

    fn session(
        id: u128,
        departure_h: i64,
        mode: crate::entities::design_vocabulary::UserRequestMode,
        budget_eur: Option<f64>,
    ) -> EvSession {
        EvSession {
            id: uuid::Uuid::from_u128(id),
            mode,
            origin: EvSessionOrigin::UserRequest,
            target_soc_frac: 0.8,
            window_start: now() - Duration::hours(1),
            expected_trip_distance_km: None,
            expected_return_time: None,
            departure_time: now() + Duration::hours(departure_h),
            soft_deadline: false,
            budget_eur,
            comfort_rates: vec![],
            created_at: now(),
            updated_at: now(),
        }
    }

    /// The EV planned for 4 one-hour slots, plugged at 30 %, serving `sessions`.
    fn ev(sessions: &[EvSession]) -> EvMilpContext {
        let state = crate::assets::AssetState::Ev(crate::assets::ev::EvState {
            soc_frac: 0.30,
            plugged: true,
            actual_power_kw: 0.0,
            pending_command_kw: 0.0,
            was_away_by_usage_sim: false,
        });
        let cum_s: Vec<i64> = (0..=4).map(|i| i * 3600).collect();
        let mut ctx = EvMilpContext::from_state(
            "car",
            &state,
            &charger(),
            4,
            &cum_s,
            now(),
            sessions,
            &[],
            1.4,
            0.0,
            1.0,
            0.0,
            0.0,
            0.0,
        );
        ctx.friction = FrictionCosts {
            startup_eur: 0.01,
            ramp_eur_kw: 0.005,
            tier_penalty_eur: 0.0,
        };
        ctx
    }

    fn variables_of(e: Expression) -> std::collections::HashSet<Variable> {
        e.linear_coefficients()
            .filter(|(_, c)| *c != 0.0)
            .map(|(v, _)| v)
            .collect()
    }

    /// Its term in the site balance is its charging power, one for one.
    #[test]
    fn net_power_is_the_charging_power() {
        let ctx = ev(&[]);
        let mut vars = good_lp::variables!();
        let v = ctx.declare_vars(4, 0.0, 0.0, &mut vars);
        let p_ev = v.p_ev[2];
        let declared = DeclaredEv { ctx: &ctx, v };
        let coefficients: Vec<_> = declared.net_power_expr(2).linear_coefficients().collect();
        assert_eq!(coefficients, vec![(p_ev, 1.0)]);
    }

    /// Cost carries the comfort reward and no friction; friction carries the startups and ramps
    /// and no comfort reward.
    #[test]
    fn the_cost_part_prices_service_and_the_friction_part_prices_starts_and_ramps() {
        let firm = [session(1, 3, Default::default(), None)];
        let ctx = ev(&firm);
        let mut vars = good_lp::variables!();
        let v = ctx.declare_vars(4, 0.01, 0.005, &mut vars);
        let (bands, starts, ramps) = (v.e_seg.clone(), v.delta_ev.clone(), v.delta_ev_ramp.clone());
        assert!(!bands.is_empty() && !starts.is_empty() && !ramps.is_empty());
        let declared = DeclaredEv { ctx: &ctx, v };
        let cost = variables_of(declared.objective(
            ObjectivePart::Cost(&CostWeights {
                c_storage_wear_eur_kwh: 0.03,
            }),
            4,
            &[1.0; 4],
        ));
        let friction = variables_of(declared.objective(ObjectivePart::Friction, 4, &[1.0; 4]));
        let priced_bands = bands.iter().take(ctx.segments.len());
        assert!(priced_bands.clone().all(|b| cost.contains(b)));
        assert!(starts.iter().chain(&ramps).all(|d| !cost.contains(d)));
        assert!(starts.iter().chain(&ramps).all(|d| friction.contains(d)));
        assert!(priced_bands.into_iter().all(|b| !friction.contains(b)));
    }

    /// The session the plan reports is the one the EV serves next: a departed session is
    /// skipped, as `EvSessionQueue::upcoming` skips it.
    #[test]
    fn the_flexibility_window_is_the_upcoming_session() {
        let departed = session(1, -1, Default::default(), None);
        let next = session(2, 3, Default::default(), None);
        let window = ev(&[departed, next.clone()])
            .flexibility_window(now())
            .expect("a firm session needs energy");
        assert_eq!(window.window_end, next.departure_time);
        assert_eq!(window.window_start, now());
        assert_eq!((window.power_min_kw, window.power_max_kw), (1.4, 7.4));
        // 30 % to the 80 % target of a 60 kWh pack.
        assert!((window.energy_needed_kwh - 30.0).abs() < 1e-9);
    }

    #[test]
    fn no_session_declares_no_flexibility_window() {
        assert_eq!(ev(&[]).flexibility_window(now()), None);
    }

    /// WP4.1-c: a MAX_COST budget that cannot buy the target even at the cheapest slot rate is
    /// reported before the solve; one that can is not.
    #[test]
    fn a_budget_too_small_for_the_target_is_reported_before_the_solve() {
        use crate::entities::design_vocabulary::UserRequestMode;
        let tariff = [0.25; 4];
        let too_low = ev(&[session(1, 3, UserRequestMode::MaxCost, Some(0.01))]);
        let ample = ev(&[session(1, 3, UserRequestMode::MaxCost, Some(1000.0))]);
        let warned = too_low.input_warnings(&tariff);
        assert_eq!(warned.len(), 1);
        assert_eq!(warned[0].kind, WarningKind::BudgetShortfall);
        assert!(ample.input_warnings(&tariff).is_empty());
    }
}
