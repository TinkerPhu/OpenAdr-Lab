use super::*;

// ── Tests ────────────────────────────────────────────────────────────────

#[test]
fn co2_g_kwh_divided_by_1000() {
    // CO₂ stored in tariffs as g/kWh; MILP needs kgCO₂/kWh
    let now = fixed_now();
    let profile = make_profile();
    let tariffs = make_tariffs(0.25, 0.08, 450.0); // 450 g/kWh
    let sim = make_snap_from_profile(&profile);
    let inp = bmi(&profile, &sim, &tariffs, &no_capacity(), now, None, None);
    // All slots should have 0.45 kgCO₂/kWh
    assert!(inp.g_imp_kgco2_kwh.iter().all(|&v| (v - 0.45).abs() < 1e-9));
}

#[test]
fn battery_eff_is_sqrt_rte() {
    // Each direction gets √(round_trip_efficiency), not the full RTE
    let now = fixed_now();
    let profile = make_profile(); // battery RTE = 0.9
    let sim = make_snap_from_profile(&profile);
    let inp = bmi(
        &profile,
        &sim,
        &TariffTimeSeries::from_snapshots(&[]),
        &no_capacity(),
        now,
        None,
        None,
    );
    let expected = 0.9_f64.sqrt();
    assert!((inp.eff_bat_ch.unwrap() - expected).abs() < 1e-9);
    assert!((inp.eff_bat_dis.unwrap() - expected).abs() < 1e-9);
    // Symmetry: both directions use the same value
    assert!((inp.eff_bat_ch.unwrap() - inp.eff_bat_dis.unwrap()).abs() < 1e-9);
}

#[test]
fn battery_init_soc_uses_live_state() {
    // When SimState has battery with SoC=0.3, build_milp_inputs should use 0.3×capacity,
    // not the profile's initial_soc=0.5.
    let now = fixed_now();
    let profile = make_profile(); // initial_soc=0.5, capacity=10.0
    let mut sim = make_snap_from_profile(&profile);
    set_battery_soc(&mut sim, 0.3); // override to 0.3
    let inp = bmi(
        &profile,
        &sim,
        &TariffTimeSeries::from_snapshots(&[]),
        &no_capacity(),
        now,
        None,
        None,
    );
    assert!((inp.e_bat_init_kwh.unwrap() - 3.0).abs() < 1e-9); // 0.3 × 10.0 = 3.0
}

#[test]
fn battery_init_soc_falls_back_to_profile() {
    // When SimState has no battery asset, fall back to profile.battery_config().initial_soc
    let now = fixed_now();
    let profile = make_profile(); // initial_soc=0.5, capacity=10.0
    let mut sim = make_snap_from_profile(&profile);
    sim.assets.clear(); // remove all assets → battery_state() returns None
    let inp = bmi(
        &profile,
        &sim,
        &TariffTimeSeries::from_snapshots(&[]),
        &no_capacity(),
        now,
        None,
        None,
    );
    assert!((inp.e_bat_init_kwh.unwrap() - 5.0).abs() < 1e-9); // 0.5 × 10.0 = 5.0
}

#[test]
fn ev_mask_plugged_no_session_all_true() {
    // Plugged EV with no session → all slots available (mask true), mode MustNotRun
    let now = fixed_now();
    let profile = make_profile();
    let mut sim = make_snap_from_profile(&profile);
    set_ev_plugged(&mut sim, true);
    let inp = bmi(
        &profile,
        &sim,
        &TariffTimeSeries::from_snapshots(&[]),
        &no_capacity(),
        now,
        None,
        None,
    );
    assert!(inp.a_ev.iter().all(|&v| v));
    assert_eq!(inp.ev_mode, MilpLoadMode::MustNotRun); // no session → MustNotRun (but mask is true)
    assert_eq!(inp.t_ev_dead_step, None);
}

