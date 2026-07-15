use super::*;

// ── run_planner() regression-guard tests ─────────────────────────────────
// These call only the public run_planner() entry point so they remain valid
// through any internal refactor. Use 1800s steps (4 slots, 2h horizon) for
// fast solver runs (~<100ms each).

fn make_profile_1800s() -> Profile {
    let mut p = make_profile();
    p.planner.plan_step_s = 1800;
    p.planner.plan_zones = vec![crate::entities::plan::PlanZone {
        step_s: 1800,
        slots: 4,
    }];
    p
}

fn make_two_zone_tariffs(imp_cheap: f64, imp_exp: f64) -> TariffTimeSeries {
    let now = fixed_now();
    TariffTimeSeries::from_snapshots(&[
        TariffSnapshot {
            interval_start: now - Duration::hours(1),
            interval_end: now + Duration::hours(1),
            import_tariff_eur_kwh: Some(imp_cheap),
            export_tariff_eur_kwh: Some(0.08),
            co2_g_kwh: Some(300.0),
        },
        TariffSnapshot {
            interval_start: now + Duration::hours(1),
            interval_end: now + Duration::hours(3),
            import_tariff_eur_kwh: Some(imp_exp),
            export_tariff_eur_kwh: Some(0.08),
            co2_g_kwh: Some(300.0),
        },
    ])
}

#[test]
fn run_planner_no_assets_covers_base_load() {
    let now = fixed_now();
    let profile = Profile {
        assets: vec![AssetProfile::BaseLoad(BaseLoadConfig {
            id: "base_load".into(),
            baseline_kw: 1.0,
            spikes: vec![],
        })],
        simulator: SimulatorConfig,
        planner: PlannerConfig {
            plan_step_s: 1800,
            plan_horizon_h: 2,
            plan_zones: vec![crate::entities::plan::PlanZone {
                step_s: 1800,
                slots: 4,
            }],
            ..PlannerConfig::default()
        },
        grid: GridConfig {
            max_import_kw: 25.0,
            max_export_kw: 10.0,
        },
        packets: vec![],
    };
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
    assert_eq!(plan.slots.len(), 4);
    for slot in &plan.slots {
        assert!(
            (slot.net_import_kw - 1.0).abs() < 0.05,
            "expected net_import ≈ 1.0 kW, got {:.4}",
            slot.net_import_kw
        );
        assert!(slot.bat_charge_kw < 1e-3);
        assert!(slot.bat_discharge_kw < 1e-3);
        assert!(!slot.allocations.iter().any(|a| a.asset_id == "ev"));
    }
}

#[test]
fn run_planner_with_heuristic_baseline_kw_varies_per_slot() {
    // WP5.2 (BL-14) planner-consumption test: when a learned heuristic
    // exists for "base_load", the resulting plan's per-slot baseline_kw
    // must vary with hour-of-day instead of repeating the flat static
    // scalar — this is the literal fix for the Controller tab's
    // previously-flat future-horizon base_load line.
    let now = fixed_now(); // 2026-04-11 06:00:00 UTC
    let profile = Profile {
        assets: vec![AssetProfile::BaseLoad(BaseLoadConfig {
            id: "base_load".into(),
            baseline_kw: 1.0,
            spikes: vec![],
        })],
        simulator: SimulatorConfig,
        planner: PlannerConfig {
            plan_step_s: 1800,
            plan_horizon_h: 2,
            plan_zones: vec![crate::entities::plan::PlanZone {
                step_s: 1800,
                slots: 4,
            }],
            ..PlannerConfig::default()
        },
        grid: GridConfig {
            max_import_kw: 25.0,
            max_export_kw: 10.0,
        },
        packets: vec![],
    };
    let sim = make_snap_from_profile(&profile);
    let tariffs = make_tariffs(0.25, 0.08, 300.0);

    // daytime_profile_kw[h] = h as f64 kW — slots at 06:00/06:30/07:00/07:30
    // UTC sample hours 6,6,7,7, so the expected per-slot baseline is
    // 6.0, 6.0, 7.0, 7.0 kW: not constant, and traceable to a known formula.
    // Same curve in both weekday/weekend buckets — this test only cares
    // about hour-of-day variation, not the weekday/weekend split itself.
    let profile_by_hour: Vec<f64> = (0..24).map(|h| h as f64).collect();
    let mut heuristics = std::collections::HashMap::new();
    heuristics.insert(
        "base_load".to_string(),
        crate::entities::design_vocabulary::AssetHeuristics {
            asset_id: "base_load".to_string(),
            daytime_profile_kw: [profile_by_hour.clone(), profile_by_hour],
            seasonal_factor: 1.0,
            last_updated: Some(now),
        },
    );

    let plan = super::super::run_planner(
        build_asset_contexts(&profile, &sim, now, None, None, &tariffs),
        &sim,
        &tariffs,
        &no_capacity(),
        &[],
        &[],
        &profile.planner,
        profile.grid.max_import_kw,
        profile.grid.max_export_kw,
        &profile.assets,
        now,
        crate::entities::asset::PlanTrigger::Periodic,
        None,
        None,
        &[],
        None,
        None,
        None,
        &heuristics,
    );

    assert_eq!(plan.slots.len(), 4);
    let baselines: Vec<f64> = plan.slots.iter().map(|s| s.baseline_kw).collect();
    assert!(
        (baselines[0] - 6.0).abs() < 1e-6,
        "slot 0 (06:00) expected 6.0 kW, got {baselines:?}"
    );
    assert!(
        (baselines[2] - 7.0).abs() < 1e-6,
        "slot 2 (07:00) expected 7.0 kW, got {baselines:?}"
    );
    assert!(
        (baselines[0] - baselines[2]).abs() > 0.5,
        "baseline_kw must vary across slots when a heuristic is supplied, got {baselines:?}"
    );
}

