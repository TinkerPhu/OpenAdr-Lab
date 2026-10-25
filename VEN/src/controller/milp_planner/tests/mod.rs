use super::*;
use crate::controller::milp_planner::asset_port::{
    BatteryMilpContext, EvMilpContext, EvMilpMode, HeaterMilpContext, HeaterMilpMode,
};

use crate::assets::{
    battery::{Battery, BatteryState},
    ev::{EvCharger, EvState},
    heater::{Heater, HeaterState},
    AssetState, MilpBuildInputs, MilpParticipant, ShiftableLoadAsset, ShiftableLoadState,
};
use crate::controller::sim_snapshot::{AssetSnapshot, SimSnapshot};
use crate::entities::asset_params::{
    AssetParams, BaseLoadParams, BatteryParams, EvParams, HeaterParams, PvParams,
};
use crate::entities::grid_signals::GridSignals;
use crate::entities::planner_params::{PlannerObjective, PlannerParams};
use crate::entities::tariff_snapshot::TariffSnapshot;

#[derive(Debug, Clone)]
struct GridConfig {
    max_import_kw: f64,
    max_export_kw: f64,
}

#[derive(Debug, Clone)]
struct Profile {
    assets: Vec<AssetParams>,
    planner: PlannerParams,
    grid: GridConfig,
}

impl Profile {
    fn pv_config(&self) -> Option<&PvParams> {
        self.assets.iter().find_map(|a| match a {
            AssetParams::Pv(v) => Some(v),
            _ => None,
        })
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

/// `GridSignals` for a planner test: prices and the limits in force, with no
/// schedule and no alert/SIMPLE windows — what almost every test wants. A
/// test that needs one of the three names just that one through struct-update
/// syntax, rather than every call site spelling out three empty vectors.
pub(super) fn test_grid(
    tariffs: &TariffTimeSeries,
    capacity_schedule: &[CapacitySnapshot],
) -> crate::entities::grid_signals::GridSignals {
    crate::entities::grid_signals::GridSignals {
        tariffs: tariffs.clone(),
        capacity_schedule: capacity_schedule.to_vec(),
        alert_windows: vec![],
        simple_windows: vec![],
    }
}

/// The site half of a `build_milp_inputs` call for a test profile: its grid limits and nothing
/// else. A test names what it adds through struct-update syntax.
fn site_inputs(profile: &Profile) -> SiteInputs<'_> {
    SiteInputs {
        grid_max_import_kw: profile.grid.max_import_kw,
        grid_max_export_kw: profile.grid.max_export_kw,
        baseline_override: None,
    }
}

/// The profile's PV and base load as the forecast participants the planner tests plan with
/// (W3 design D5): PV from the profile's sin-model curve, base load flat at its profile
/// baseline, the forecasts these tests have always used, with the PV pin applied when set.
/// Production builds both from the live assets instead (`PvInverter`, `BaseLoad`).
fn forecast_contexts(
    profile: &Profile,
    now: DateTime<Utc>,
    pv_forecast_override: Option<f64>,
) -> Vec<Box<dyn crate::controller::milp_planner::AssetMilpContext>> {
    use crate::controller::milp_planner::{ForecastMilpContext, ForecastRole};
    let crate::entities::time_grid::TimeGrid { n, cum_s, .. } =
        crate::entities::time_grid::TimeGrid::from_zones(&profile.planner.plan_zones);
    let mut ctxs: Vec<Box<dyn crate::controller::milp_planner::AssetMilpContext>> = Vec::new();
    for ap in &profile.assets {
        let (role, curtailable, forecast_kw): (ForecastRole, bool, Vec<f64>) = match ap {
            AssetParams::Pv(cfg) => (
                ForecastRole::Generation,
                true,
                (0..n)
                    .map(|t| match pv_forecast_override {
                        Some(kw) => kw.max(0.0),
                        None => cfg.forecast_kw(now + Duration::seconds(cum_s[t])),
                    })
                    .collect(),
            ),
            AssetParams::BaseLoad(cfg) => (ForecastRole::Load, false, vec![cfg.baseline_kw; n]),
            _ => continue,
        };
        ctxs.push(Box::new(ForecastMilpContext {
            asset_id: ap.id().to_string(),
            role,
            forecast_kw,
            curtailable,
        }));
    }
    ctxs
}

/// No diurnal reference series: history disabled, as in almost every planner test.
const NO_REFS: StaleRateRefs<'static> = StaleRateRefs {
    import: None,
    co2: None,
};

/// The capacity schedule of a VTN that has announced nothing.
fn no_capacity() -> Vec<CapacitySnapshot> {
    vec![]
}