#[test]
fn ev_mask_plugged_with_session_deadline() {
    // Plugged EV with EvSession deadline at 1h → first 12 slots true (step=300s, 12×300=3600s)
    let now = fixed_now();
    let profile = make_profile(); // plan_step_s=300, plan_horizon_h=2 → 24 steps
    let mut sim = make_snap_from_profile(&profile);
    set_ev_plugged(&mut sim, true);
    let session = crate::entities::device_session::EvSession {
        id: uuid::Uuid::new_v4(),
        target_soc: 0.9,
        departure_time: now + Duration::hours(1),
        soft_deadline: false,
        created_at: now,
        updated_at: now,
    };
    let inp = bmi(
        &profile,
        &sim,
        &TariffTimeSeries::from_snapshots(&[]),
        &no_capacity(),
        now,
        Some(&session),
        None,
    );
    // deadline = 3600s, step_s=300 → deadline_step = 12
    let d = inp.t_ev_dead_step.unwrap();
    assert_eq!(d, 12);
    // Slots 0..=12 true, slots 13..23 false
    for t in 0..inp.n {
        assert_eq!(inp.a_ev[t], t <= d, "slot {t} mask mismatch");
    }
}

#[test]
fn ev_mask_unplugged_all_false() {
    // Unplugged EV → all slots false regardless of session
    let now = fixed_now();
    let profile = make_profile();
    let mut sim = make_snap_from_profile(&profile);
    set_ev_plugged(&mut sim, false);
    let session = crate::entities::device_session::EvSession {
        id: uuid::Uuid::new_v4(),
        target_soc: 0.9,
        departure_time: now + Duration::hours(1),
        soft_deadline: false,
        created_at: now,
        updated_at: now,
    };
    let inp = bmi(
        &profile,
        &sim,
        &TariffTimeSeries::from_snapshots(&[]),
        &no_capacity(),
        now,
        Some(&session),
        None,
    );
    assert!(inp.a_ev.iter().all(|&v| !v));
    assert_eq!(inp.ev_mode, MilpLoadMode::MustNotRun);
}

#[test]
fn ev_mode_must_run_for_firm_deadline_session() {
    let now = fixed_now();
    let profile = make_profile();
    let mut sim = make_snap_from_profile(&profile);
    set_ev_plugged(&mut sim, true);
    let session = crate::entities::device_session::EvSession {
        id: uuid::Uuid::new_v4(),
        target_soc: 0.9,
        departure_time: now + Duration::hours(2),
        soft_deadline: false,
        created_at: now,
        updated_at: now,
    };
    let inp = bmi(
        &profile,
        &sim,
        &TariffTimeSeries::from_snapshots(&[]),
        &no_capacity(),
        now,
        Some(&session),
        None,
    );
    assert_eq!(inp.ev_mode, MilpLoadMode::MustRun);
}

#[test]
fn ev_mode_may_run_for_soft_deadline_session() {
    let now = fixed_now();
    let profile = make_profile();
    let mut sim = make_snap_from_profile(&profile);
    set_ev_plugged(&mut sim, true);
    let session = crate::entities::device_session::EvSession {
        id: uuid::Uuid::new_v4(),
        target_soc: 0.9,
        departure_time: now + Duration::hours(2),
        soft_deadline: true,
        created_at: now,
        updated_at: now,
    };
    let inp = bmi(
        &profile,
        &sim,
        &TariffTimeSeries::from_snapshots(&[]),
        &no_capacity(),
        now,
        Some(&session),
        None,
    );
    assert_eq!(inp.ev_mode, MilpLoadMode::MayRun);
}

#[test]
fn ev_mode_must_not_run_for_no_session() {
    let now = fixed_now();
    let profile = make_profile();
    let mut sim = make_snap_from_profile(&profile);
    set_ev_plugged(&mut sim, true);
    // No session at all
    let inp = bmi(
        &profile,
        &sim,
        &TariffTimeSeries::from_snapshots(&[]),
        &no_capacity(),
        now,
        None,
        None,
    );
    assert_eq!(inp.ev_mode, MilpLoadMode::MustNotRun);
}

#[test]
fn tariff_fallback_when_series_empty() {
    // Empty TariffTimeSeries → defaults: imp=0.25, exp=0.08, co2=300g→0.30kg
    let now = fixed_now();
    let profile = make_profile();
    let sim = make_snap_from_profile(&profile);
    let empty_tariffs = TariffTimeSeries::from_snapshots(&[]);
    let inp = bmi(
        &profile,
        &sim,
        &empty_tariffs,
        &no_capacity(),
        now,
        None,
        None,
    );
    assert!(inp.c_imp_eur_kwh.iter().all(|&v| (v - 0.25).abs() < 1e-9));
    assert!(inp.c_exp_eur_kwh.iter().all(|&v| (v - 0.08).abs() < 1e-9));
    assert!(inp.g_imp_kgco2_kwh.iter().all(|&v| (v - 0.30).abs() < 1e-9));
}

