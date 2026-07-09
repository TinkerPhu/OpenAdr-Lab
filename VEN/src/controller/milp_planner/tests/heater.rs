use super::*;

// ── Heater trajectory model unit tests ────────────────────────────────────

#[test]
fn heater_inputs_e_init_positive_above_min() {
    // volume_l=200 → thermal_mass = 200×4.186/3600 ≈ 0.23256 kWh/°C
    // T_current=60, T_min=40 → e_init = (60−40) × 0.23256 ≈ 4.65 kWh
    let now = fixed_now();
    let profile = make_heater_only_profile(Some(200.0), 40.0, 80.0, 60.0);
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
    let expected = 20.0 * 200.0 * 4.186 / 3600.0;
    assert!(
        (inp.e_heat_init_kwh - expected).abs() < 0.01,
        "e_init={:.4} expected≈{:.4}",
        inp.e_heat_init_kwh,
        expected
    );
}

#[test]
fn heater_inputs_e_init_negative_below_min() {
    // T_current=35 < T_min=40 → e_init < 0
    let now = fixed_now();
    let profile = make_heater_only_profile(Some(200.0), 40.0, 80.0, 40.0);
    let mut sim = make_snap_from_profile(&profile);
    set_heater_temp(&mut sim, 35.0);
    let inp = bmi(
        &profile,
        &sim,
        &TariffTimeSeries::from_snapshots(&[]),
        &no_capacity(),
        now,
        None,
        None,
    );
    assert!(
        inp.e_heat_init_kwh < 0.0,
        "e_init {} should be negative when temp < T_min",
        inp.e_heat_init_kwh
    );
}

#[test]
fn heater_inputs_e_max_formula() {
    // e_max = (T_max − T_min) × thermal_mass = (80−40) × 200×4.186/3600 ≈ 9.30 kWh
    let now = fixed_now();
    let profile = make_heater_only_profile(Some(200.0), 40.0, 80.0, 40.0);
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
    let expected = 40.0 * 200.0 * 4.186 / 3600.0;
    assert!(
        (inp.e_heat_max_kwh - expected).abs() < 0.01,
        "e_max={:.4} expected≈{:.4}",
        inp.e_heat_max_kwh,
        expected
    );
}

#[test]
fn heater_inputs_q_dem_scalar() {
    // q_dem = draw_kw + k_loss × ((T_min+T_max)/2 − ambient)
    // With defaults: draw=0, k_loss=0.1, t_mid=(40+80)/2=60, ambient=10
    // → q_dem = 0 + 0.1 × (60−10) = 5.0 kW
    let now = fixed_now();
    let profile = make_heater_only_profile(Some(200.0), 40.0, 80.0, 60.0);
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
    assert!(
        (inp.q_heat_dem_kw - 5.0).abs() < 0.01,
        "q_dem={:.4} expected 5.0",
        inp.q_heat_dem_kw
    );
}

#[test]
fn heater_inputs_e_target_from_heater_target() {
    // e_target = (target_temp_c − T_min) × thermal_mass, clamped to [0, e_max]
    // target=70, T_min=40 → (70−40) × 200×4.186/3600 ≈ 6.98 kWh
    let now = fixed_now();
    let profile = make_heater_only_profile(Some(200.0), 40.0, 80.0, 60.0);
    let sim = make_snap_from_profile(&profile);
    let target = HeaterTarget {
        id: uuid::Uuid::new_v4(),
        target_temp_c: 70.0,
        ready_by: now + Duration::hours(1),
        created_at: now,
        updated_at: now,
    };
    let inp = bmi(
        &profile,
        &sim,
        &TariffTimeSeries::from_snapshots(&[]),
        &no_capacity(),
        now,
        None,
        Some(&target),
    );
    let expected = 30.0 * 200.0 * 4.186 / 3600.0;
    assert!(
        (inp.e_heat_target_kwh - expected).abs() < 0.01,
        "e_target={:.4} expected≈{:.4}",
        inp.e_heat_target_kwh,
        expected
    );
}