/// A schedule declaring a contracted allowance over the whole planning horizon: the VTN's
/// `*_CAPACITY_SUBSCRIPTION` / `*_RESERVATION` events as the parser would resolve them.
fn allowance_schedule(
    import: (Option<f64>, Option<f64>),
    export: (Option<f64>, Option<f64>),
) -> Vec<CapacitySnapshot> {
    vec![CapacitySnapshot {
        interval_start: fixed_now() - Duration::hours(1),
        interval_end: fixed_now() + Duration::hours(48),
        import_subscription_kw: import.0,
        import_reservation_kw: import.1,
        export_subscription_kw: export.0,
        export_reservation_kw: export.1,
        ..CapacitySnapshot::default()
    }]
}

fn make_profile() -> Profile {
    Profile {
        assets: vec![
            AssetParams::Battery(BatteryParams {
                id: "battery".into(),
                capacity_kwh: 10.0,
                max_charge_kw: 5.0,
                max_discharge_kw: 5.0,
                initial_soc_frac: 0.5,
                round_trip_efficiency: 0.9,
                min_soc_frac: 0.1,
                c_terminal_eur_kwh: None,
            }),
            AssetParams::Ev(EvParams {
                id: "ev".into(),
                max_charge_kw: 7.4,
                consumption_kwh_per_km: 0.18,
                max_discharge_kw: 0.0,
                initial_soc_frac: 0.2,
                battery_kwh: 60.0,
                soc_target_frac: 0.8,
                default_charge_kw: 0.0,
                min_charge_kw: 1.4,
                response_delay_s: 10.0,
                v2g_capable: false,
                usage_sim: None,
            }),
            AssetParams::Heater(HeaterParams {
                default_target_temp_c: None,
                id: "heater".into(),
                max_kw: 3.0,
                temp_initial_c: 20.0,
                temp_min_c: 18.0,
                temp_max_c: 23.0,
                temp_safety_max_c: 23.0,
                thermostat_delta_c: 3.0,
                power_stages: 2,
                thermal_mass_kwh_per_c: 2.0,
                k_loss_kw_per_c: 0.1,
                draw_kw: 0.0,
                switching_penalty_eur: 0.01,
                c_terminal_eur_kwh: None,
            }),
            AssetParams::Pv(PvParams {
                id: "pv".into(),
                rated_kw: 5.0,
                inverter_max_kw: 5.0,
                co2_g_kwh: 0.0,
            }),
            AssetParams::BaseLoad(BaseLoadParams {
                id: "base_load".into(),
                baseline_kw: 0.5,
                spikes: vec![],
            }),
        ],
        planner: PlannerParams {
            plan_step_s: 300,
            plan_horizon_h: 2,
            plan_zones: vec![crate::entities::plan::PlanZone {
                step_s: 300,
                slots: 24,
            }],
            // MILP unit tests verify tariff arbitrage mechanics, not the import malus.
            // Zero it so cheap tariff truly means cheap and arbitrage assertions hold.
            c_ctrl_imp_malus_eur_kwh: 0.0,
            ..PlannerParams::default()
        },
        grid: GridConfig {
            max_import_kw: 25.0,
            max_export_kw: 10.0,
        },
    }
}

/// A real simulator snapshot of the profile's assets: capability, storage and
/// values all come from the assets themselves (asset-competence rule).
fn make_snap_from_profile(profile: &Profile) -> SimSnapshot {
    crate::simulator::SimState::from_params(&profile.assets, chrono::Utc::now()).to_sim_snapshot()
}

/// Rebuild one snapshot entry through the real asset after a state change, so
/// its capability/storage/forced power can't go stale.
fn refresh_from_asset(
    entry: &mut AssetSnapshot,
    asset: &dyn crate::assets::Asset,
    state: AssetState,
) {
    let fresh = crate::services::test_support::asset_snapshots::snapshot_from_asset(
        asset,
        state,
        entry.power_kw,
        entry.setpoint_or_power_kw(),
    );
    *entry = AssetSnapshot {
        default_setpoint_kw: entry.default_setpoint_kw,
        ..fresh
    };
}

/// Asset `asset_id`'s planned power in slot `slot` [kW] (positive = import), 0.0 where the plan
/// keeps it idle.
fn planned_kw(slot: &crate::entities::plan::PlanTimeSlot, asset_id: &str) -> f64 {
    slot.planned_kw_by_asset
        .get(asset_id)
        .copied()
        .unwrap_or(0.0)
}

