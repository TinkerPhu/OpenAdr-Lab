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
            grid: crate::profile::GridConfig {
                max_import_kw: 25.0,
                max_export_kw: 10.0,
            },
            packets: vec![],
            absorber: Default::default(),
        };
        let sim = make_snap_from_profile(&profile);
        let plan = run_planner(
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

