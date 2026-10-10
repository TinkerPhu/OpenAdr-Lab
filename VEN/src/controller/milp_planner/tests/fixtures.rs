//! Synthetic fixtures shared by the planner tests (R-120): one `MilpInputs` with no optional
//! assets, the flat weights, and the plain EV session. A test states what it changes on top of
//! these, by assigning the fields it is about or through struct-update syntax; nothing here may
//! encode a scenario of its own.

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
        budget_warning: None,
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
        e_bat_nom_kwh: None,
        e_bat_init_kwh: None,
        e_bat_min_kwh: None,
        e_bat_max_kwh: None,
        p_bat_ch_max_kw: None,
        p_bat_dis_max_kw: None,
        eff_bat_ch: None,
        eff_bat_dis: None,
        a_ev: vec![false; n],
        ev_mode: MilpLoadMode::MustNotRun,
        ev_obligations: vec![],
        // A real pack size even when no EV participates: the SoC balance divides
        // by it, and a fixture that says "0 kWh of battery" is not a thing.
        ev_battery_kwh: 60.0,
        p_ev_max_kw: 0.0,
        p_ev_min_kw: 0.0,
        ev_segments: vec![],
        e_ev_extra_max_kwh: 0.0,
        v_ev_extra_eur_kwh: 0.0,
        heater_mode: MilpLoadMode::MustNotRun,
        t_heat_dead_step: None,
        p_heat_step_kw: 0.0,
        heat_n_stages: 0,
        e_heat_init_kwh: 0.0,
        e_heat_max_kwh: 0.0,
        q_heat_dem_kw: 0.0,
        e_heat_target_kwh: 0.0,
        lambda_heat_sw_eur: 0.0,
        w_tier_penalty_eur: 0.0,
        heat_initial_y: 0.0,
        shiftable_loads: vec![],
        soc_ev_init: None,
        ev_soc_drops: None,
    }
}

/// `synthetic_inputs` on hourly slots, the shape the solver's unit tests use.
pub(super) fn make_solver_inputs(n: usize, base_kw: f64) -> MilpInputs {
    synthetic_inputs(n, 3600, base_kw)
}

pub(super) fn make_phase1_weights() -> Phase1Weights {
    Phase1Weights {
        w_energy: 1.0,
        w_ghg: 0.0,
        w_grid: 0.0,
        w_import: 0.0,
        w_viol: 1.0,
        c_bat_wear_eur_kwh: 0.0,
        c_bat_ev_coexist_eur_kwh: 0.0,
        c_ctrl_imp_malus_eur_kwh: 0.0,
        w_services: 1.0,
    }
}

pub(super) fn make_phase2_weights() -> Phase2Weights {
    Phase2Weights {
        c_bat_startup_eur: 0.0,
        c_bat_ramp_eur_kw: 0.0,
        c_ev_startup_eur: 0.0,
        c_ev_ramp_eur_kw: 0.0,
        lambda_heat_sw_eur: 0.0,
        w_tier_penalty_eur: 0.0,
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