/// The site's power balance in every slot of `plan`, over every asset it planned:
/// `p_imp + p_pv_used − p_exp = p_base + Σ asset net power`, within `tolerance_kw` (an allocation
/// under 0.01 kW is not reported, so the sum can miss that much per asset).
fn assert_site_balance(plan: &Plan, tolerance_kw: f64) {
    for (t, slot) in plan.slots.iter().enumerate() {
        let assets_kw: f64 = plan
            .planned_assets()
            .iter()
            .map(|id| planned_kw(slot, id))
            .sum();
        let supply_kw = slot.net_import_kw + slot.pv_used_kw - slot.net_export_kw;
        let demand_kw = slot.baseline_kw + assets_kw;
        assert!(
            (supply_kw - demand_kw).abs() < tolerance_kw,
            "power balance violated at slot {t}: supply {supply_kw:.4} kW, demand {demand_kw:.4} kW"
        );
    }
}

fn set_ev_plugged(snap: &mut SimSnapshot, plugged: bool) {
    set_ev_state(snap, Some(plugged), None);
}

/// Set the EV's state of charge; the EV itself answers what it can then take and give.
fn set_ev_soc(snap: &mut SimSnapshot, soc_frac: f64) {
    set_ev_state(snap, None, Some(soc_frac));
}

/// Rebuild the EV's snapshot entry from the real `EvCharger` with `plugged` and/or `soc_frac`
/// replaced, so capability and the available energies are the asset's own answer.
fn set_ev_state(snap: &mut SimSnapshot, plugged: Option<bool>, soc_frac: Option<f64>) {
    if let Some(ev) = snap.assets.get_mut("ev") {
        let v = |k: &str| ev.val(k).unwrap_or(0.0);
        let charger = EvCharger {
            max_charge_kw: v("max_charge_kw"),
            max_discharge_kw: v("max_discharge_kw"),
            v2g_capable: v("max_discharge_kw") > 0.0,
            battery_kwh: v("battery_kwh"),
            consumption_kwh_per_km: 0.18,
            soc_target_frac: v("soc_target"),
            soc_target_profile: v("soc_target"),
            default_charge_kw: ev.default_setpoint_kw.expect("the EV follows a setpoint"),
            min_soc_frac: v("min_soc"),
            min_charge_kw: v("min_charge_kw"),
            response_delay_s: 0.0,
            departure_time: None,
            usage_sim: None,
            usage_sim_seed_tag: 0,
        };
        let state = AssetState::Ev(EvState {
            soc_frac: soc_frac.unwrap_or_else(|| v("soc")),
            plugged: plugged.unwrap_or_else(|| v("plugged") > 0.5),
            actual_power_kw: 0.0,
            pending_command_kw: 0.0,
            was_away_by_usage_sim: false,
            unserved_at_departure_kwh: 0.0,
        });
        refresh_from_asset(ev, &charger, state);
    }
}

fn set_battery_soc(snap: &mut SimSnapshot, soc_frac: f64) {
    if let Some(bat) = snap.assets.get_mut("battery") {
        let v = |k: &str| bat.val(k).unwrap_or(0.0);
        let battery = Battery {
            capacity_kwh: v("capacity_kwh"),
            max_charge_kw: v("max_charge_kw"),
            max_discharge_kw: v("max_discharge_kw"),
            round_trip_efficiency: v("round_trip_efficiency"),
            min_soc_frac: v("min_soc"),
        };
        let state = AssetState::Battery(BatteryState {
            soc_frac,
            actual_power_kw: 0.0,
        });
        refresh_from_asset(bat, &battery, state);
    }
}

fn set_heater_temp(snap: &mut SimSnapshot, temp_c: f64) {
    use crate::assets::heater::HeaterEmergencyMode;
    if let Some(h) = snap.assets.get_mut("heater") {
        let v = |k: &str| h.val(k).unwrap_or(0.0);
        let heater = Heater {
            default_target_temp_c: None,
            max_kw: v("max_kw"),
            power_stages: v("power_stages") as u8,
            temp_min_c: v("temp_min_c"),
            temp_max_c: v("temp_max_c"),
            temp_min_c_profile: v("temp_min_c"),
            temp_max_c_profile: v("temp_max_c"),
            temp_safety_max_c: v("temp_safety_max_c"),
            thermostat_delta_c: 3.0,
            emergency_mode: HeaterEmergencyMode::from_overrides(
                Some(v("emergency_curtail") > 0.5),
                Some(v("emergency_absorb") > 0.5),
            ),
            thermal_mass_kwh_per_c: v("thermal_mass_kwh_per_c"),
            k_loss_kw_per_c: 0.0,
            draw_kw: 0.0,
            switching_penalty_eur: 0.0,
            ambient_temp_c: 10.0,
        };
        let state = AssetState::Heater(HeaterState {
            temperature_c: temp_c,
            actual_power_kw: 0.0,
            emergency_latched: false,
            ceiling_latched: false,
        });
        refresh_from_asset(h, &heater, state);
    }
}