#[test]
fn heater_inputs_autonomous_e_target_is_e_max() {
    // Without HeaterTarget, e_heat_target_kwh == e_heat_max_kwh
    let now = fixed_now();
    let profile = make_heater_only_profile(Some(200.0), 40.0, 80.0, 60.0);
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
    assert!(
        (inp.e_heat_target_kwh - inp.e_heat_max_kwh).abs() < 1e-9,
        "autonomous: e_target {} should equal e_max {}",
        inp.e_heat_target_kwh,
        inp.e_heat_max_kwh
    );
}

#[test]
fn heater_inputs_autonomous_mode_is_may_run() {
    // Without HeaterTarget, heater_mode == MilpLoadMode::MayRun
    let now = fixed_now();
    let profile = make_heater_only_profile(Some(200.0), 40.0, 80.0, 60.0);
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
    assert_eq!(inp.heater_mode, MilpLoadMode::MayRun);
}

#[test]
fn heater_inputs_switching_penalty_defaults() {
    // HeaterConfig with no switching_penalty_eur → lambda_heat_sw_eur == 0.01
    let now = fixed_now();
    let profile = make_profile(); // heater has no switching_penalty_eur set
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
    assert!(
        (inp.lambda_heat_sw_eur - 0.01).abs() < 1e-9,
        "lambda_sw={} expected 0.01",
        inp.lambda_heat_sw_eur
    );
}

#[test]
fn solve_heater_dynamics_respected() {
    // e_tank[t+1] = e_tank[t] + (p_heat[t] − q_dem) × dt_h  must hold for every slot.
    // Use k_loss=0 and draw_kw=0 so q_dem=0 and the math is exact.
    let now = fixed_now();
    let mut profile = make_heater_only_profile(None, 18.0, 23.0, 20.0);
    if let Some(AssetProfile::Heater(ref mut h)) = profile.assets.get_mut(0) {
        h.k_loss_kw_per_c = 0.0;
        h.draw_kw = 0.0;
    }
    let sim = make_snap_from_profile(&profile);
    let tariffs = make_tariffs(0.25, 0.08, 300.0);
    let plan = run_planner(
        build_asset_contexts(&profile, &sim, now, None, None, &tariffs),
        &sim,
        &tariffs,
        &no_capacity(),
        &profile,
        now,
        crate::entities::asset::PlanTrigger::Periodic,
        None,
        None,
        &[],
        None,
        None,
    );
    let thermal_mass = 2.0_f64; // make_heater_only_profile default
    let temp_min = 18.0_f64;
    let dt_h = profile.planner.plan_step_s as f64 / 3600.0;
    for t in 0..plan.slots.len() - 1 {
        let p_kw = plan.slots[t]
            .planned_kw_by_asset
            .get("heater")
            .copied()
            .unwrap_or(0.0);
        let temp_t = plan.slots[t].planned_state_by_asset["heater"]["temp_c"];
        let temp_next = plan.slots[t + 1].planned_state_by_asset["heater"]["temp_c"];
        let e_t = (temp_t - temp_min) * thermal_mass;
        let e_next_expected = e_t + p_kw * dt_h; // q_dem=0
        let e_next_actual = (temp_next - temp_min) * thermal_mass;
        assert!(
            (e_next_actual - e_next_expected).abs() < 1e-3,
            "slot {t}: dynamics violated — e_tank[{t}+1]={e_next_actual:.4} expected {e_next_expected:.4} \
             (p={p_kw:.2} kW, dt_h={dt_h:.4})"
        );
    }
}