#[test]
fn run_planner_with_heuristic_baseline_kw_differs_saturday_vs_tuesday() {
    // WP D2 (weekday/weekend split): a heuristic with distinct weekday and
    // weekend buckets must produce different plan.slots[t].baseline_kw for
    // the same hour-of-day depending on which weekday the plan is for —
    // this is the direct end-to-end proof of the 2-bucket restructure.
    use chrono::TimeZone;
    // 2023-01-03 was a Tuesday, 2023-01-07 a Saturday.
    let tuesday = Utc.with_ymd_and_hms(2023, 1, 3, 6, 0, 0).unwrap();
    let saturday = Utc.with_ymd_and_hms(2023, 1, 7, 6, 0, 0).unwrap();

    let profile = Profile {
        assets: vec![AssetProfile::BaseLoad(BaseLoadConfig {
            id: "base_load".into(),
            baseline_kw: 1.0,
            spikes: vec![],
        })],
        simulator: SimulatorConfig,
        planner: PlannerConfig {
            plan_step_s: 1800,
            plan_horizon_h: 2,
            plan_zones: vec![crate::entities::plan::PlanZone {
                step_s: 1800,
                slots: 4,
            }],
            ..PlannerConfig::default()
        },
        grid: GridConfig {
            max_import_kw: 25.0,
            max_export_kw: 10.0,
        },
        packets: vec![],
    };

    let mut weekday_profile = vec![0.0; 24];
    weekday_profile[6] = 6.0;
    let mut weekend_profile = vec![0.0; 24];
    weekend_profile[6] = 11.0;
    let mut heuristics = std::collections::HashMap::new();
    heuristics.insert(
        "base_load".to_string(),
        crate::entities::design_vocabulary::AssetHeuristics {
            asset_id: "base_load".to_string(),
            daytime_profile_kw: [weekday_profile, weekend_profile],
            seasonal_factor: 1.0,
            last_updated: Some(tuesday),
        },
    );

    let baseline_kw_for = |now: DateTime<Utc>| {
        let sim = make_snap_from_profile(&profile);
        let tariffs = TariffTimeSeries::from_snapshots(&[TariffSnapshot {
            interval_start: now - Duration::hours(1),
            interval_end: now + Duration::hours(25),
            import_tariff_eur_kwh: Some(0.25),
            export_tariff_eur_kwh: Some(0.08),
            co2_g_kwh: Some(300.0),
        }]);
        let plan = super::super::run_planner(
            build_asset_contexts(&profile, &sim, now, None, None, &tariffs),
            &sim,
            &tariffs,
            &no_capacity(),
            &[],
            &[],
            &profile.planner,
            profile.grid.max_import_kw,
            profile.grid.max_export_kw,
            &profile.assets,
            now,
            crate::entities::asset::PlanTrigger::Periodic,
            None,
            None,
            &[],
            None,
            None,
            None,
            &heuristics,
        );
        plan.slots[0].baseline_kw
    };

    let tuesday_baseline = baseline_kw_for(tuesday);
    let saturday_baseline = baseline_kw_for(saturday);

    assert!(
        (tuesday_baseline - 6.0).abs() < 1e-6,
        "Tuesday 06:00 should sample the weekday bucket, got {tuesday_baseline}"
    );
    assert!(
        (saturday_baseline - 11.0).abs() < 1e-6,
        "Saturday 06:00 should sample the weekend bucket, got {saturday_baseline}"
    );
    assert!(
        (tuesday_baseline - saturday_baseline).abs() > 1.0,
        "same hour-of-day must differ between a weekday and weekend plan"
    );
}