fn set_heater_power(snap: &mut SimSnapshot, power_kw: f64) {
    if let Some(h) = snap.assets.get_mut("heater") {
        h.power_kw = power_kw;
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
        assets: vec![AssetParams::Heater(HeaterParams {
            default_target_temp_c: None,
            id: "heater".into(),
            max_kw: 3.0,
            temp_initial_c,
            temp_min_c,
            temp_max_c,
            temp_safety_max_c: temp_max_c,
            thermostat_delta_c: 3.0,
            power_stages: 2,
            thermal_mass_kwh_per_c: thermal_mass,
            k_loss_kw_per_c: 0.1,
            draw_kw: 0.0,
            switching_penalty_eur: 0.01,
            c_terminal_eur_kwh: None,
        })],
        planner: PlannerParams {
            plan_step_s: 300,
            plan_horizon_h: 2,
            plan_zones: vec![crate::entities::plan::PlanZone {
                step_s: 300,
                slots: 24,
            }],
            // MILP unit tests verify tariff arbitrage mechanics, not the import malus.
            // Zero it so cheap tariff truly means cheap and arbitrage assertions hold.
            c_ctrl_imp_malus_eur_kwh: 0.0,
            ..PlannerParams::default()
        },
        grid: GridConfig {
            max_import_kw: 25.0,
            max_export_kw: 10.0,
        },
    }
}

/// A site with nothing but a 1 kW base load, planned in 4 x 1800 s slots on a 25 kW / 10 kW grid.
fn base_load_only_profile() -> Profile {
    Profile {
        assets: vec![AssetParams::BaseLoad(BaseLoadParams {
            id: "base_load".into(),
            baseline_kw: 1.0,
            spikes: vec![],
        })],
        planner: PlannerParams {
            plan_step_s: 1800,
            plan_horizon_h: 2,
            plan_zones: vec![crate::entities::plan::PlanZone {
                step_s: 1800,
                slots: 4,
            }],
            ..PlannerParams::default()
        },
        grid: GridConfig {
            max_import_kw: 25.0,
            max_export_kw: 10.0,
        },
    }
}

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

/// The parts of one planning cycle every asset context of a test is built from.
struct TestCycle<'a> {
    n: usize,
    cum_s: &'a [i64],
    now: DateTime<Utc>,
    ev_sessions: &'a [crate::entities::device_session::EvSession],
    heater_target: Option<&'a crate::entities::device_session::HeaterTarget>,
    planner: &'a PlannerParams,
}

impl<'a> TestCycle<'a> {
    fn inputs(
        &self,
        asset_id: &'a str,
        state: &'a AssetState,
        c_terminal_eur_kwh: f64,
    ) -> MilpBuildInputs<'a> {
        MilpBuildInputs {
            asset_id,
            state,
            n: self.n,
            cum_s: self.cum_s,
            now: self.now,
            ev_sessions: self.ev_sessions,
            heater_target: self.heater_target,
            comfort_rates: &[],
            planner: self.planner,
            c_terminal_eur_kwh,
            heater_anchor: vec![],
            pv_forecast_override: None,
        }
    }
}