#[test]
fn solve_heater_must_run_meets_e_target() {
    // MustRun with deadline: e_tank[t_dead] ≥ e_target.
    // Lossless heater (q_dem=0) starts cold. Target = 21°C within 18 slots (90 min).
    // At full power (3 kW, 5-min slot): +0.25 kWh/slot. Need (21-18)×2=6 kWh; 18×0.25=4.5 kWh
    // with e_init=(20-18)×2=4 kWh: total possible = 4 + 4.5 = 8.5 kWh > 6 kWh → feasible.
    let now = fixed_now();
    let mut profile = make_heater_only_profile(None, 18.0, 23.0, 20.0);
    if let Some(AssetProfile::Heater(ref mut h)) = profile.assets.get_mut(0) {
        h.k_loss_kw_per_c = 0.0;
        h.draw_kw = 0.0;
    }
    let sim = make_snap_from_profile(&profile);
    let tariffs = make_tariffs(0.25, 0.08, 300.0);
    let target = crate::entities::device_session::HeaterTarget {
        id: uuid::Uuid::new_v4(),
        target_temp_c: 21.0,
        ready_by: now + Duration::seconds(18 * 300),
        created_at: now,
        updated_at: now,
    };
    let plan = run_planner(
        build_asset_contexts(&profile, &sim, now, None, Some(&target), &tariffs),
        &sim,
        &tariffs,
        &no_capacity(),
        &profile,
        now,
        crate::entities::asset::PlanTrigger::UserRequest,
        None,
        Some(&target),
        &[],
        None,
        None,
    );
    // t_dead_step = (18×300)/300 = 18; check plan.slots[18]["heater"]["temp_c"] ≥ 21.0
    let t_dead = 18_usize;
    assert!(
        plan.slots.len() > t_dead,
        "plan must have at least {t_dead} slots"
    );
    let temp_at_deadline = plan.slots[t_dead].planned_state_by_asset["heater"]["temp_c"];
    assert!(
        temp_at_deadline >= 21.0 - 0.01,
        "MustRun: temp_c at deadline slot {t_dead} = {temp_at_deadline:.3} must be ≥ 21.0"
    );
}

#[test]
fn solve_heater_soft_low_positive_when_below_min() {
    // e_init < 0 (temp_init < temp_min) → s_low[0] > 0.
    // Verified via planned_state_by_asset: slot-0 temp_c < temp_min (e_tank[0] = e_init < 0).
    // The planner must remain feasible — s_low relaxes the lower bound.
    let now = fixed_now();
    let profile = make_heater_only_profile(None, 18.0, 23.0, 16.0); // temp_init < temp_min
    let sim = make_snap_from_profile(&profile);
    let tariffs = make_tariffs(0.25, 0.08, 300.0);
    let plan = run_planner(
        build_asset_contexts(&profile, &sim, now, None, None, &tariffs),
        &sim,
        &tariffs,
        &no_capacity(),
        &profile,
        now,
        crate::entities::asset::PlanTrigger::Periodic,
        None,
        None,
        &[],
        None,
        None,
    );
    assert!(
        !plan.slots.is_empty(),
        "solver must remain feasible when starting below T_min"
    );
    // slot 0 reports e_tank[0] = e_init = (16-18)×2 = -4 kWh → temp_c = 18 + (-4/2) = 16°C
    let temp_slot0 = plan.slots[0].planned_state_by_asset["heater"]["temp_c"];
    assert!(
        temp_slot0 < 18.0,
        "slot 0 temp_c={temp_slot0:.2} should be below temp_min=18 (s_low active)"
    );
}