#[test]
fn heater_mid_kw_defaults_to_half_max() {
    // HeaterConfig.mid_kw = None → p_heat_mid_kw = max_kw / 2.0
    let now = fixed_now();
    let profile = make_profile(); // heater max_kw=3.0, mid_kw=None
    let mut sim = make_snap_from_profile(&profile);
    set_ev_plugged(&mut sim, true); // avoid EV noise
    let inp = bmi(
        &profile,
        &sim,
        &TariffTimeSeries::from_snapshots(&[]),
        &no_capacity(),
        now,
        None,
        None,
    );
    assert!((inp.p_heat_mid_kw - 1.5).abs() < 1e-9); // 3.0 / 2.0
    assert!((inp.p_heat_full_kw - 3.0).abs() < 1e-9);
}

#[test]
fn heater_mid_kw_uses_explicit_value() {
    // HeaterConfig.mid_kw = Some(2.0) → p_heat_mid_kw = 2.0
    let now = fixed_now();
    let mut profile = make_profile();
    // Replace heater with one that has explicit mid_kw
    profile.assets = profile
        .assets
        .into_iter()
        .map(|a| match a {
            AssetProfile::Heater(mut h) => {
                h.mid_kw = Some(2.0);
                AssetProfile::Heater(h)
            }
            other => other,
        })
        .collect();
    let sim = make_snap_from_profile(&profile);
    let inp = bmi(
        &profile,
        &sim,
        &TariffTimeSeries::from_snapshots(&[]),
        &no_capacity(),
        now,
        None,
        None,
    );
    assert!((inp.p_heat_mid_kw - 2.0).abs() < 1e-9);
}

#[test]
fn weights_preset_min_cost() {
    let mut profile = make_profile();
    profile.planner.w_energy = 99.0; // should be overridden by preset
    profile.planner.w_ghg = 99.0;
    let w = build_phase1_weights(&profile, PlannerObjective::MinCost);
    assert!((w.w_energy - 1.0).abs() < 1e-9);
    assert!((w.w_ghg - 0.20).abs() < 1e-9);
    assert!((w.w_grid - 0.02).abs() < 1e-9);
    assert!((w.c_bat_wear_eur_kwh - 0.03).abs() < 1e-9);
}

#[test]
fn weights_preset_min_ghg() {
    let profile = make_profile();
    let w = build_phase1_weights(&profile, PlannerObjective::MinGhg);
    assert!((w.w_energy - 0.0).abs() < 1e-9);
    assert!((w.w_ghg - 10.0).abs() < 1e-9);
    assert!((w.c_bat_wear_eur_kwh - 0.0).abs() < 1e-9);
}

#[test]
fn weights_preset_custom_uses_fields() {
    let mut profile = make_profile();
    profile.planner.w_energy = 0.5;
    profile.planner.w_ghg = 0.001;
    profile.planner.w_grid = 0.1;
    profile.planner.c_bat_wear_eur_kwh = 0.02;
    let w = build_phase1_weights(&profile, PlannerObjective::Custom);
    assert!((w.w_energy - 0.5).abs() < 1e-9);
    assert!((w.w_ghg - 0.001).abs() < 1e-9);
    assert!((w.w_grid - 0.1).abs() < 1e-9);
    assert!((w.c_bat_wear_eur_kwh - 0.02).abs() < 1e-9);
}

#[test]
fn capacity_event_overrides_grid_limit() {
    // When OadrCapacityState has an active limit, p_imp_max_cont_kw should use it
    let now = fixed_now();
    let profile = make_profile(); // grid.max_import_kw = 25.0
    let sim = make_snap_from_profile(&profile);
    let capacity = OadrCapacityState {
        import_limit_kw: Some(5.0), // OpenADR event limit
        export_limit_kw: None,
        import_subscription_kw: None,
        import_reservation_kw: None,
        import_limit_event_id: None,
        export_limit_event_id: None,
        last_updated: None,
    };
    let inp = bmi(
        &profile,
        &sim,
        &TariffTimeSeries::from_snapshots(&[]),
        &capacity,
        now,
        None,
        None,
    );
    // Physical limit unchanged
    assert!(inp
        .p_imp_max_phys_kw
        .iter()
        .all(|&v| (v - 25.0).abs() < 1e-9));
    // Contractual limit uses the event value
    assert!(inp
        .p_imp_max_cont_kw
        .iter()
        .all(|&v| (v - 5.0).abs() < 1e-9));
}