fn build_asset_contexts(
    profile: &Profile,
    snap: &SimSnapshot,
    now: DateTime<Utc>,
    ev_session: Option<&crate::entities::device_session::EvSession>,
    heater_target: Option<&crate::entities::device_session::HeaterTarget>,
    tariffs: &TariffTimeSeries,
) -> Vec<Box<dyn crate::controller::milp_planner::AssetMilpContext>> {
    let crate::entities::time_grid::TimeGrid { n, cum_s, .. } =
        crate::entities::time_grid::TimeGrid::from_zones(&profile.planner.plan_zones);
    let cycle = TestCycle {
        n,
        cum_s: &cum_s,
        now,
        ev_sessions: ev_session.map(std::slice::from_ref).unwrap_or(&[]),
        heater_target,
        planner: &profile.planner,
    };

    let avg_imp_eur_kwh = {
        let total: f64 = (0..n)
            .map(|t| {
                let slot_t = now + Duration::seconds(cum_s[t]);
                tariffs
                    .import_eur_kwh
                    .interpolate_at(slot_t)
                    .unwrap_or(0.25)
            })
            .sum();
        if n > 0 {
            total / n as f64
        } else {
            0.25
        }
    };

    let mut ctxs: Vec<Box<dyn crate::controller::milp_planner::AssetMilpContext>> = Vec::new();
    for ap in &profile.assets {
        match ap {
            AssetParams::Battery(cfg) => {
                let soc_frac = snap
                    .assets
                    .get("battery")
                    .and_then(|s| s.val("soc"))
                    .unwrap_or(cfg.initial_soc_frac);
                let state = AssetState::Battery(BatteryState {
                    soc_frac,
                    actual_power_kw: 0.0,
                });
                let ac = Battery::from_params(cfg);
                let c_terminal = cfg
                    .c_terminal_eur_kwh
                    .unwrap_or(avg_imp_eur_kwh * cfg.round_trip_efficiency);
                ctxs.push(MilpParticipant::build_milp_context(
                    &ac,
                    &cycle.inputs("battery", &state, c_terminal),
                ));
            }
            AssetParams::Ev(cfg) => {
                let soc_frac = snap
                    .assets
                    .get("ev")
                    .and_then(|s| s.val("soc"))
                    .unwrap_or(cfg.initial_soc_frac);
                let plugged = snap
                    .assets
                    .get("ev")
                    .and_then(|s| s.val("plugged"))
                    .map(|v| v > 0.5)
                    .unwrap_or(true);
                let state = AssetState::Ev(EvState {
                    soc_frac,
                    actual_power_kw: 0.0,
                    plugged,
                    pending_command_kw: 0.0,
                    was_away_by_usage_sim: false,
                    unserved_at_departure_kwh: 0.0,
                });
                let ac = EvCharger::from_params(cfg);
                ctxs.push(MilpParticipant::build_milp_context(
                    &ac,
                    &cycle.inputs("ev", &state, 0.0),
                ));
            }
            AssetParams::Heater(cfg) => {
                let heater_snap = snap.assets.get("heater");
                let temp_c = heater_snap
                    .and_then(|s| s.val("temp_c"))
                    .unwrap_or(cfg.temp_initial_c);
                let actual_power_kw = heater_snap.map(|s| s.power_kw).unwrap_or(0.0);
                let state = AssetState::Heater(HeaterState {
                    temperature_c: temp_c,
                    actual_power_kw,
                    emergency_latched: false,
                    ceiling_latched: false,
                });
                let ac = Heater::from_params(cfg);
                let c_terminal = cfg
                    .c_terminal_eur_kwh
                    .unwrap_or(avg_imp_eur_kwh + profile.planner.c_ctrl_imp_malus_eur_kwh);
                ctxs.push(MilpParticipant::build_milp_context(
                    &ac,
                    &cycle.inputs("heater", &state, c_terminal),
                ));
            }
            _ => {}
        }
    }
    // The forecast participants come first, as `run_planner` orders them.
    let mut all = forecast_contexts(profile, now, None);
    all.extend(ctxs);
    all
}

/// The forecast participants a live simulator state builds (its PV and base load, each from its
/// own forecast), the way production builds them (`simulator::plan_context::build_asset_contexts`).
fn live_forecast_contexts(
    sim_state: &crate::simulator::SimState,
    profile: &Profile,
    now: DateTime<Utc>,
) -> Vec<Box<dyn crate::controller::milp_planner::AssetMilpContext>> {
    use crate::controller::milp_planner::Participation;
    let crate::entities::time_grid::TimeGrid { n, cum_s, .. } =
        crate::entities::time_grid::TimeGrid::from_zones(&profile.planner.plan_zones);
    crate::simulator::plan_context::build_asset_contexts(
        sim_state,
        n,
        &cum_s,
        now,
        &std::collections::BTreeMap::new(),
        &std::collections::BTreeMap::new(),
        &profile.planner,
        &std::collections::HashMap::new(),
        &std::collections::BTreeMap::new(),
        &std::collections::HashMap::new(),
        None,
    )
    .into_iter()
    .filter(|c| matches!(c.participation(), Participation::Forecast { .. }))
    .collect()
}