#[test]
fn solve_heater_switching_reduces_with_penalty() {
    // After the Phase 1/Phase 2 separation fix, lambda_sw_eur must NOT appear in Phase 1.
    // Verifies two properties:
    //   1. Phase 1 cost is identical regardless of lambda_sw_eur.
    //   2. Phase 2 with high lambda_sw produces fewer heater on/off transitions.
    //
    // Setup: tank at T_min + 0.5°C (1.0 kWh buffer, thermal_mass=2.0 kWh/°C).
    // Tariff alternates cheap(0.05)/expensive(0.40) every 5-min slot for 2 hours.
    // With lambda=0.0: Phase 2 has no switching objective → fragmented (cheap-only) plan.
    // With lambda=1.0: switching cost exceeds energy savings → Phase 2 consolidates.
    let now = fixed_now();
    let n_slots: usize = 24; // 2h at 5min steps
    let step_s: u64 = 300;

    let snaps: Vec<_> = (0..n_slots)
        .map(|t| {
            let tariff = if t % 2 == 0 { 0.05_f64 } else { 0.40_f64 };
            TariffSnapshot {
                interval_start: now + Duration::seconds(t as i64 * step_s as i64),
                interval_end: now + Duration::seconds((t as i64 + 1) * step_s as i64),
                import_tariff_eur_kwh: Some(tariff),
                export_tariff_eur_kwh: Some(0.04),
                co2_g_kwh: Some(300.0),
            }
        })
        .collect();
    let tariffs = TariffTimeSeries::from_snapshots(&snaps);

    // temp_initial_c=18.5: e_init = 0.5°C × 2.0 = 1.0 kWh above T_min.
    // Net gain per [on,off] pair ≈ +0.075 kWh → alternating schedule is feasible.
    let make_profile_lambda = |lambda: f64| -> Profile {
        let mut p = make_heater_only_profile(None, 18.0, 23.0, 18.5);
        if let Some(AssetProfile::Heater(ref mut h)) = p.assets.get_mut(0) {
            h.switching_penalty_eur = lambda;
        }
        // epsilon=2.0 gives Phase 2 enough room to consolidate 12 expensive slots
        // (12 × 0.10€ = 1.20€ extra energy) to eliminate ~23 switches (saving 23 × lambda).
        p.planner.phase2_epsilon_eur = 2.0;
        p
    };

    let profile_no = make_profile_lambda(0.0);
    let profile_high = make_profile_lambda(1.0);
    let sim_no = make_snap_from_profile(&profile_no);
    let sim_high = make_snap_from_profile(&profile_high);

    let plan_no = run_planner(
        build_asset_contexts(&profile_no, &sim_no, now, None, None, &tariffs),
        &sim_no,
        &tariffs,
        &no_capacity(),
        &profile_no,
        now,
        PlanTrigger::Periodic,
        None,
        None,
        &[],
        None,
        None,
    );
    let plan_high = run_planner(
        build_asset_contexts(&profile_high, &sim_high, now, None, None, &tariffs),
        &sim_high,
        &tariffs,
        &no_capacity(),
        &profile_high,
        now,
        PlanTrigger::Periodic,
        None,
        None,
        &[],
        None,
        None,
    );

    // Phase 1 cost must not depend on lambda_sw_eur after the fix.
    assert!(
        (plan_no.objective_eur - plan_high.objective_eur).abs() < 1e-3,
        "Phase 1 cost must be lambda-independent: no_lambda={:.4} high_lambda={:.4}",
        plan_no.objective_eur,
        plan_high.objective_eur
    );

    // Count heater on/off transitions between consecutive slots.
    let count_transitions = |plan: &Plan| -> usize {
        let powers: Vec<f64> = plan
            .slots
            .iter()
            .map(|s| s.planned_kw_by_asset.get("heater").copied().unwrap_or(0.0))
            .collect();
        powers
            .windows(2)
            .filter(|w| (w[0] > 0.1) != (w[1] > 0.1))
            .count()
    };

    let tr_no = count_transitions(&plan_no);
    let tr_high = count_transitions(&plan_high);
    assert!(
            tr_high < tr_no,
            "high lambda_sw should consolidate heater (fewer transitions): high={tr_high} no_lambda={tr_no}"
        );
}

