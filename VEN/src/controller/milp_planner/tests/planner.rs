use super::*;

// ── run_planner() regression-guard tests ─────────────────────────────────
// These call only the public run_planner() entry point so they remain valid
// through any internal refactor. Use 1800s steps (4 slots, 2h horizon) for
// fast solver runs (~<100ms each).

fn make_profile_1800s() -> Profile {
    let mut p = make_profile();
    p.planner.plan_step_s = 1800;
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
        })],
        simulator: SimulatorConfig::default(),
        planner: PlannerConfig {
            plan_step_s: 1800,
            plan_horizon_h: 2,
            ..PlannerConfig::default()
        },
        grid: GridConfig {
            max_import_kw: 25.0,
            max_export_kw: 10.0,
        },
        packets: vec![],
    };
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
fn run_planner_battery_absent_no_bat_allocation() {
    let now = fixed_now();
    let mut profile = make_profile_1800s();
    profile
        .assets
        .retain(|a| !matches!(a, AssetProfile::Battery(_)));
    let mut sim = make_snap_from_profile(&profile);
    set_ev_plugged(&mut sim, true);
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
            }),
        ],
        simulator: SimulatorConfig::default(),
        planner: PlannerConfig {
            plan_step_s: 1800,
            plan_horizon_h: 24,
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
        id: uuid::Uuid::new_v4(),
        target_soc: 0.8,
        departure_time: now + Duration::hours(24),
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
        fn milp_params(&self, n: usize, step_s: u64, now: DateTime<Utc>) -> AssetMilpParams {
            self.inner.milp_params(n, step_s, now)
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
        fn constraints(&self, pool: &MilpVarPool, n: usize, dt_h: f64) -> Vec<Constraint> {
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
            dt_h: f64,
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
