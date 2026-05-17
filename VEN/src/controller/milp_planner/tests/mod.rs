    use super::*;

    use crate::assets::{
        battery::{Battery, BatteryState},
        ev::{EvCharger, EvState}, heater::{Heater, HeaterState},
        AssetConfig, AssetState,
    };
    use crate::entities::asset_params::{
        BaseLoadParams, BatteryParams, EvParams, HeaterParams, PvParams,
    };
    use crate::controller::simulator_port::{AssetSnapshot, GridSnapshot, SimSnapshot};
    use crate::entities::asset_params::AssetParams;
    use crate::entities::device_session::HeaterTarget;
    use crate::entities::planner_params::{PlannerObjective, PlannerParams};
    use crate::entities::tariff_snapshot::TariffSnapshot;

    type AssetProfile = AssetParams;
    type BatteryConfig = BatteryParams;
    type EvConfig = EvParams;
    type HeaterConfig = HeaterParams;
    type PvConfig = PvParams;
    type BaseLoadConfig = BaseLoadParams;
    type PlannerConfig = PlannerParams;

    #[derive(Debug, Clone, Default)]
    struct SimulatorConfig;

    #[derive(Debug, Clone, Default)]
    struct AbsorberConfig;

    #[derive(Debug, Clone)]
    struct GridConfig {
        max_import_kw: f64,
        max_export_kw: f64,
    }

    #[derive(Debug, Clone)]
    struct Profile {
        assets: Vec<AssetProfile>,
        simulator: SimulatorConfig,
        planner: PlannerConfig,
        grid: GridConfig,
        packets: Vec<()>,
        absorber: AbsorberConfig,
    }

    impl Profile {
        fn ev_config(&self) -> Option<&EvParams> {
            self.assets.iter().find_map(|a| match a {
                AssetProfile::Ev(v) => Some(v),
                _ => None,
            })
        }

        fn heater_config(&self) -> Option<&HeaterParams> {
            self.assets.iter().find_map(|a| match a {
                AssetProfile::Heater(v) => Some(v),
                _ => None,
            })
        }

        fn pv_config(&self) -> Option<&PvParams> {
            self.assets.iter().find_map(|a| match a {
                AssetProfile::Pv(v) => Some(v),
                _ => None,
            })
        }

        fn battery_config(&self) -> Option<&BatteryParams> {
            self.assets.iter().find_map(|a| match a {
                AssetProfile::Battery(v) => Some(v),
                _ => None,
            })
        }

        fn base_load_kw(&self) -> f64 {
            self.assets
                .iter()
                .find_map(|a| match a {
                    AssetProfile::BaseLoad(v) => Some(v.baseline_kw),
                    _ => None,
                })
                .unwrap_or(0.0)
        }
    }

    fn fixed_now() -> DateTime<Utc> {
        use chrono::TimeZone;
        Utc.with_ymd_and_hms(2026, 4, 11, 6, 0, 0).unwrap()
    }

    fn make_tariffs(imp: f64, exp: f64, co2_g: f64) -> TariffTimeSeries {
        let now = fixed_now();
        let snap = TariffSnapshot {
            interval_start: now - Duration::hours(1),
            interval_end: now + Duration::hours(25),
            import_tariff_eur_kwh: Some(imp),
            export_tariff_eur_kwh: Some(exp),
            co2_g_kwh: Some(co2_g),
        };
        TariffTimeSeries::from_snapshots(&[snap])
    }

    fn no_capacity() -> OadrCapacityState {
        OadrCapacityState {
            import_limit_kw: None,
            export_limit_kw: None,
            import_subscription_kw: None,
            import_reservation_kw: None,
            import_limit_event_id: None,
            export_limit_event_id: None,
            last_updated: None,
        }
    }

    fn make_profile() -> Profile {
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
                    max_charge_kw: 7.4,
                    max_discharge_kw: 0.0,
                    initial_soc: 0.2,
                    battery_kwh: 60.0,
                    soc_target: 0.8,
                    default_charge_kw: 0.0,
                    min_charge_kw: 1.4,
                }),
                AssetProfile::Heater(HeaterConfig {
                    id: "heater".into(),
                    max_kw: 3.0,
                    temp_initial_c: 20.0,
                    temp_min_c: 18.0,
                    temp_max_c: 23.0,
                    mid_kw: None,
                    thermal_mass_kwh_per_c: 2.0,
                    k_loss_kw_per_c: 0.1,
                    draw_kw: 0.0,
                    switching_penalty_eur: 0.01,
                }),
                AssetProfile::Pv(PvConfig {
                    id: "pv".into(),
                    rated_kw: 5.0,
                }),
                AssetProfile::BaseLoad(BaseLoadConfig {
                    id: "base_load".into(),
                    baseline_kw: 0.5,
                }),
            ],
            simulator: SimulatorConfig::default(),
            planner: PlannerConfig {
                plan_step_s: 300,
                plan_horizon_h: 2,
                ..PlannerConfig::default()
            },
            grid: GridConfig {
                max_import_kw: 25.0,
                max_export_kw: 10.0,
            },
            packets: vec![],
            absorber: AbsorberConfig::default(),
        }
    }

    fn make_snap_from_profile(profile: &Profile) -> SimSnapshot {
        use std::collections::HashMap as HM;

        let mut assets: HM<String, AssetSnapshot> = HM::new();

        for asset in &profile.assets {
            match asset {
                AssetProfile::Battery(cfg) => {
                    let soc = cfg.initial_soc;
                    let cap = cfg.capacity_kwh;
                    let min_soc = cfg.min_soc;
                    let max_ch = cfg.max_charge_kw;
                    let max_dis = cfg.max_discharge_kw;
                    let eff = cfg.round_trip_efficiency;
                    let cap_max_export_kw = if soc <= min_soc { 0.0 } else { -max_dis };
                    let cap_max_import_kw = if soc >= 1.0 { 0.0 } else { max_ch };
                    let mut values = HM::new();
                    values.insert("soc".into(), soc);
                    values.insert("capacity_kwh".into(), cap);
                    values.insert("max_charge_kw".into(), max_ch);
                    values.insert("max_discharge_kw".into(), max_dis);
                    values.insert("min_soc".into(), min_soc);
                    values.insert("round_trip_efficiency".into(), eff);
                    assets.insert(
                        "battery".to_string(),
                        AssetSnapshot {
                            power_kw: 0.0,
                            asset_type: "battery".to_string(),
                            cap_max_import_kw,
                            cap_max_export_kw,
                            available_discharge_kwh: Some((soc - min_soc).max(0.0) * cap),
                            available_charge_kwh: Some((1.0 - soc).max(0.0) * cap),
                            default_setpoint_kw: 0.0,
                            setpoint_kw: 0.0,
                            values,
                        },
                    );
                }
                AssetProfile::Ev(cfg) => {
                    let soc = cfg.initial_soc;
                    let bat_kwh = cfg.battery_kwh;
                    let max_ch = cfg.max_charge_kw;
                    let soc_target = cfg.soc_target;
                    let cap_max_import_kw = if soc >= soc_target { 0.0 } else { max_ch };
                    let mut values = HM::new();
                    values.insert("soc".into(), soc);
                    values.insert("plugged".into(), 1.0);
                    values.insert("max_charge_kw".into(), max_ch);
                    values.insert("soc_target".into(), soc_target);
                    values.insert("battery_kwh".into(), bat_kwh);
                    assets.insert(
                        "ev".to_string(),
                        AssetSnapshot {
                            power_kw: 0.0,
                            asset_type: "ev".to_string(),
                            cap_max_import_kw,
                            cap_max_export_kw: 0.0,
                            available_discharge_kwh: Some(soc * bat_kwh),
                            available_charge_kwh: Some((1.0 - soc) * bat_kwh),
                            default_setpoint_kw: cfg.default_charge_kw,
                            setpoint_kw: 0.0,
                            values,
                        },
                    );
                }
                AssetProfile::Heater(cfg) => {
                    let temp_c = cfg.temp_initial_c;
                    let max_kw = cfg.max_kw;
                    let mid_kw = cfg.mid_kw.unwrap_or(max_kw / 2.0);
                    let mut values = HM::new();
                    values.insert("temp_c".into(), temp_c);
                    values.insert("max_kw".into(), max_kw);
                    values.insert("mid_kw".into(), mid_kw);
                    values.insert("temp_min_c".into(), cfg.temp_min_c);
                    values.insert("temp_max_c".into(), cfg.temp_max_c);
                    let cap_max_import_kw = if temp_c >= cfg.temp_max_c { 0.0 } else { max_kw };
                    assets.insert(
                        "heater".to_string(),
                        AssetSnapshot {
                            power_kw: 0.0,
                            asset_type: "heater".to_string(),
                            cap_max_import_kw,
                            cap_max_export_kw: 0.0,
                            available_discharge_kwh: None,
                            available_charge_kwh: None,
                            default_setpoint_kw: 0.0,
                            setpoint_kw: 0.0,
                            values,
                        },
                    );
                }
                AssetProfile::Pv(cfg) => {
                    let mut values = HM::new();
                    values.insert("irradiance".into(), 0.0);
                    values.insert("rated_kw".into(), cfg.rated_kw);
                    values.insert("irradiance_offset".into(), 0.0);
                    values.insert("pv_alpha".into(), 0.1);
                    assets.insert(
                        "pv".to_string(),
                        AssetSnapshot {
                            power_kw: 0.0,
                            asset_type: "pv".to_string(),
                            cap_max_import_kw: 0.0,
                            cap_max_export_kw: 0.0,
                            available_discharge_kwh: None,
                            available_charge_kwh: None,
                            default_setpoint_kw: 0.0,
                            setpoint_kw: 0.0,
                            values,
                        },
                    );
                }
                AssetProfile::BaseLoad(cfg) => {
                    let mut values = HM::new();
                    values.insert("baseline_kw".into(), cfg.baseline_kw);
                    assets.insert(
                        "base_load".to_string(),
                        AssetSnapshot {
                            power_kw: 0.0,
                            asset_type: "base_load".to_string(),
                            cap_max_import_kw: cfg.baseline_kw,
                            cap_max_export_kw: cfg.baseline_kw,
                            available_discharge_kwh: None,
                            available_charge_kwh: None,
                            default_setpoint_kw: cfg.baseline_kw,
                            setpoint_kw: 0.0,
                            values,
                        },
                    );
                }
            }
        }

        SimSnapshot {
            ts: chrono::Utc::now(),
            grid: GridSnapshot {
                net_power_w: 0.0,
                voltage_v: 230.0,
                import_kwh: 0.0,
                export_kwh: 0.0,
            },
            assets,
        }
    }

    fn set_ev_plugged(snap: &mut SimSnapshot, plugged: bool) {
        if let Some(ev) = snap.assets.get_mut("ev") {
            let soc = ev.val("soc").unwrap_or(0.0);
            let max_ch = ev.val("max_charge_kw").unwrap_or(0.0);
            let soc_target = ev.val("soc_target").unwrap_or(1.0);
            let bat_kwh = ev.val("battery_kwh").unwrap_or(0.0);
            ev.values.insert("plugged".into(), if plugged { 1.0 } else { 0.0 });
            if plugged {
                ev.cap_max_import_kw = if soc >= soc_target { 0.0 } else { max_ch };
                ev.cap_max_export_kw = 0.0;
                ev.available_discharge_kwh = Some(soc * bat_kwh);
                ev.available_charge_kwh = Some((1.0 - soc) * bat_kwh);
            } else {
                ev.cap_max_import_kw = 0.0;
                ev.cap_max_export_kw = 0.0;
                ev.available_discharge_kwh = None;
                ev.available_charge_kwh = None;
            }
        }
    }

    fn set_battery_soc(snap: &mut SimSnapshot, soc: f64) {
        if let Some(bat) = snap.assets.get_mut("battery") {
            let cap = bat.val("capacity_kwh").unwrap_or(0.0);
            let max_ch = bat.val("max_charge_kw").unwrap_or(0.0);
            let max_dis = bat.val("max_discharge_kw").unwrap_or(0.0);
            let min_soc = bat.val("min_soc").unwrap_or(0.0);
            bat.values.insert("soc".into(), soc);
            bat.cap_max_import_kw = if soc >= 1.0 { 0.0 } else { max_ch };
            bat.cap_max_export_kw = if soc <= min_soc { 0.0 } else { -max_dis };
            bat.available_discharge_kwh = Some((soc - min_soc).max(0.0) * cap);
            bat.available_charge_kwh = Some((1.0 - soc).max(0.0) * cap);
        }
    }

    fn set_heater_temp(snap: &mut SimSnapshot, temp_c: f64) {
        if let Some(h) = snap.assets.get_mut("heater") {
            h.values.insert("temp_c".into(), temp_c);
        }
    }

    fn make_heater_only_profile(
        volume_l: Option<f64>,
        temp_min_c: f64,
        temp_max_c: f64,
        temp_initial_c: f64,
    ) -> Profile {
        let thermal_mass = volume_l.map(|v| v * 4.186 / 3600.0).unwrap_or(2.0);
        Profile {
            assets: vec![AssetProfile::Heater(HeaterConfig {
                id: "heater".into(),
                max_kw: 3.0,
                temp_initial_c,
                temp_min_c,
                temp_max_c,
                mid_kw: None,
                thermal_mass_kwh_per_c: thermal_mass,
                k_loss_kw_per_c: 0.1,
                draw_kw: 0.0,
                switching_penalty_eur: 0.01,
            })],
            simulator: SimulatorConfig::default(),
            planner: PlannerConfig {
                plan_step_s: 300,
                plan_horizon_h: 2,
                ..PlannerConfig::default()
            },
            grid: GridConfig { max_import_kw: 25.0, max_export_kw: 10.0 },
            packets: vec![],
            absorber: AbsorberConfig::default(),
        }
    }

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

    fn build_asset_contexts(
        profile: &Profile,
        snap: &SimSnapshot,
        now: DateTime<Utc>,
        ev_session: Option<&crate::entities::device_session::EvSession>,
        heater_target: Option<&crate::entities::device_session::HeaterTarget>,
    ) -> Vec<Box<dyn crate::controller::milp_planner::AssetMilpContext>> {
        let step_s = profile.planner.plan_step_s;
        let n = (profile.planner.plan_horizon_h as f64 * 3600.0 / step_s as f64) as usize;
        let lambda_sw = profile
            .heater_config()
            .map(|h| h.switching_penalty_eur)
            .unwrap_or(0.0);
        let v_ev_extra = profile.planner.v_ev_extra_eur_kwh;
        let ev_min_kw = profile.ev_config().map(|e| e.min_charge_kw).unwrap_or(0.0);

        let mut ctxs: Vec<Box<dyn crate::controller::milp_planner::AssetMilpContext>> = Vec::new();
        for ap in &profile.assets {
            match ap {
                AssetProfile::Battery(cfg) => {
                    let soc = snap.assets.get("battery").and_then(|s| s.val("soc")).unwrap_or(cfg.initial_soc);
                    let state = AssetState::Battery(BatteryState { soc, actual_power_kw: 0.0 });
                    let ac = AssetConfig::Battery(Battery::from_params(cfg));
                    if let Some(ctx) = ac.build_milp_context(&state, n, step_s, now, ev_session, heater_target, ev_min_kw, v_ev_extra, lambda_sw) {
                        ctxs.push(ctx);
                    }
                }
                AssetProfile::Ev(cfg) => {
                    let soc = snap.assets.get("ev").and_then(|s| s.val("soc")).unwrap_or(cfg.initial_soc);
                    let plugged = snap.assets.get("ev").and_then(|s| s.val("plugged")).map(|v| v > 0.5).unwrap_or(true);
                    let state = AssetState::Ev(EvState { soc, actual_power_kw: 0.0, plugged });
                    let ac = AssetConfig::Ev(EvCharger::from_params(cfg));
                    if let Some(ctx) = ac.build_milp_context(&state, n, step_s, now, ev_session, heater_target, ev_min_kw, v_ev_extra, lambda_sw) {
                        ctxs.push(ctx);
                    }
                }
                AssetProfile::Heater(cfg) => {
                    let temp_c = snap.assets.get("heater").and_then(|s| s.val("temp_c")).unwrap_or(cfg.temp_initial_c);
                    let state = AssetState::Heater(HeaterState { temperature_c: temp_c, actual_power_kw: 0.0 });
                    let ac = AssetConfig::Heater(Heater::from_params(cfg));
                    if let Some(ctx) = ac.build_milp_context(&state, n, step_s, now, ev_session, heater_target, ev_min_kw, v_ev_extra, lambda_sw) {
                        ctxs.push(ctx);
                    }
                }
                _ => {}
            }
        }
        ctxs
    }

    fn contexts_from_inputs(inputs: &MilpInputs) -> Vec<Box<dyn crate::controller::milp_planner::AssetMilpContext>> {
        use crate::controller::milp_planner::asset_port::{EvMilpContext, EvMilpMode, HeaterMilpContext, HeaterMilpMode};
        use crate::services::test_support::milp_mocks::{MockBatteryCtx, MockEvCtx, MockHeaterCtx};
        let mut v: Vec<Box<dyn crate::controller::milp_planner::AssetMilpContext>> = Vec::new();
        if let Some(e_nom) = inputs.e_bat_nom_kwh {
            v.push(Box::new(MockBatteryCtx::new(
                e_nom,
                inputs.e_bat_init_kwh.unwrap_or(0.0),
                inputs.e_bat_min_kwh.unwrap_or(0.0),
                inputs.p_bat_ch_max_kw.unwrap_or(0.0).max(inputs.p_bat_dis_max_kw.unwrap_or(0.0)),
                inputs.eff_bat_ch.unwrap_or(1.0),
            )));
        }
        if inputs.p_ev_max_kw > 0.0 {
            let mode = match inputs.ev_mode {
                MilpLoadMode::MustRun => EvMilpMode::MustRun,
                MilpLoadMode::MayRun => EvMilpMode::MayRun,
                MilpLoadMode::MustNotRun => EvMilpMode::MustNotRun,
            };
            v.push(Box::new(MockEvCtx { ctx: EvMilpContext {
                mode,
                a_ev: inputs.a_ev.clone(),
                t_dead_step: inputs.t_ev_dead_step,
                p_max_kw: inputs.p_ev_max_kw,
                p_min_kw: inputs.p_ev_min_kw,
                e_core_kwh: inputs.e_ev_core_kwh,
                e_extra_max_kwh: inputs.e_ev_extra_max_kwh,
                v_extra_eur_kwh: inputs.v_ev_extra_eur_kwh,
            }}));
        }
        if inputs.p_heat_full_kw > 0.0 {
            let mode = match inputs.heater_mode {
                MilpLoadMode::MustRun => HeaterMilpMode::MustRun,
                MilpLoadMode::MayRun => HeaterMilpMode::MayRun,
                MilpLoadMode::MustNotRun => HeaterMilpMode::MustNotRun,
            };
            v.push(Box::new(MockHeaterCtx { ctx: HeaterMilpContext {
                mode,
                t_dead_step: inputs.t_heat_dead_step,
                p_mid_kw: inputs.p_heat_mid_kw,
                p_full_kw: inputs.p_heat_full_kw,
                e_init_kwh: inputs.e_heat_init_kwh,
                e_max_kwh: inputs.e_heat_max_kwh,
                q_dem_kw: inputs.q_heat_dem_kw,
                e_target_kwh: inputs.e_heat_target_kwh,
                lambda_sw_eur: inputs.lambda_heat_sw_eur,
                initial_z_mid: inputs.heat_initial_z_mid,
                initial_z_full: inputs.heat_initial_z_full,
            }}));
        }
        v
    }

    fn build_phase1_weights(profile: &Profile, objective: PlannerObjective) -> Phase1Weights {
        super::build_phase1_weights(&profile.planner, objective)
    }

    fn build_milp_inputs(
        ctxs: &[Box<dyn crate::controller::milp_planner::AssetMilpContext>],
        sim: &SimSnapshot,
        tariffs: &TariffTimeSeries,
        cap: &OadrCapacityState,
        profile: &Profile,
        now: DateTime<Utc>,
        shiftable_loads: &[crate::entities::device_session::ShiftableLoad],
        baseline_override: Option<&crate::entities::device_session::BaselineOverride>,
    ) -> MilpInputs {
        build_milp_inputs_with_override(ctxs, sim, tariffs, cap, profile, now, shiftable_loads, baseline_override, None)
    }

    fn build_milp_inputs_with_override(
        ctxs: &[Box<dyn crate::controller::milp_planner::AssetMilpContext>],
        sim: &SimSnapshot,
        tariffs: &TariffTimeSeries,
        cap: &OadrCapacityState,
        profile: &Profile,
        now: DateTime<Utc>,
        shiftable_loads: &[crate::entities::device_session::ShiftableLoad],
        baseline_override: Option<&crate::entities::device_session::BaselineOverride>,
        pv_forecast_override: Option<f64>,
    ) -> MilpInputs {
        super::build_milp_inputs(
            ctxs,
            sim,
            tariffs,
            cap,
            &profile.planner,
            profile.grid.max_import_kw,
            profile.grid.max_export_kw,
            profile.pv_config(),
            profile.assets.iter().find_map(|a| match a {
                AssetProfile::BaseLoad(v) => Some(v),
                _ => None,
            }),
            now,
            shiftable_loads,
            baseline_override,
            pv_forecast_override,
        )
    }

    fn run_planner(
        asset_contexts: Vec<Box<dyn crate::controller::milp_planner::AssetMilpContext>>,
        assets: &SimSnapshot,
        tariffs: &TariffTimeSeries,
        capacity: &OadrCapacityState,
        profile: &Profile,
        now: DateTime<Utc>,
        trigger: PlanTrigger,
        ev_session: Option<&crate::entities::device_session::EvSession>,
        heater_target: Option<&crate::entities::device_session::HeaterTarget>,
        shiftable_loads: &[crate::entities::device_session::ShiftableLoad],
        baseline_override: Option<&crate::entities::device_session::BaselineOverride>,
        objective_override: Option<PlannerObjective>,
    ) -> Plan {
        super::run_planner(
            asset_contexts,
            assets,
            tariffs,
            capacity,
            &profile.planner,
            profile.grid.max_import_kw,
            profile.grid.max_export_kw,
            &profile.assets,
            now,
            trigger,
            ev_session,
            heater_target,
            shiftable_loads,
            baseline_override,
            objective_override,
            None,
        )
    }

    fn bmi(
        profile: &Profile,
        sim: &SimSnapshot,
        tariffs: &TariffTimeSeries,
        cap: &OadrCapacityState,
        now: DateTime<Utc>,
        ev_session: Option<&crate::entities::device_session::EvSession>,
        heater_target: Option<&crate::entities::device_session::HeaterTarget>,
    ) -> MilpInputs {
        let ctxs = build_asset_contexts(profile, sim, now, ev_session, heater_target);
        build_milp_inputs(&ctxs, sim, tariffs, cap, profile, now, &[], None)
    }

    mod basic;
    mod solver;
    mod pv;
    mod planner;
    mod heater;