#[test]
fn solve_heater_upper_bound_not_exceeded() {
    // e_tank[t] ≤ e_max for all t: temp_c[t] must never exceed temp_max.
    // Start near T_max with a cheap tariff so the optimizer is incentivised to overheat.
    let now = fixed_now();
    let profile = make_heater_only_profile(None, 18.0, 23.0, 22.8); // temp near T_max
    let sim = make_snap_from_profile(&profile);
    let tariffs = make_tariffs(0.05, 0.04, 300.0); // very cheap → heater wants to run
    let plan = run_planner(
        build_asset_contexts(&profile, &sim, now, None, None, &tariffs),
        &sim,
        &tariffs,
        &no_capacity(),
        &profile,
        now,
        crate::entities::asset::PlanTrigger::Periodic,
        None,
        None,
        &[],
        None,
        None,
    );
    let thermal_mass = 2.0_f64;
    let temp_min = 18.0_f64;
    let temp_max = 23.0_f64;
    let e_max = (temp_max - temp_min) * thermal_mass; // 10.0 kWh
    for (t, slot) in plan.slots.iter().enumerate() {
        let temp_c = slot.planned_state_by_asset["heater"]["temp_c"];
        let e_tank = (temp_c - temp_min) * thermal_mass;
        assert!(
            e_tank <= e_max + 1e-3,
            "slot {t}: e_tank={e_tank:.4} kWh exceeds e_max={e_max:.4} (temp_c={temp_c:.3})"
        );
    }
}

// T009: Battery soc trajectory is populated in planned_state_by_asset.
// SoC key must exist in every slot; in charging slots SoC must be non-decreasing.
#[test]
fn battery_planned_state_soc_populated_and_non_decreasing_in_charging_slots() {
    let now = fixed_now();
    let mut profile = make_profile_1800s();
    // Battery-only + base load to keep the problem simple
    profile
        .assets
        .retain(|a| matches!(a, AssetProfile::Battery(_) | AssetProfile::BaseLoad(_)));
    let mut sim = make_snap_from_profile(&profile);
    set_battery_soc(&mut sim, 0.1); // low SoC → planner will charge on cheap slots
    let tariffs = make_two_zone_tariffs(0.05, 0.40);
    let plan = run_planner(
        build_asset_contexts(&profile, &sim, now, None, None, &tariffs),
        &sim,
        &tariffs,
        &no_capacity(),
        &profile,
        now,
        crate::entities::asset::PlanTrigger::Periodic,
        None,
        None,
        &[],
        None,
        None,
    );
    // Every slot must have the "soc" key for the battery asset.
    for (t, slot) in plan.slots.iter().enumerate() {
        let state = slot
            .planned_state_by_asset
            .get("battery")
            .unwrap_or_else(|| panic!("slot {t}: planned_state_by_asset missing battery key"));
        assert!(
            state.contains_key("soc"),
            "slot {t}: missing 'soc' key in battery state map"
        );
    }
    // FR-008: in slots where the battery is charging, SoC must be non-decreasing.
    let socs: Vec<f64> = plan
        .slots
        .iter()
        .map(|s| s.planned_state_by_asset["battery"]["soc"])
        .collect();
    for t in 1..socs.len() {
        if plan.slots[t - 1].bat_charge_kw > 0.01 {
            assert!(
                    socs[t] >= socs[t - 1] - 1e-6,
                    "SoC must be non-decreasing in charging slot: slot {t} soc={:.4} < slot {} soc={:.4}",
                    socs[t], t - 1, socs[t - 1]
                );
        }
    }
}