/// `ctxs` with the forecast participant `id` replaced by one forecasting `forecast_kw`: a test
/// stating what that asset's own forecast is (e.g. a learned base-load profile).
fn with_forecast(
    ctxs: Vec<Box<dyn crate::controller::milp_planner::AssetMilpContext>>,
    id: &str,
    forecast_kw: Vec<f64>,
) -> Vec<Box<dyn crate::controller::milp_planner::AssetMilpContext>> {
    use crate::controller::milp_planner::{ForecastMilpContext, Participation};
    ctxs.into_iter()
        .map(|c| match c.participation() {
            Participation::Forecast { role, .. } if c.asset_id() == id => {
                Box::new(ForecastMilpContext {
                    asset_id: id.to_string(),
                    role,
                    forecast_kw: forecast_kw.clone(),
                    curtailable: c
                        .as_any()
                        .downcast_ref::<ForecastMilpContext>()
                        .is_some_and(|f| f.curtailable),
                }) as Box<dyn crate::controller::milp_planner::AssetMilpContext>
            }
            _ => c,
        })
        .collect()
}

/// One firm EV obligation of `kwh` to be delivered by `deadline_step`, expressed
/// the way the model states it since R-93: a target state of charge against the
/// fixture's own pack size. Keeps the many solver fixtures that used to say
/// "`e_ev_required_kwh`: 20.0" one line long, and in one place, so the
/// kWh→SoC conversion cannot drift test by test.
fn ev_firm_kwh(
    battery_kwh: f64,
    soc_init_frac: f64,
    kwh: f64,
    deadline_step: usize,
) -> Vec<crate::controller::milp_planner::asset_port::EvObligation> {
    vec![crate::controller::milp_planner::asset_port::EvObligation {
        deadline_step,
        target_soc_frac: soc_init_frac + kwh / battery_kwh,
        session_id: None,
    }]
}

/// Test-only equivalent of `plan_context.rs::build_asset_contexts`'s generic
/// `sim_snap.iter_assets().filter_map(...)` pass, for shiftable loads only —
/// this test module's own `build_asset_contexts` above builds contexts from
/// `profile.assets` (static config), which shiftable loads were never part
/// of (shiftable-load-as-asset D4). Appends one `ShiftableLoadMilpContext`
/// per pending (not-yet-started) load onto an existing `ctxs` list.
fn push_shiftable_load_contexts(
    ctxs: &mut Vec<Box<dyn crate::controller::milp_planner::AssetMilpContext>>,
    loads: &[crate::entities::device_session::ShiftableLoad],
    profile: &Profile,
    now: DateTime<Utc>,
) {
    let crate::entities::time_grid::TimeGrid { n, cum_s, .. } =
        crate::entities::time_grid::TimeGrid::from_zones(&profile.planner.plan_zones);
    for load in loads {
        let asset = ShiftableLoadAsset {
            power_kw: load.power_kw,
            duration_min: load.duration_min,
            earliest_start: load.earliest_start,
            latest_end: load.latest_end,
        };
        let state = AssetState::ShiftableLoad(ShiftableLoadState {
            started: false,
            elapsed_min: 0.0,
            actual_power_kw: 0.0,
            past_deadline: false,
        });
        let cycle = TestCycle {
            n,
            cum_s: &cum_s,
            now,
            ev_sessions: &[],
            heater_target: None,
            planner: &profile.planner,
        };
        ctxs.push(MilpParticipant::build_milp_context(
            &asset,
            &cycle.inputs(&load.asset_id, &state, 0.0),
        ));
    }
}

fn build_phase1_weights(profile: &Profile, objective: PlannerObjective) -> Phase1Weights {
    super::build_phase1_weights(&profile.planner, objective)
}

#[allow(clippy::too_many_arguments)] // test wrapper mirrors the real solve-path signature
fn build_milp_inputs(
    tariffs: &TariffTimeSeries,
    cap: &[CapacitySnapshot],
    profile: &Profile,
    now: DateTime<Utc>,
    shiftable_loads: &[crate::entities::device_session::ShiftableLoad],
    baseline_override: Option<&crate::entities::device_session::BaselineOverride>,
) -> MilpInputs {
    build_milp_inputs_with_override(
        tariffs,
        cap,
        profile,
        now,
        shiftable_loads,
        baseline_override,
        None,
    )
}

#[allow(clippy::too_many_arguments)] // test wrapper mirrors the real solve-path signature
fn build_milp_inputs_with_override(
    tariffs: &TariffTimeSeries,
    cap: &[CapacitySnapshot],
    profile: &Profile,
    now: DateTime<Utc>,
    // Superseded by `ctxs` (shiftable-load-as-asset) — kept as a parameter
    // since callers still pass it positionally; see `mod.rs::run_planner`'s
    // own `_shiftable_loads` for the same reasoning.
    _shiftable_loads: &[crate::entities::device_session::ShiftableLoad],
    baseline_override: Option<&crate::entities::device_session::BaselineOverride>,
    pv_forecast_override: Option<f64>,
) -> MilpInputs {
    super::build_milp_inputs(
        &test_grid(tariffs, cap),
        &profile.planner,
        &SiteInputs {
            baseline_override,
            ..site_inputs(profile)
        },
        &NO_REFS,
        now,
        &forecast_contexts(profile, now, pv_forecast_override),
    )
}

