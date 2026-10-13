//! Synthetic fixtures shared by the planner tests (R-120): one `MilpInputs` with no optional
//! assets, a `SyntheticSite` for the tests that add some, the flat weights, and the plain EV
//! session. A test states what it changes on top of these, by assigning the fields it is about
//! or through struct-update syntax; nothing here may encode a scenario of its own.
//!
//! An asset is stated as its own MILP context, the thing the solve reads (R-134). `MilpInputs`
//! is never the place to describe an asset: it carries the site and the grid only.

use super::*;

/// `MilpInputs` for `n` slots of `step_s` seconds with no optional assets: flat tariffs (0.25
/// import, 0.08 export), 0.30 kgCO2/kWh, a 25 kW / 10 kW grid, `base_kw` of base load.
pub(super) fn synthetic_inputs(n: usize, step_s: i64, base_kw: f64) -> MilpInputs {
    MilpInputs {
        n,
        dt_h: vec![crate::entities::units::dt_h_from_s(step_s as f64); n],
        cum_s: (0..=n as i64).map(|i| i * step_s).collect(),
        c_imp_eur_kwh: vec![0.25; n],
        rate_stale: vec![false; n],
        stale_rate_warning: None,
        co2_stale_rate_warning: None,
        c_exp_eur_kwh: vec![0.08; n],
        g_imp_kgco2_kwh: vec![0.30; n],
        p_pv_kw: vec![0.0; n],
        p_base_kw: vec![base_kw; n],
        p_imp_max_phys_kw: vec![25.0; n],
        p_exp_max_phys_kw: vec![10.0; n],
        p_imp_max_cont_kw: vec![25.0; n],
        p_exp_max_cont_kw: vec![10.0; n],
        pen_imp_eur_kwh: 0.0,
        pen_exp_eur_kwh: 0.0,
        mip_gap_target: 0.02,
        penalty_rules: vec![],
    }
}

pub(super) fn make_phase1_weights() -> Phase1Weights {
    Phase1Weights {
        w_energy: 1.0,
        w_ghg: 0.0,
        w_grid: 0.0,
        w_import: 0.0,
        w_viol: 1.0,
        c_storage_wear_eur_kwh: 0.0,
        c_bat_ev_coexist_eur_kwh: 0.0,
        c_ctrl_imp_malus_eur_kwh: 0.0,
    }
}

/// A user-requested EV session from `now` to `departure_time`: firm deadline, no trip, no
/// budget, no comfort curve. A test that needs another mode or a budget names it through
/// struct-update syntax.
pub(super) fn ev_session_until(
    now: DateTime<Utc>,
    target_soc_frac: f64,
    departure_time: DateTime<Utc>,
) -> crate::entities::device_session::EvSession {
    crate::entities::device_session::EvSession {
        mode: Default::default(),
        origin: crate::entities::device_session::EvSessionOrigin::UserRequest,
        id: uuid::Uuid::new_v4(),
        target_soc_frac,
        window_start: now,
        expected_trip_distance_km: None,
        expected_return_time: None,
        departure_time,
        soft_deadline: false,
        budget_eur: None,
        comfort_rates: vec![],
        created_at: now,
        updated_at: now,
    }
}

/// A site for a solver test: the grid side as `MilpInputs`, and each asset as its MILP context.
/// The EV and the heater are always stated and take part once they have power; the battery
/// takes part once it is set.
pub(super) struct SyntheticSite {
    pub grid: MilpInputs,
    pub battery: Option<BatteryMilpContext>,
    pub ev: EvMilpContext,
    pub heater: HeaterMilpContext,
}

impl SyntheticSite {
    /// The contexts of the assets that take part, in the order the model declares them.
    pub fn contexts(&self) -> Vec<Box<dyn crate::controller::milp_planner::AssetMilpContext>> {
        let mut v: Vec<Box<dyn crate::controller::milp_planner::AssetMilpContext>> = Vec::new();
        if let Some(battery) = &self.battery {
            v.push(Box::new(battery.clone()));
        }
        if self.ev.p_max_kw > 0.0 {
            v.push(Box::new(self.ev.clone()));
        }
        if self.heater.p_step_kw * self.heater.n_stages as f64 > 0.0 {
            v.push(Box::new(self.heater.clone()));
        }
        v
    }

    /// The inputs of this site: the grid side.
    pub fn inputs(&self) -> MilpInputs {
        self.grid.clone()
    }

