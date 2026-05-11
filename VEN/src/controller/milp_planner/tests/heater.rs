use super::*;

    // ── Heater trajectory model unit tests ────────────────────────────────────

    #[test]
    fn heater_inputs_e_init_positive_above_min() {
        // volume_l=200 → thermal_mass = 200×4.186/3600 ≈ 0.23256 kWh/°C
        // T_current=60, T_min=40 → e_init = (60−40) × 0.23256 ≈ 4.65 kWh
        let now = fixed_now();
        let profile = make_heater_only_profile(Some(200.0), 40.0, 80.0, 60.0);
        let sim = make_snap_from_profile(&profile);
        let inp = bmi(&profile, &sim, &TariffTimeSeries::from_snapshots(&[]), &no_capacity(), now, None, None);
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
        let inp = bmi(&profile, &sim, &TariffTimeSeries::from_snapshots(&[]), &no_capacity(), now, None, None);
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
        let inp = bmi(&profile, &sim, &TariffTimeSeries::from_snapshots(&[]), &no_capacity(), now, None, None);
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
        let inp = bmi(&profile, &sim, &TariffTimeSeries::from_snapshots(&[]), &no_capacity(), now, None, None);
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
        let inp = bmi(&profile, &sim, &TariffTimeSeries::from_snapshots(&[]), &no_capacity(), now, None, Some(&target));
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
        let inp = bmi(&profile, &sim, &TariffTimeSeries::from_snapshots(&[]), &no_capacity(), now, None, None);
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
        let inp = bmi(&profile, &sim, &TariffTimeSeries::from_snapshots(&[]), &no_capacity(), now, None, None);
        assert_eq!(inp.heater_mode, MilpLoadMode::MayRun);
    }

    #[test]
    fn heater_inputs_switching_penalty_defaults() {
        // HeaterConfig with no switching_penalty_eur → lambda_heat_sw_eur == 0.01
        let now = fixed_now();
        let profile = make_profile(); // heater has no switching_penalty_eur set
        let sim = make_snap_from_profile(&profile);
        let inp = bmi(&profile, &sim, &TariffTimeSeries::from_snapshots(&[]), &no_capacity(), now, None, None);
        assert!(
            (inp.lambda_heat_sw_eur - 0.01).abs() < 1e-9,
            "lambda_sw={} expected 0.01",
            inp.lambda_heat_sw_eur
        );
    }

    #[test]
    #[ignore = "implemented in Step 5"]
    fn solve_heater_dynamics_respected() {
        // After solve: e_tank[t+1] ≈ e_tank[t] + (p_heat[t] − q_dem) × dt_h ± 1e-3
        todo!("implement after solve_milp() heater trajectory integration")
    }

    #[test]
    #[ignore = "implemented in Step 5"]
    fn solve_heater_must_run_meets_e_target() {
        // MustRun with deadline: e_tank[t_dead] ≥ e_target − 1e-3
        todo!("implement after solve_milp() heater trajectory integration")
    }

    #[test]
    #[ignore = "implemented in Step 5"]
    fn solve_heater_soft_low_positive_when_below_min() {
        // e_init < 0: s_low[0] > 0 in solution
        todo!("implement after solve_milp() heater trajectory integration")
    }

    #[test]
    #[ignore = "implemented in Step 5"]
    fn solve_heater_switching_reduces_with_penalty() {
        // High lambda_sw → fewer mode changes than lambda_sw = 0
        todo!("implement after solve_milp() heater trajectory integration")
    }

    #[test]
    #[ignore = "implemented in Step 5"]
    fn solve_heater_upper_bound_not_exceeded() {
        // e_tank[t] ≤ e_max + 1e-6 for all t in solution
        todo!("implement after solve_milp() heater trajectory integration")
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
        let plan = run_planner(
            build_asset_contexts(&profile, &sim, now, None, None),
            &sim,
            &make_two_zone_tariffs(0.05, 0.40),
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
        let plan = run_planner(
            build_asset_contexts(&profile, &sim, now, Some(&session), None),
            &sim,
            &make_tariffs(0.25, 0.08, 300.0),
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

    // T018: Heater T_tank trajectory is populated in planned_state_by_asset.
    #[test]
    fn heater_planned_state_temp_c_populated() {
        let now = fixed_now();
        let profile = make_heater_only_profile(None, 18.0, 23.0, 20.0);
        let sim = make_snap_from_profile(&profile);
        let plan = run_planner(
            build_asset_contexts(&profile, &sim, now, None, None),
            &sim,
            &make_tariffs(0.25, 0.08, 300.0),
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