/// `run_planner` for a test profile. Contexts built by hand without any forecast participant
/// get the profile's PV and base load added, which is what the profile always contributed to the
/// site before PV and base load became participants (W3).
#[allow(clippy::too_many_arguments)] // test wrapper mirrors the real solve-path signature
fn run_planner(
    mut asset_contexts: Vec<Box<dyn crate::controller::milp_planner::AssetMilpContext>>,
    tariffs: &TariffTimeSeries,
    capacity: &[CapacitySnapshot],
    profile: &Profile,
    now: DateTime<Utc>,
    trigger: PlanTrigger,
    baseline_override: Option<&crate::entities::device_session::BaselineOverride>,
    objective_override: Option<PlannerObjective>,
) -> Plan {
    use crate::controller::milp_planner::Participation;
    if !asset_contexts
        .iter()
        .any(|c| matches!(c.participation(), Participation::Forecast { .. }))
    {
        let mut with_site = forecast_contexts(profile, now, None);
        with_site.extend(asset_contexts);
        asset_contexts = with_site;
    }
    super::run_planner(
        asset_contexts,
        &test_grid(tariffs, capacity),
        &profile.planner,
        profile.grid.max_import_kw,
        profile.grid.max_export_kw,
        now,
        trigger,
        baseline_override,
        objective_override,
        None,
        None,
    )
}

fn bmi(
    profile: &Profile,
    sim: &SimSnapshot,
    tariffs: &TariffTimeSeries,
    cap: &[CapacitySnapshot],
    now: DateTime<Utc>,
    ev_session: Option<&crate::entities::device_session::EvSession>,
    heater_target: Option<&crate::entities::device_session::HeaterTarget>,
) -> Built {
    let ctxs = build_asset_contexts(profile, sim, now, ev_session, heater_target, tariffs);
    Built {
        inputs: build_milp_inputs(tariffs, cap, profile, now, &[], None),
        ctxs,
    }
}

/// What `bmi` built: the planner's inputs, and the asset contexts they were built from. A test
/// about an asset's own parameters reads them where the solve does, from the asset's context
/// (`battery()`, `heater()`), not from the inputs.
struct Built {
    inputs: MilpInputs,
    ctxs: Vec<Box<dyn crate::controller::milp_planner::AssetMilpContext>>,
}

impl std::ops::Deref for Built {
    type Target = MilpInputs;
    fn deref(&self) -> &MilpInputs {
        &self.inputs
    }
}

impl Built {
    fn battery(&self) -> &BatteryMilpContext {
        context(&self.ctxs)
    }

    fn heater(&self) -> &HeaterMilpContext {
        context(&self.ctxs)
    }

    fn ev(&self) -> &EvMilpContext {
        context(&self.ctxs)
    }
}

/// The first context of type `C` among `ctxs`: what a test reads to see what an asset derived
/// for itself (its mask, its obligations, its stages).
fn context<C: 'static>(ctxs: &[Box<dyn crate::controller::milp_planner::AssetMilpContext>]) -> &C {
    ctxs.iter()
        .find_map(|c| c.as_any().downcast_ref::<C>())
        .unwrap_or_else(|| panic!("no {} was built", std::any::type_name::<C>()))
}

/// One battery's solution as a test reads it: zeros of the plan's lengths when the site has none.
struct BatteryView {
    charge_kw: Vec<f64>,
    discharge_kw: Vec<f64>,
    /// Stored energy at every step boundary [kWh], len = n + 1.
    energy_kwh: Vec<f64>,
    /// The charge (1) / discharge (0) direction per slot, empty when the site has no battery.
    direction: Vec<f64>,
}

/// One EV's solution as a test reads it: zeros of the plan's lengths when the site has none.
struct EvView {
    power_kw: Vec<f64>,
    /// State of charge at every step boundary, len = n + 1.
    soc: Vec<f64>,
    /// Per-obligation shortfall [kWh].
    shortfall_kwh: Vec<f64>,
    on: Vec<f64>,
    extra_kwh: f64,
    bands_kwh: f64,
}

/// One heater's solution as a test reads it: zeros of the plan's lengths when the site has none.
struct HeaterView {
    stage: Vec<f64>,
    ready: f64,
    /// Tank energy above T_min [kWh] per slot, empty when the site has no heater.
    tank_kwh: Vec<f64>,
}