// T014: EV soc trajectory is populated in planned_state_by_asset.
#[test]
fn ev_planned_state_soc_populated() {
    let now = fixed_now();
    let mut profile = make_profile_1800s();
    // EV + base load only (no battery, no heater, no PV)
    profile
        .assets
        .retain(|a| matches!(a, AssetProfile::Ev(_) | AssetProfile::BaseLoad(_)));
    profile.assets = profile
        .assets
        .into_iter()
        .map(|a| match a {
            AssetProfile::Ev(mut ev) => {
                ev.battery_kwh = 10.0;
                AssetProfile::Ev(ev)
            }
            other => other,
        })
        .collect();
    let mut sim = make_snap_from_profile(&profile);
    set_ev_plugged(&mut sim, true);
    // Set EV soc to 0.2
    if let Some(ev) = sim.assets.get_mut("ev") {
        let bat_kwh = ev.val("battery_kwh").unwrap_or(60.0);
        let soc_target = ev.val("soc_target").unwrap_or(0.8);
        let max_ch = ev.val("max_charge_kw").unwrap_or(7.4);
        ev.values.insert("soc".into(), 0.2);
        ev.cap_max_import_kw = if 0.2_f64 >= soc_target { 0.0 } else { max_ch };
        ev.available_discharge_kwh = Some(0.2 * bat_kwh);
        ev.available_charge_kwh = Some(0.8 * bat_kwh);
    }
    let session = crate::entities::device_session::EvSession {
        id: uuid::Uuid::new_v4(),
        target_soc: 0.8,
        departure_time: now + Duration::hours(2),
        soft_deadline: false,
        created_at: now,
        updated_at: now,
    };
    let tariffs = make_tariffs(0.25, 0.08, 300.0);
    let plan = run_planner(
        build_asset_contexts(&profile, &sim, now, Some(&session), None, &tariffs),
        &sim,
        &tariffs,
        &no_capacity(),
        &profile,
        now,
        crate::entities::asset::PlanTrigger::Periodic,
        Some(&session),
        None,
        &[],
        None,
        None,
    );
    // Every slot must have the "soc" key for the ev asset.
    for (t, slot) in plan.slots.iter().enumerate() {
        let state = slot
            .planned_state_by_asset
            .get("ev")
            .unwrap_or_else(|| panic!("slot {t}: planned_state_by_asset missing ev key"));
        assert!(
            state.contains_key("soc"),
            "slot {t}: missing 'soc' key in ev state map"
        );
        let soc = state["soc"];
        assert!(
            (0.0..=1.0).contains(&soc),
            "slot {t}: soc={soc} out of [0,1]"
        );
    }
    // First slot SoC must match the initial SoC (0.2)
    let first_soc = plan.slots[0].planned_state_by_asset["ev"]["soc"];
    assert!(
        (first_soc - 0.2).abs() < 1e-9,
        "expected first-slot soc=0.2, got {first_soc}"
    );
}

