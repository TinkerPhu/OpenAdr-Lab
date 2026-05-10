    use super::*;

    use crate::controller::simulator_port::{AssetSnapshot, GridSnapshot, SimSnapshot};
    use crate::entities::device_session::HeaterTarget;
    use crate::entities::tariff_snapshot::TariffSnapshot;
    use crate::profile::{
        AssetProfile, BaseLoadConfig, BatteryConfig, EvConfig, GridConfig, HeaterConfig,
        PlannerConfig, PlannerObjective, PvConfig, SimulatorConfig,
    };

    // ── Test helpers ─────────────────────────────────────────────────────────

    fn fixed_now() -> DateTime<Utc> {
        use chrono::TimeZone;
        Utc.with_ymd_and_hms(2026, 4, 11, 6, 0, 0).unwrap()
    }

    /// Build a TariffTimeSeries with a single constant interval covering the full horizon.
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

    /// Build a Profile with battery + EV + heater + PV + base_load.
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
                    volume_l: None,
                    thermal_mass_kwh_per_c: None,
                    k_loss_kw_per_c: None,
                    draw_kw: None,
                    switching_penalty_eur: None,
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
                plan_step_s: 300,  // 5 min steps
                plan_horizon_h: 2, // 2-hour horizon → 24 steps
                ..PlannerConfig::default()
            },
            grid: GridConfig {
                max_import_kw: 25.0,
                max_export_kw: 10.0,
            },
            packets: vec![],
            absorber: Default::default(),
        }
    }

    /// Build a SimSnapshot from profile initial state (mirrors SimState::from_profile).
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
                    // initial_state starts plugged=true
                    let plugged = true;
                    let cap_max_import_kw = if soc >= soc_target { 0.0 } else { max_ch };
                    let mut values = HM::new();
                    values.insert("soc".into(), soc);
                    values.insert("plugged".into(), 1.0); // plugged
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
                    let _ = plugged; // suppress warning; used above
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
                    let cap_max_import_kw = if temp_c >= cfg.temp_max_c {
                        0.0
                    } else {
                        max_kw
                    };
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

    /// Set plugged state on the EV in an existing SimSnapshot.
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

    /// Set battery SoC on the battery in an existing SimSnapshot.
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

    /// Set heater temperature on the heater in an existing SimSnapshot.
    fn set_heater_temp(snap: &mut SimSnapshot, temp_c: f64) {
        if let Some(h) = snap.assets.get_mut("heater") {
            h.values.insert("temp_c".into(), temp_c);
        }
    }

    /// Build a Profile with only a heater (no battery, EV, PV, base_load).
    fn make_heater_only_profile(
        volume_l: Option<f64>,
        temp_min_c: f64,
        temp_max_c: f64,
        temp_initial_c: f64,
    ) -> Profile {
        Profile {
            assets: vec![AssetProfile::Heater(HeaterConfig {
                id: "heater".into(),
                max_kw: 3.0,
                temp_initial_c,
                temp_min_c,
                temp_max_c,
                mid_kw: None,
                volume_l,
                thermal_mass_kwh_per_c: None,
                k_loss_kw_per_c: None,
                draw_kw: None,
                switching_penalty_eur: None,
            })],
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
            absorber: Default::default(),
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

mod basic;
mod solver;
mod pv;
mod planner;
mod heater;