    /// Phase 1 on this site, with the solver's test time limit.
    pub fn solve_phase1(
        &self,
        weights: &Phase1Weights,
    ) -> Result<SolveOutput, Box<dyn std::error::Error>> {
        solve_phase1(&self.inputs(), weights, &self.contexts(), 60.0)
    }

    /// The firm energy this EV's obligations demand above its live SoC [kWh].
    pub fn firm_kwh(&self) -> f64 {
        self.ev.firm_obligation_kwh()
    }

    /// State one firm EV obligation of `kwh` by `deadline_step`, from this EV's own pack size
    /// and live SoC, so the kWh→SoC conversion lives in one place.
    pub fn set_ev_firm(&mut self, kwh: f64, deadline_step: usize) {
        self.ev.obligations = ev_firm_kwh(
            self.ev.battery_kwh,
            self.ev.soc_init_frac,
            kwh,
            deadline_step,
        );
    }
}

/// `synthetic_inputs(n, step_s, base_kw)` as a site whose EV and heater are absent (no power).
pub(super) fn synthetic_site(n: usize, step_s: i64, base_kw: f64) -> SyntheticSite {
    SyntheticSite {
        grid: synthetic_inputs(n, step_s, base_kw),
        battery: None,
        ev: absent_ev(n),
        heater: HeaterMilpContext {
            asset_id: "heater".into(),
            friction: Default::default(),
            temp_min_c: 18.0,
            thermal_mass_kwh_per_c: 2.0,
            target_ready_by: None,
            mode: HeaterMilpMode::MustNotRun,
            t_dead_step: None,
            p_step_kw: 0.0,
            n_stages: 0,
            e_init_kwh: 0.0,
            e_max_kwh: 0.0,
            q_dem_kw: 0.0,
            e_target_kwh: 0.0,
            lambda_sw_eur: 0.0,
            initial_y: 0.0,
            c_terminal_eur_kwh: 0.0,
            anchored_kw: vec![],
            comfort_full_reward_eur_kwh: 0.0,
            comfort_full_co2_reward_eur_kwh: 0.0,
        },
    }
}

/// An EV that takes no part: unplugged throughout and without power. A test gives it power, a
/// mode and a plugged window to bring it in.
pub(super) fn absent_ev(n: usize) -> EvMilpContext {
    EvMilpContext {
        asset_id: "ev".into(),
        friction: Default::default(),
        session_departure: None,
        mode: EvMilpMode::MustNotRun,
        soc_init_frac: 0.0,
        soc_max_frac: 1.0,
        a_ev: vec![false; n],
        soc_drops: None,
        obligations: vec![],
        // A real pack size even while the EV is absent: the SoC balance divides by it.
        battery_kwh: 60.0,
        p_max_kw: 0.0,
        p_min_kw: 0.0,
        segments: vec![],
        e_extra_max_kwh: 0.0,
        v_extra_eur_kwh: 0.0,
        asap_lateness_eur_kwh_h: 0.0,
        free_only: false,
        p_free_cap_kw: None,
        reward_per_slot: false,
        free_early_bias: false,
        budget_eur: None,
        c_imp_eur_kwh: None,
        v_extra_co2_eur_kwh: 0.0,
    }
}

/// `synthetic_site` on hourly slots, the shape the solver's unit tests use.
pub(super) fn make_solver_site(n: usize, base_kw: f64) -> SyntheticSite {
    synthetic_site(n, 3600, base_kw)
}

/// A battery's context, from the real asset: `capacity_kwh` usable up to full, holding
/// `init_kwh`, never below `min_kwh`, charging and discharging at up to `p_max_kw`, with
/// `eff_one_way` on each leg.
pub(super) fn battery_ctx(
    capacity_kwh: f64,
    init_kwh: f64,
    min_kwh: f64,
    p_max_kw: f64,
    eff_one_way: f64,
) -> BatteryMilpContext {
    Battery {
        capacity_kwh,
        max_charge_kw: p_max_kw,
        max_discharge_kw: p_max_kw,
        round_trip_efficiency: eff_one_way * eff_one_way,
        min_soc_frac: min_kwh / capacity_kwh,
    }
    .build_milp_context("battery", init_kwh / capacity_kwh, 0.0)
}

/// `battery_ctx` without losses.
pub(super) fn lossless_battery(
    capacity_kwh: f64,
    init_kwh: f64,
    min_kwh: f64,
    p_max_kw: f64,
) -> BatteryMilpContext {
    battery_ctx(capacity_kwh, init_kwh, min_kwh, p_max_kw, 1.0)
}