// T019: ven-3 heater config (200 L, 45–60 °C, 3-tier 300/600/900 s zones) must be feasible.
// Root cause of production infeasibility: heater_block_end() returned Some(horizon_end) for
// a fallback plan (all zeros), anchoring all 288 slots to heater-off. Forced-off dynamics
// drained e_tank below its domain lower bound → presolve contradiction in 32 ms.
// Fix: heater_block_end() returns None when kw0 < 0.1 (heater off or no heater data).
#[test]
fn solve_ven3_heater_three_tier_zones_feasible() {
    // Exact ven-3 production parameters that were causing MILP infeasibility.
    let now = fixed_now();
    let volume_l = 200.0_f64;
    let thermal_mass = volume_l * 4.186 / 3600.0; // ≈ 0.2326 kWh/K
    let profile = Profile {
        assets: vec![
            AssetProfile::Heater(HeaterParams {
                id: "heater".into(),
                max_kw: 6.0,
                mid_kw: Some(3.0),
                temp_initial_c: 47.82, // live temperature from ven-3
                temp_min_c: 45.0,
                temp_max_c: 60.0,
                thermal_mass_kwh_per_c: thermal_mass,
                k_loss_kw_per_c: 0.005,
                draw_kw: 0.3,
                switching_penalty_eur: 0.50,
                c_terminal_eur_kwh: None,
            }),
            AssetProfile::Ev(EvParams {
                id: "ev".into(),
                max_charge_kw: 11.0,
                max_discharge_kw: 0.0,
                initial_soc: 0.30,
                battery_kwh: 75.0,
                soc_target: 0.80,
                default_charge_kw: 0.0,
                min_charge_kw: 0.0,
                response_delay_s: 10.0,
            }),
            AssetProfile::Pv(PvParams {
                id: "pv".into(),
                rated_kw: 6.0,
            }),
            AssetProfile::BaseLoad(BaseLoadParams {
                id: "base_load".into(),
                baseline_kw: 0.6,
            }),
        ],
        simulator: SimulatorConfig::default(),
        planner: PlannerConfig {
            plan_step_s: 300,
            plan_horizon_h: 48,
            plan_zones: vec![
                crate::entities::plan::PlanZone {
                    step_s: 300,
                    slots: 96,
                },
                crate::entities::plan::PlanZone {
                    step_s: 600,
                    slots: 96,
                },
                crate::entities::plan::PlanZone {
                    step_s: 900,
                    slots: 96,
                },
            ],
            c_ctrl_imp_malus_eur_kwh: 0.22,
            phase2_epsilon_eur: 0.17,
            ..PlannerConfig::default()
        },
        grid: GridConfig {
            max_import_kw: 25.0,
            max_export_kw: 10.0,
        },
        packets: vec![],
    };
    let mut sim = make_snap_from_profile(&profile);
    // Production state: emergency thermostat running at 6 kW because temp < 48°C.
    // This sets initial_z_full=1.0 via HeaterMilpContext::from_state → must still be feasible.
    set_heater_power(&mut sim, 6.0);
    let tariffs = make_tariffs(0.25, 0.08, 300.0);
    let plan = run_planner(
        build_asset_contexts(&profile, &sim, now, None, None, &tariffs),
        &sim,
        &tariffs,
        &no_capacity(),
        &profile,
        now,
        crate::entities::asset::PlanTrigger::Periodic,
        None,
        None,
        &[],
        None,
        None,
    );
    assert!(
        plan.warnings.is_empty(),
        "ven-3 heater MILP should be feasible, got warnings: {:?}",
        plan.warnings
    );
    let heater_kws: Vec<f64> = plan
        .slots
        .iter()
        .map(|s| s.planned_kw_by_asset.get("heater").copied().unwrap_or(0.0))
        .collect();
    assert!(
        heater_kws.iter().any(|&kw| kw > 0.1),
        "heater should be planned for at least some slots; all zeros suggests MILP infeasibility"
    );
}

// T018: Heater T_tank trajectory is populated in planned_state_by_asset.
#[test]
fn heater_planned_state_temp_c_populated() {
    let now = fixed_now();
    let profile = make_heater_only_profile(None, 18.0, 23.0, 20.0);
    let sim = make_snap_from_profile(&profile);
    let tariffs = make_tariffs(0.25, 0.08, 300.0);
    let plan = run_planner(
        build_asset_contexts(&profile, &sim, now, None, None, &tariffs),
        &sim,
        &tariffs,
        &no_capacity(),
        &profile,
        now,
        crate::entities::asset::PlanTrigger::Periodic,
        None,
        None,
        &[],
        None,
        None,
    );
    // Every slot must have the "temp_c" key for the heater asset.
    for (t, slot) in plan.slots.iter().enumerate() {
        let state = slot
            .planned_state_by_asset
            .get("heater")
            .unwrap_or_else(|| panic!("slot {t}: planned_state_by_asset missing heater key"));
        assert!(
            state.contains_key("temp_c"),
            "slot {t}: missing 'temp_c' key in heater state map"
        );
        let temp_c = state["temp_c"];
        assert!(
            temp_c >= 18.0 - 1e-6,
            "slot {t}: temp_c={temp_c:.2} below temp_min_c=18.0"
        );
    }
}