/// How a test reads one asset's part of a solution, so no test names how `SolveOutput` stores
/// it. Each returns the first asset of its kind, which is the only one in every single-site test.
trait AssetSolutionsView {
    fn battery(&self) -> BatteryView;
    fn ev(&self) -> EvView;
    fn heater(&self) -> HeaterView;
    /// Each shiftable load's power per slot [kW], in context order.
    fn shiftable_kw(&self) -> Vec<Vec<f64>>;
    /// Each shiftable load's start choice per valid start slot, by asset id.
    fn shiftable_starts(&self) -> Vec<(String, Vec<f64>)>;
    /// What every asset reports about its own solution, in planner input order.
    fn warnings(&self) -> Vec<crate::entities::plan::PlanWarning>;
}

impl AssetSolutionsView for SolveOutput {
    fn battery(&self) -> BatteryView {
        let n = self.p_imp_kw.len();
        self.assets
            .iter()
            .find_map(|a| match (&a.detail, &a.storage) {
                (AssetSolutionDetail::Battery { direction }, Some(legs)) => Some(BatteryView {
                    charge_kw: legs.charge_kw.clone(),
                    discharge_kw: legs.discharge_kw.clone(),
                    energy_kwh: legs.energy_kwh.clone(),
                    direction: direction.clone(),
                }),
                _ => None,
            })
            .unwrap_or_else(|| BatteryView {
                charge_kw: vec![0.0; n],
                discharge_kw: vec![0.0; n],
                energy_kwh: vec![0.0; n + 1],
                direction: vec![],
            })
    }

    fn ev(&self) -> EvView {
        let n = self.p_imp_kw.len();
        self.assets
            .iter()
            .find_map(|a| match &a.detail {
                AssetSolutionDetail::Ev(e) => Some(EvView {
                    power_kw: e.p_ev_kw.clone(),
                    soc: e.soc_ev.clone(),
                    shortfall_kwh: e.shortfall_kwh.clone(),
                    on: e.z_ev_on.clone(),
                    extra_kwh: e.e_ev_extra_kwh,
                    bands_kwh: e.e_seg_kwh,
                }),
                _ => None,
            })
            .unwrap_or_else(|| EvView {
                power_kw: vec![0.0; n],
                soc: vec![0.0; n + 1],
                shortfall_kwh: vec![],
                on: vec![0.0; n],
                extra_kwh: 0.0,
                bands_kwh: 0.0,
            })
    }

    fn heater(&self) -> HeaterView {
        let n = self.p_imp_kw.len();
        self.assets
            .iter()
            .find_map(|a| match &a.detail {
                AssetSolutionDetail::Heater(h) => Some(HeaterView {
                    stage: h.y_heat.clone(),
                    ready: h.z_heat_ready,
                    tank_kwh: h.e_tank_kwh.clone(),
                }),
                _ => None,
            })
            .unwrap_or_else(|| HeaterView {
                stage: vec![0.0; n],
                ready: 0.0,
                tank_kwh: vec![],
            })
    }

    fn shiftable_kw(&self) -> Vec<Vec<f64>> {
        self.assets
            .iter()
            .filter(|a| matches!(a.detail, AssetSolutionDetail::ShiftableLoad { .. }))
            .map(|a| a.net_power_kw.clone())
            .collect()
    }

    fn shiftable_starts(&self) -> Vec<(String, Vec<f64>)> {
        let mut starts: Vec<_> = self
            .assets
            .iter()
            .filter_map(|a| match &a.detail {
                AssetSolutionDetail::ShiftableLoad { starts } => {
                    Some((a.asset_id.clone(), starts.clone()))
                }
                _ => None,
            })
            .collect();
        starts.sort_by(|a, b| a.0.cmp(&b.0));
        starts
    }

    fn warnings(&self) -> Vec<crate::entities::plan::PlanWarning> {
        self.assets
            .iter()
            .flat_map(|a| a.warnings.iter().cloned())
            .collect()
    }
}

mod fixtures;
mod forecast_plan;
use fixtures::*;

mod base_load;
mod basic;
mod capacity_schedule;
mod cost_sign;
mod gb41_soft_deadline_core;
mod golden_compare;
mod heater;
mod inputs_golden;
mod model_fingerprint;
mod modes;
mod multi_instance;
mod penalty;
mod phase2_spikes;
mod planner;
mod plans_golden;
mod pv;
mod soc_balance;
mod solve_cost;
mod solver;
mod stale_rates;