#[test]
fn run_planner_battery_absent_no_bat_allocation() {
    let now = fixed_now();
    let mut profile = make_profile_1800s();
    profile
        .assets
        .retain(|a| !matches!(a, AssetProfile::Battery(_)));
    let mut sim = make_snap_from_profile(&profile);
    set_ev_plugged(&mut sim, true);
    let session = crate::entities::device_session::EvSession {
        mode: Default::default(),
        id: uuid::Uuid::new_v4(),
        target_soc: 0.8,
        departure_time: now + Duration::hours(2),
        soft_deadline: false,
        budget_eur: None,
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
    for slot in &plan.slots {
        assert!(
            slot.bat_charge_kw < 1e-3,
            "battery absent → bat_charge_kw=0, got {:.4}",
            slot.bat_charge_kw
        );
        assert!(
            slot.bat_discharge_kw < 1e-3,
            "battery absent → bat_discharge_kw=0, got {:.4}",
            slot.bat_discharge_kw
        );
    }
    assert!(
        plan.soc_trajectory_kwh.is_empty() || plan.soc_trajectory_kwh.iter().all(|&v| v < 1e-3),
        "no battery → soc_trajectory_kwh empty or all-zero"
    );
}

#[test]
fn run_planner_ev_absent_no_ev_allocation() {
    let now = fixed_now();
    let mut profile = make_profile_1800s();
    profile.assets.retain(|a| !matches!(a, AssetProfile::Ev(_)));
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
    for slot in &plan.slots {
        assert!(
            !slot.allocations.iter().any(|a| a.asset_id == "ev"),
            "EV absent → no EV allocation"
        );
    }
}

#[test]
fn run_planner_battery_charges_on_cheap_tariff() {
    let now = fixed_now();
    let mut profile = make_profile_1800s();
    profile
        .assets
        .retain(|a| matches!(a, AssetProfile::Battery(_) | AssetProfile::BaseLoad(_)));
    let mut sim = make_snap_from_profile(&profile);
    set_battery_soc(&mut sim, 0.1); // low SoC → wants to charge
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
    let cheap_charge: f64 = plan.slots[0..2].iter().map(|s| s.bat_charge_kw).sum();
    let exp_dis: f64 = plan.slots[2..4].iter().map(|s| s.bat_discharge_kw).sum();
    assert!(
        cheap_charge > 0.1 || exp_dis > 0.1,
        "expected charging in cheap slots or discharging in expensive slots; \
             cheap_charge={:.3}, exp_dis={:.3}",
        cheap_charge,
        exp_dis
    );
}

#[test]
fn run_planner_ev_must_run_energy_met() {
    let now = fixed_now();
    let mut profile = make_profile_1800s();
    profile
        .assets
        .retain(|a| !matches!(a, AssetProfile::Heater(_) | AssetProfile::Pv(_)));
    // Shrink EV battery so 7 kWh is feasible in 2h at 7.4 kW
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
    // Set EV soc to 0.1
    if let Some(ev) = sim.assets.get_mut("ev") {
        let bat_kwh = ev.val("battery_kwh").unwrap_or(60.0);
        let soc_target = ev.val("soc_target").unwrap_or(0.8);
        let max_ch = ev.val("max_charge_kw").unwrap_or(7.4);
        ev.values.insert("soc".into(), 0.1);
        ev.cap_max_import_kw = if 0.1_f64 >= soc_target { 0.0 } else { max_ch };
        ev.available_discharge_kwh = Some(0.1 * bat_kwh);
        ev.available_charge_kwh = Some(0.9 * bat_kwh);
    }
    let e_core_kwh = (0.8 - 0.1) * 10.0; // 7.0 kWh
    let session = crate::entities::device_session::EvSession {
        mode: Default::default(),
        id: uuid::Uuid::new_v4(),
        target_soc: 0.8,
        departure_time: now + Duration::hours(2),
        soft_deadline: false,
        budget_eur: None,
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
    let dt_h = 1800.0 / 3600.0;
    let ev_energy: f64 = plan
        .slots
        .iter()
        .map(|s| s.planned_kw_by_asset.get("ev").copied().unwrap_or(0.0) * dt_h)
        .sum();
    assert!(
        ev_energy >= e_core_kwh - 0.1,
        "MustRun EV should meet {:.1} kWh core, got {:.4}",
        e_core_kwh,
        ev_energy
    );
}

#[test]
fn run_planner_power_balance_invariant() {
    // No EV session — avoids infeasibility from large battery/target gap in make_profile.
    // Battery + heater + PV (MayRun) is sufficient to exercise the balance.
    let now = fixed_now();
    let profile = make_profile_1800s();
    let mut sim = make_snap_from_profile(&profile);
    set_battery_soc(&mut sim, 0.5);
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
    for (t, slot) in plan.slots.iter().enumerate() {
        let ev_kw = slot.planned_kw_by_asset.get("ev").copied().unwrap_or(0.0);
        let heat_kw = slot
            .planned_kw_by_asset
            .get("heater")
            .copied()
            .unwrap_or(0.0);
        // p_imp + p_pv + p_dis = p_base + p_ev + p_heat + p_ch + p_exp
        let lhs = slot.net_import_kw + slot.pv_forecast_kw + slot.bat_discharge_kw;
        let rhs = slot.baseline_kw + ev_kw + heat_kw + slot.bat_charge_kw + slot.net_export_kw;
        assert!(
            (lhs - rhs).abs() < 0.1,
            "power balance violated at slot {t}: lhs={:.4} rhs={:.4}",
            lhs,
            rhs
        );
    }
}

#[test]
fn run_planner_absent_battery_no_panic() {
    let now = fixed_now();
    let mut profile = make_profile_1800s();
    profile
        .assets
        .retain(|a| !matches!(a, AssetProfile::Battery(_)));
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
    assert_eq!(plan.slots.len(), 4, "plan must have 4 slots");
    assert!(
        plan.soc_trajectory_kwh.is_empty() || plan.soc_trajectory_kwh.iter().all(|&v| v < 1e-3),
        "no battery → soc_trajectory_kwh empty or all-zero"
    );
}

// ── T019: n=48 test profile fixture (24 h / 1800 s = 48 slots) ───────────

fn make_profile_n48() -> Profile {
    Profile {
        assets: vec![
            AssetProfile::Battery(BatteryConfig {
                id: "battery".into(),
                capacity_kwh: 10.0,
                max_charge_kw: 5.0,
                max_discharge_kw: 5.0,
                initial_soc: 0.5,
                round_trip_efficiency: 0.9,
                min_soc: 0.1,
                c_terminal_eur_kwh: None,
            }),
            AssetProfile::Ev(EvConfig {
                id: "ev".into(),
                max_charge_kw: 7.2,
                max_discharge_kw: 0.0,
                initial_soc: 0.5,
                battery_kwh: 40.0,
                soc_target: 0.8,
                default_charge_kw: 0.0,
                min_charge_kw: 1.4,
                response_delay_s: 10.0,
            }),
            AssetProfile::Heater(HeaterConfig {
                id: "heater".into(),
                max_kw: 2.0,
                temp_initial_c: 20.0,
                temp_min_c: 18.0,
                temp_max_c: 23.0,
                mid_kw: Some(1.0),
                thermal_mass_kwh_per_c: 2.0,
                k_loss_kw_per_c: 0.1,
                draw_kw: 0.0,
                switching_penalty_eur: 0.01,
                c_terminal_eur_kwh: None,
            }),
            AssetProfile::Pv(PvConfig {
                id: "pv".into(),
                rated_kw: 6.0,
            }),
            AssetProfile::BaseLoad(BaseLoadConfig {
                id: "base_load".into(),
                baseline_kw: 0.5,
                spikes: vec![],
            }),
        ],
        simulator: SimulatorConfig,
        planner: PlannerConfig {
            plan_step_s: 1800,
            plan_horizon_h: 24,
            plan_zones: vec![crate::entities::plan::PlanZone {
                step_s: 1800,
                slots: 48,
            }],
            ..PlannerConfig::default()
        },
        grid: GridConfig {
            max_import_kw: 25.0,
            max_export_kw: 10.0,
        },
        packets: vec![],
    }
}

// ── T020: n=48 regression test and edge-case assertions ──────────────────

#[test]
fn run_planner_n48_full_horizon() {
    let now = fixed_now();
    let profile = make_profile_n48();
    let mut sim = make_snap_from_profile(&profile);
    set_battery_soc(&mut sim, 0.5);
    set_ev_plugged(&mut sim, true);
    let session = crate::entities::device_session::EvSession {
        mode: Default::default(),
        id: uuid::Uuid::new_v4(),
        target_soc: 0.8,
        departure_time: now + Duration::hours(24),
        soft_deadline: false,
        budget_eur: None,
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
    assert_eq!(plan.slots.len(), 48, "n=48: 24 h / 1800 s = 48 slots");
    // Battery SoC trajectory: len = n+1, values within [e_min=1.0, e_nom=10.0] kWh
    if !plan.soc_trajectory_kwh.is_empty() {
        assert_eq!(
            plan.soc_trajectory_kwh.len(),
            49,
            "SoC trajectory must have n+1=49 entries"
        );
        for (i, &soc_kwh) in plan.soc_trajectory_kwh.iter().enumerate() {
            assert!(
                soc_kwh >= 0.99,
                "SoC[{i}]={soc_kwh:.4} kWh below e_min 1.0 kWh"
            );
            assert!(
                soc_kwh <= 10.01,
                "SoC[{i}]={soc_kwh:.4} kWh above e_nom 10.0 kWh"
            );
        }
    }
    for (t, slot) in plan.slots.iter().enumerate() {
        assert!(
            slot.net_import_kw <= 25.1,
            "slot {t}: net_import_kw={:.4} kW > max_import 25 kW",
            slot.net_import_kw
        );
    }
}

// (a) empty asset_contexts → grid-only plan, no panic
#[test]
fn run_planner_n48_empty_asset_contexts_no_panic() {
    let now = fixed_now();
    let profile = make_profile_n48();
    let sim = make_snap_from_profile(&profile);
    let plan = run_planner(
        vec![],
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
    assert_eq!(plan.slots.len(), 48, "grid-only plan must have 48 slots");
}

// (b) EV in MustNotRun mode → no EV power in any slot
#[test]
fn run_planner_n48_ev_must_not_run_no_ev_in_plan() {
    use crate::services::test_support::milp_mocks::MockEvCtx;
    let now = fixed_now();
    let profile = make_profile_n48();
    let sim = make_snap_from_profile(&profile);
    let n = 48_usize;
    let ev_ctx: Box<dyn crate::controller::milp_planner::AssetMilpContext> =
        Box::new(MockEvCtx::must_not_run(n, 7.2));
    let plan = run_planner(
        vec![ev_ctx],
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
    assert_eq!(plan.slots.len(), 48);
    for (t, slot) in plan.slots.iter().enumerate() {
        let ev_power = ["ev", "mock_ev"]
            .iter()
            .map(|k| slot.planned_kw_by_asset.get(*k).copied().unwrap_or(0.0))
            .sum::<f64>();
        assert!(
            ev_power < 1e-3,
            "slot {t}: MustNotRun EV must have 0 power, got {ev_power:.4}"
        );
    }
}

// (c) duplicate asset_kind → debug_assert! panics in debug builds
#[test]
#[cfg(debug_assertions)]
#[should_panic(expected = "duplicate AssetKind")]
fn run_planner_duplicate_asset_kind_panics() {
    use crate::services::test_support::milp_mocks::MockBatteryCtx;
    let now = fixed_now();
    let profile = make_profile_n48();
    let sim = make_snap_from_profile(&profile);
    let ctxs: Vec<Box<dyn crate::controller::milp_planner::AssetMilpContext>> = vec![
        Box::new(MockBatteryCtx::new(10.0, 5.0, 1.0, 5.0, 0.9487)),
        Box::new(MockBatteryCtx::new(5.0, 2.5, 0.5, 3.0, 0.9487)), // duplicate Battery kind
    ];
    let _ = run_planner(
        ctxs,
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
}

// (d) infeasible constraints → solver Err, run_planner returns fallback plan (no panic)
#[test]
fn run_planner_infeasible_constraints_fallback_no_panic() {
    use crate::controller::milp_interactions::MilpVarPool;
    use crate::controller::milp_planner::{AssetKind, AssetMilpContext, AssetMilpParams};
    use crate::services::test_support::milp_mocks::MockBatteryCtx;
    use chrono::{DateTime, Utc};
    use good_lp::{Constraint, Expression, ProblemVariables};

    struct InfeasibleBatCtx {
        inner: MockBatteryCtx,
    }
    impl AssetMilpContext for InfeasibleBatCtx {
        fn asset_id(&self) -> &str {
            self.inner.asset_id()
        }
        fn asset_kind(&self) -> AssetKind {
            self.inner.asset_kind()
        }
        fn milp_params(&self, n: usize, now: DateTime<Utc>) -> AssetMilpParams {
            self.inner.milp_params(n, now)
        }
        fn declare_vars_into_pool(
            &self,
            n: usize,
            c_s: f64,
            c_r: f64,
            vars: &mut ProblemVariables,
            pool: &mut MilpVarPool,
        ) {
            self.inner.declare_vars_into_pool(n, c_s, c_r, vars, pool);
        }
        fn constraints(&self, pool: &MilpVarPool, n: usize, dt_h: &[f64]) -> Vec<Constraint> {
            let mut cs = self.inner.constraints(pool, n, dt_h);
            // Contradiction: require p_ch[0] ≥ 9999 while battery bounds p_ch[0] ≤ 5 kW
            if let Some(bat) = &pool.bat {
                if !bat.p_ch.is_empty() {
                    cs.push(constraint!(bat.p_ch[0] >= 9999.0));
                }
            }
            cs
        }
        fn objective(
            &self,
            pool: &MilpVarPool,
            n: usize,
            dt_h: &[f64],
            c_wear: f64,
            c_startup: f64,
            c_ramp: f64,
        ) -> Expression {
            self.inner
                .objective(pool, n, dt_h, c_wear, c_startup, c_ramp)
        }
    }

    let now = fixed_now();
    let profile = make_profile_n48();
    let sim = make_snap_from_profile(&profile);
    let infeasible: Box<dyn AssetMilpContext> = Box::new(InfeasibleBatCtx {
        inner: MockBatteryCtx::new(10.0, 5.0, 1.0, 5.0, 0.9487),
    });
    // run_planner catches the solver Err and returns a fallback plan — must not panic
    let plan = run_planner(
        vec![infeasible],
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
    assert_eq!(plan.slots.len(), 48, "fallback plan must have 48 slots");
}

// ── WP3.1 (BL-04): grid-alert windows ────────────────────────────────────

#[test]
fn alert_window_clamps_import_cap_for_overlapping_slots_only() {
    use crate::entities::capacity::AlertWindow;

    let now = fixed_now();
    let profile = make_profile_1800s(); // 4 × 1800s slots, 2h horizon
    let sim = make_snap_from_profile(&profile);
    let tariffs = make_tariffs(0.25, 0.08, 300.0);
    let cap = no_capacity();
    let ctxs = build_asset_contexts(&profile, &sim, now, None, None, &tariffs);

    // Alert covers the first hour → slots 0 and 1; slots 2 and 3 stay free.
    let alert = AlertWindow {
        alert_type: "ALERT_GRID_EMERGENCY".to_string(),
        start: now,
        end: now + Duration::hours(1),
        event_id: "alert-1".to_string(),
        message: "grid emergency".to_string(),
    };

    let inputs = super::super::inputs::build_milp_inputs(
        &ctxs,
        &sim,
        &tariffs,
        &cap,
        std::slice::from_ref(&alert),
        &[],
        &profile.planner,
        profile.grid.max_import_kw,
        profile.grid.max_export_kw,
        profile.pv_config(),
        None,
        now,
        &[],
        None,
        None,
        &std::collections::HashMap::new(),
    );

    assert_eq!(inputs.p_imp_max_cont_kw[0], 0.0, "slot 0 inside alert");
    assert_eq!(inputs.p_imp_max_cont_kw[1], 0.0, "slot 1 inside alert");
    assert!(
        inputs.p_imp_max_cont_kw[2] > 0.0,
        "slot 2 after alert must keep the contractual cap"
    );
    assert!(inputs.p_imp_max_cont_kw[3] > 0.0);
    // Export side untouched by the alert.
    assert!(inputs.p_exp_max_cont_kw.iter().all(|&v| v > 0.0));
}

#[test]
fn run_planner_alert_window_yields_zero_import_cap_slots_and_solves() {
    use crate::entities::capacity::AlertWindow;

    let now = fixed_now();
    let profile = make_profile_1800s();
    let sim = make_snap_from_profile(&profile);
    let tariffs = make_tariffs(0.25, 0.08, 300.0);
    let ctxs = build_asset_contexts(&profile, &sim, now, None, None, &tariffs);

    let alert = AlertWindow {
        alert_type: "ALERT_GRID_EMERGENCY".to_string(),
        start: now,
        end: now + Duration::hours(1),
        event_id: "alert-1".to_string(),
        message: "grid emergency".to_string(),
    };

    // Call the real entry point (not the alert-less test wrapper) so the alert
    // path is exercised end-to-end through a genuine HiGHS solve.
    let plan = super::super::run_planner(
        ctxs,
        &sim,
        &tariffs,
        &no_capacity(),
        std::slice::from_ref(&alert),
        &[],
        &profile.planner,
        profile.grid.max_import_kw,
        profile.grid.max_export_kw,
        &profile.assets,
        now,
        crate::entities::asset::PlanTrigger::Alert,
        None,
        None,
        &[],
        None,
        None,
        None,
        &std::collections::HashMap::new(),
    );

    assert_eq!(plan.slots.len(), 4);
    assert_eq!(plan.slots[0].import_cap_kw, 0.0);
    assert_eq!(plan.slots[1].import_cap_kw, 0.0);
    assert!(plan.slots[2].import_cap_kw > 0.0);
    // The solve must not degrade to the fallback path: a real solution has a
    // battery/EV/heater allocation somewhere or at least no "solver failed"
    // critical warning mentioning failure.
    assert!(
        !plan
            .warnings
            .iter()
            .any(|w| w.message.contains("MILP solver failed")),
        "alert must not make the solve fail outright: {:?}",
        plan.warnings
    );
}

// ── WP3.2: SIMPLE levels 0–3 ─────────────────────────────────────────────

#[test]
fn simple_levels_clamp_import_cap_per_level_and_alert_overrides() {
    use crate::entities::capacity::{AlertWindow, SimpleWindow};

    let now = fixed_now();
    let profile = make_profile_1800s(); // 4 × 1800s slots; grid contractual 25 kW
    let sim = make_snap_from_profile(&profile);
    let tariffs = make_tariffs(0.25, 0.08, 300.0);
    let cap = no_capacity();
    let ctxs = build_asset_contexts(&profile, &sim, now, None, None, &tariffs);

    let win = |level: u8, from_slot: i64, slots: i64| SimpleWindow {
        level,
        start: now + Duration::seconds(from_slot * 1800),
        end: now + Duration::seconds((from_slot + slots) * 1800),
        event_id: format!("simple-l{level}"),
    };
    // Slot 0: level 1; slot 1: levels 1 AND 2 overlap (2 wins); slot 2: level 3.
    let simple = vec![win(1, 0, 2), win(2, 1, 1), win(3, 2, 1)];

    let inputs = super::super::inputs::build_milp_inputs(
        &ctxs,
        &sim,
        &tariffs,
        &cap,
        &[],
        &simple,
        &profile.planner,
        profile.grid.max_import_kw,
        profile.grid.max_export_kw,
        profile.pv_config(),
        profile.assets.iter().find_map(|a| match a {
            AssetProfile::BaseLoad(v) => Some(v),
            _ => None,
        }),
        now,
        &[],
        None,
        None,
        &std::collections::HashMap::new(),
    );

    // L1: 50% of contractual 25 kW (default simple_level1_import_cap_pct).
    assert!((inputs.p_imp_max_cont_kw[0] - 12.5).abs() < 1e-9);
    // L2 (overlap with L1 — highest wins): baseline forecast 0.5 kW.
    assert!((inputs.p_imp_max_cont_kw[1] - 0.5).abs() < 1e-9);
    // L3: zero.
    assert_eq!(inputs.p_imp_max_cont_kw[2], 0.0);
    // Slot 3: no window — contractual limit untouched.
    assert!((inputs.p_imp_max_cont_kw[3] - 25.0).abs() < 1e-9);

    // Alert overrides a mild SIMPLE level on the same slot.
    let alert = AlertWindow {
        alert_type: "ALERT_GRID_EMERGENCY".to_string(),
        start: now,
        end: now + Duration::seconds(1800),
        event_id: "alert-1".to_string(),
        message: String::new(),
    };
    let inputs2 = super::super::inputs::build_milp_inputs(
        &build_asset_contexts(&profile, &sim, now, None, None, &tariffs),
        &sim,
        &tariffs,
        &cap,
        std::slice::from_ref(&alert),
        &[win(1, 0, 1)],
        &profile.planner,
        profile.grid.max_import_kw,
        profile.grid.max_export_kw,
        profile.pv_config(),
        None,
        now,
        &[],
        None,
        None,
        &std::collections::HashMap::new(),
    );
    assert_eq!(
        inputs2.p_imp_max_cont_kw[0], 0.0,
        "alert wins over SIMPLE L1"
    );
}

// ── WP3.3 (§8.10): subscription + reservation constrain the solver ───────

#[test]
fn reservation_allowance_binds_when_tighter_and_is_inactive_when_looser() {
    let now = fixed_now();
    let profile = make_profile_1800s(); // contractual/physical import 25 kW
    let sim = make_snap_from_profile(&profile);
    let tariffs = make_tariffs(0.25, 0.08, 300.0);
    let ctxs = build_asset_contexts(&profile, &sim, now, None, None, &tariffs);

    // Reservation alone (no subscription), tighter than the 25 kW limit → binds.
    let mut cap = no_capacity();
    cap.import_reservation_kw = Some(3.0);
    let inputs = bmi(&profile, &sim, &tariffs, &cap, now, None, None);
    assert!(inputs
        .p_imp_max_cont_kw
        .iter()
        .all(|&v| (v - 3.0).abs() < 1e-9));
    let _ = ctxs;

    // Subscription 20 + reservation 10 = 30 kW allowance, looser than the
    // 25 kW physical bound → inactive.
    let mut cap = no_capacity();
    cap.import_subscription_kw = Some(20.0);
    cap.import_reservation_kw = Some(10.0);
    let inputs = bmi(&profile, &sim, &tariffs, &cap, now, None, None);
    assert!(inputs
        .p_imp_max_cont_kw
        .iter()
        .all(|&v| (v - 25.0).abs() < 1e-9));

    // Export side: subscription 4 kW binds against the 10 kW physical bound.
    let mut cap = no_capacity();
    cap.export_subscription_kw = Some(4.0);
    let inputs = bmi(&profile, &sim, &tariffs, &cap, now, None, None);
    assert!(inputs
        .p_exp_max_cont_kw
        .iter()
        .all(|&v| (v - 4.0).abs() < 1e-9));
}

// ── WP4.6 review: deterministic earliest-start for shiftable loads ───────────

fn make_shiftable(
    now: DateTime<Utc>,
    dur_min: u32,
    window_min: i64,
) -> crate::entities::device_session::ShiftableLoad {
    crate::entities::device_session::ShiftableLoad {
        id: uuid::Uuid::new_v4(),
        asset_id: "wm".to_string(),
        power_kw: 2.0,
        duration_min: dur_min,
        earliest_start: now,
        latest_end: now + Duration::minutes(window_min),
        mode: Default::default(),
        created_at: now,
        updated_at: now,
    }
}

fn wm_kw_per_slot(plan: &crate::entities::plan::Plan) -> Vec<f64> {
    plan.slots
        .iter()
        .map(|s| {
            s.allocations
                .iter()
                .find(|a| a.asset_id == "wm")
                .map(|a| a.power_kw)
                .unwrap_or(0.0)
        })
        .collect()
}

/// Two cost-equal start slots (the window straddles a slot boundary because
/// slot grids start at the ALIGNED now, not the wall now) must start the
/// load in the EARLIEST slot — the E2E flake root cause: HiGHS was free to
/// pick the future slot and the load "never appeared" within the poll window.
#[test]
fn run_planner_shiftable_tie_breaks_to_earliest_start() {
    let now = fixed_now();
    let mut profile = make_profile_1800s();
    profile
        .assets
        .retain(|a| matches!(a, AssetProfile::BaseLoad(_)));
    let sim = make_snap_from_profile(&profile);
    let tariffs = make_tariffs(0.25, 0.08, 300.0); // flat → slots 0 and 1 cost-equal
                                                   // 30-min load, 65-min window → valid start slots {0, 1} on the 1800 s grid.
    let load = make_shiftable(now, 30, 65);
    let plan = run_planner(
        build_asset_contexts(&profile, &sim, now, None, None, &tariffs),
        &sim,
        &tariffs,
        &no_capacity(),
        &profile,
        now,
        crate::entities::asset::PlanTrigger::UserRequest,
        None,
        None,
        &[load],
        None,
        None,
    );
    let wm = wm_kw_per_slot(&plan);
    assert!(
        wm[0] > 1.9,
        "cost-equal starts must resolve to the earliest slot, got {wm:?}"
    );
}

/// The tie-break is a tie-break only: a genuinely cheaper later slot still wins.
#[test]
fn run_planner_shiftable_still_defers_for_real_savings() {
    let now = fixed_now();
    let mut profile = make_profile_1800s();
    profile
        .assets
        .retain(|a| matches!(a, AssetProfile::BaseLoad(_)));
    let sim = make_snap_from_profile(&profile);
    // Slot 0 expensive, slot 1 cheap (30-min intervals).
    let tariffs = TariffTimeSeries::from_snapshots(&[
        TariffSnapshot {
            interval_start: now,
            interval_end: now + Duration::minutes(30),
            import_tariff_eur_kwh: Some(0.40),
            export_tariff_eur_kwh: Some(0.08),
            co2_g_kwh: Some(300.0),
        },
        TariffSnapshot {
            interval_start: now + Duration::minutes(30),
            interval_end: now + Duration::hours(3),
            import_tariff_eur_kwh: Some(0.05),
            export_tariff_eur_kwh: Some(0.08),
            co2_g_kwh: Some(300.0),
        },
    ]);
    let load = make_shiftable(now, 30, 65);
    let plan = run_planner(
        build_asset_contexts(&profile, &sim, now, None, None, &tariffs),
        &sim,
        &tariffs,
        &no_capacity(),
        &profile,
        now,
        crate::entities::asset::PlanTrigger::UserRequest,
        None,
        None,
        &[load],
        None,
        None,
    );
    let wm = wm_kw_per_slot(&plan);
    assert!(
        wm[1] > 1.9 && wm[0] < 1e-3,
        "a 0.35 €/kWh saving must beat the 0.001 €/slot tie-break, got {wm:?}"
    );
}
