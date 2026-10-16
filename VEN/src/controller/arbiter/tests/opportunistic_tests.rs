//! The opportunistic pass (openspec `opportunistic-objective`, spec `opportunistic-dispatch`):
//! surplus taken in declared order, import covered by storage, never beyond the balance.
//! Snapshots come from real assets via `SimState`, so no fixture states an asset's capability.

use std::collections::{BTreeSet, HashMap};

use super::super::{opportunistic_pass, ArbiterOutcome, ArbiterTick, EvOverlays};
use crate::controller::SimSnapshot;
use crate::entities::asset_params::{
    AssetParams, BaseLoadParams, BatteryParams, EvParams, HeaterParams, PvParams,
};
use crate::entities::planner_params::PlannerObjective;

fn battery(initial_soc_frac: f64) -> AssetParams {
    AssetParams::Battery(BatteryParams {
        initial_soc_frac,
        ..BatteryParams::default()
    })
}

fn ev() -> AssetParams {
    AssetParams::Ev(EvParams {
        initial_soc_frac: 0.4,
        soc_target_frac: 0.8,
        ..EvParams::default()
    })
}

fn heater_at(temp_c: f64) -> AssetParams {
    AssetParams::Heater(HeaterParams {
        temp_initial_c: temp_c,
        ..HeaterParams::default()
    })
}

fn site(params: &[AssetParams]) -> SimSnapshot {
    let mut all = params.to_vec();
    all.push(AssetParams::Pv(PvParams::default()));
    all.push(AssetParams::BaseLoad(BaseLoadParams::default()));
    crate::simulator::SimState::from_params(&all, chrono::Utc::now()).to_sim_snapshot()
}

/// One pass with PV generating `pv_kw` and the base load drawing `base_kw`.
fn pass(sim: &SimSnapshot, pv_kw: f64, base_kw: f64, excluded: &[&str]) -> ArbiterOutcome {
    let live: HashMap<String, f64> = [
        (crate::ids::ASSET_PV.to_string(), -pv_kw),
        (crate::ids::ASSET_BASE_LOAD.to_string(), base_kw),
    ]
    .into();
    let overlay = EvOverlays::new();
    let bridge = HashMap::new();
    let tick = ArbiterTick {
        sim,
        plan_slot: None,
        objective: PlannerObjective::Opportunistic,
        ev_overlay: &overlay,
        live_kw: &live,
        live_released_kw: None,
        alert_active: false,
        limit_target_kw: None,
        prev_bridge_kw_by_asset: &bridge,
    };
    let base: HashMap<String, f64> = sim
        .assets
        .iter()
        .map(|(id, a)| (id.clone(), a.default_setpoint_kw))
        .collect();
    let excluded: BTreeSet<String> = excluded.iter().map(|s| s.to_string()).collect();
    opportunistic_pass(&tick, &base, &excluded)
}

fn sp(o: &ArbiterOutcome, id: &str) -> f64 {
    o.setpoints[id]
}

#[test]
fn scarce_surplus_goes_to_the_lowest_rank_first() {
    let sim = site(&[ev(), battery(0.5)]);
    let o = pass(&sim, 3.0, 0.5, &[]);
    assert!(
        (sp(&o, "ev") - 2.5).abs() < 1e-9,
        "EV takes the whole 2.5 kW"
    );
    assert_eq!(sp(&o, "battery"), 0.0, "nothing left for the battery");
    let r = o.opportunistic.unwrap();
    assert_eq!(r.leading_asset_id.as_deref(), Some("ev"));
    assert!(r.absorbing);
}

#[test]
fn ample_surplus_fills_every_asset_in_order_and_exports_the_rest() {
    let sim = site(&[ev(), battery(0.5)]);
    let ev_max = EvParams::default().max_charge_kw;
    let bat_max = BatteryParams::default().max_charge_kw;
    let surplus = ev_max + bat_max + 1.0;
    let o = pass(&sim, surplus + 0.5, 0.5, &[]);
    assert!((sp(&o, "ev") - ev_max).abs() < 1e-9);
    assert!((sp(&o, "battery") - bat_max).abs() < 1e-9);
    assert!(
        (o.opportunistic.unwrap().unresolved_kw + 1.0).abs() < 1e-9,
        "1 kW exported"
    );
}

#[test]
fn a_full_asset_passes_the_surplus_on() {
    let sim = site(&[
        battery(1.0),
        heater_at(HeaterParams::default().temp_min_c + 5.0),
    ]);
    let o = pass(&sim, 2.5, 0.5, &[]);
    assert_eq!(sp(&o, "battery"), 0.0, "a full battery takes nothing");
    assert!(sp(&o, "heater") > 0.0, "the heater takes the surplus");
}

#[test]
fn a_stage_that_would_import_is_not_taken() {
    // A heater stage larger than the surplus would import the difference.
    let params = HeaterParams::default();
    let stage_kw = params.max_kw / params.power_stages as f64;
    let sim = site(&[heater_at(params.temp_min_c + 5.0)]);
    let o = pass(&sim, 0.5 + stage_kw * 0.5, 0.5, &[]);
    assert_eq!(sp(&o, "heater"), 0.0);
}

#[test]
fn the_battery_covers_base_load_at_night() {
    let sim = site(&[battery(0.5)]);
    let o = pass(&sim, 0.0, 0.8, &[]);
    assert!(
        (sp(&o, "battery") + 0.8).abs() < 1e-9,
        "discharges exactly the import"
    );
    assert!(!o.opportunistic.unwrap().absorbing);
}

#[test]
fn the_battery_never_discharges_beyond_the_import() {
    let sim = site(&[battery(0.9)]);
    let o = pass(&sim, 0.0, 0.5, &[]);
    assert!(sp(&o, "battery") >= -0.5 - 1e-9);
}

#[test]
fn the_battery_never_charges_from_the_grid() {
    let sim = site(&[battery(0.5)]);
    let o = pass(&sim, 0.0, 2.0, &[]);
    assert!(sp(&o, "battery") <= 0.0);
}

#[test]
fn an_empty_battery_leaves_the_import() {
    let empty = BatteryParams::default().min_soc_frac;
    let sim = site(&[battery(empty)]);
    let o = pass(&sim, 0.0, 1.0, &[]);
    assert_eq!(sp(&o, "battery"), 0.0);
    assert!((o.opportunistic.unwrap().unresolved_kw - 1.0).abs() < 1e-9);
}

#[test]
fn a_balanced_site_moves_nothing() {
    let sim = site(&[ev(), battery(0.5)]);
    let o = pass(&sim, 0.5, 0.5, &[]);
    assert_eq!(sp(&o, "ev"), 0.0);
    assert_eq!(sp(&o, "battery"), 0.0);
    assert_eq!(o.opportunistic.unwrap().leading_asset_id, None);
}

#[test]
fn an_excluded_asset_keeps_its_base_and_the_next_one_takes_the_surplus() {
    let sim = site(&[ev(), battery(0.5)]);
    let o = pass(&sim, 2.5, 0.5, &["ev"]);
    assert_eq!(sp(&o, "ev"), 0.0);
    assert!((sp(&o, "battery") - 2.0).abs() < 1e-9);
}

/// The pass writes nothing the deviation pass or the residual tracker reads (D0).
#[test]
fn the_pass_writes_no_deviation_state() {
    let sim = site(&[ev(), battery(0.5)]);
    for (pv, base) in [(3.0, 0.5), (0.0, 1.0)] {
        let o = pass(&sim, pv, base, &[]);
        assert_eq!(o.active_lever, None);
        assert!(o.displaced_kw_by_asset.is_empty());
        assert_eq!((o.net_kw, o.dev_kw), (None, None));
        assert_eq!(o.unresolved_kw, 0.0);
        assert!(o.limit.is_none());
    }
}

/// The same balance gives the same commands tick after tick: an open-loop allocation, so a
/// charger's response lag cannot make it toggle.
#[test]
fn the_same_balance_commands_the_same_setpoints_every_tick() {
    let sim = site(&[ev(), battery(0.5)]);
    let first = pass(&sim, 3.0, 0.5, &[]);
    for _ in 0..20 {
        assert_eq!(pass(&sim, 3.0, 0.5, &[]).setpoints, first.setpoints);
    }
}

mod request_regime {
    use super::*;
    use crate::controller::arbiter::request_regime;
    use crate::entities::design_vocabulary::NonPlanningDispatch::{StartNow, SurplusOnly};

    fn defaults(sim: &SimSnapshot) -> HashMap<String, f64> {
        sim.assets
            .iter()
            .map(|(id, a)| (id.clone(), a.default_setpoint_kw))
            .collect()
    }

    #[test]
    fn a_start_now_request_runs_at_full_effort_and_takes_no_surplus_on_top() {
        let sim = site(&[ev(), battery(0.5)]);
        let requests = [("ev".to_string(), StartNow)].into();
        let (base, excluded) = request_regime(&sim, defaults(&sim), &requests, |_| true);
        assert_eq!(base["ev"], sim.assets["ev"].cap_max_import_kw);
        assert!(base["ev"] > 0.0);
        assert!(excluded.contains("ev"));
        assert!(!excluded.contains("battery"));
    }

    #[test]
    fn a_surplus_only_request_takes_surplus_even_with_the_switch_off() {
        let sim = site(&[ev()]);
        let requests = [("ev".to_string(), SurplusOnly)].into();
        let (base, excluded) = request_regime(&sim, defaults(&sim), &requests, |_| false);
        assert_eq!(base["ev"], 0.0);
        assert!(!excluded.contains("ev"));
    }

    #[test]
    fn without_a_request_the_assets_own_switch_decides() {
        let sim = site(&[ev(), battery(0.5)]);
        let off_for_ev = |id: &str| id != "ev";
        let (_, excluded) = request_regime(&sim, defaults(&sim), &HashMap::new(), off_for_ev);
        assert!(excluded.contains("ev"));
        assert!(!excluded.contains("battery"));
    }

    /// The switch is an EV's setting: a battery or heater takes surplus whatever it says.
    #[test]
    fn the_surplus_switch_governs_only_surplus_charging_assets() {
        let sim = site(&[ev(), battery(0.5), heater_at(50.0)]);
        let (_, excluded) = request_regime(&sim, defaults(&sim), &HashMap::new(), |_| false);
        assert_eq!(
            excluded.into_iter().collect::<Vec<_>>(),
            vec!["ev".to_string()]
        );
    }

    /// A start-now EV under the opportunistic pass: full rate now, whatever the PV output.
    #[test]
    fn a_start_now_ev_charges_at_full_rate_without_pv() {
        let sim = site(&[ev(), battery(0.5)]);
        let requests = [("ev".to_string(), StartNow)].into();
        let (base, excluded) = request_regime(&sim, defaults(&sim), &requests, |_| true);
        let live: HashMap<String, f64> = [
            (crate::ids::ASSET_PV.to_string(), 0.0),
            (crate::ids::ASSET_BASE_LOAD.to_string(), 0.5),
        ]
        .into();
        let overlay = EvOverlays::new();
        let bridge = HashMap::new();
        let tick = ArbiterTick {
            sim: &sim,
            plan_slot: None,
            objective: PlannerObjective::Opportunistic,
            ev_overlay: &overlay,
            live_kw: &live,
            live_released_kw: None,
            alert_active: false,
            limit_target_kw: None,
            prev_bridge_kw_by_asset: &bridge,
        };
        let o = opportunistic_pass(&tick, &base, &excluded);
        assert_eq!(o.setpoints["ev"], EvParams::default().max_charge_kw);
        // A meter-following home battery covers what the meter would import, the EV's
        // charge included, as far as its own rate allows.
        let bat_max = BatteryParams::default().max_discharge_kw;
        let import = 0.5 + EvParams::default().max_charge_kw;
        assert!((o.setpoints["battery"] + import.min(bat_max)).abs() < 1e-9);
    }
}

mod requests {
    use crate::controller::arbiter::opportunistic_requests;
    use crate::entities::design_vocabulary::NonPlanningDispatch::{StartNow, SurplusOnly};
    use crate::entities::design_vocabulary::UserRequestMode;
    use crate::entities::device_session::{EvSession, HeaterTarget, ShiftableLoad};
    use chrono::{Duration, TimeZone, Utc};
    use std::collections::{BTreeMap, HashMap};

    fn now() -> chrono::DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 7, 14, 12, 0, 0).unwrap()
    }

    fn heater_target(mode: UserRequestMode) -> HeaterTarget {
        HeaterTarget {
            id: uuid::Uuid::new_v4(),
            asset_id: "heater".into(),
            target_temp_c: 55.0,
            ready_by: now() + Duration::hours(3),
            mode,
            comfort_rates: vec![],
            created_at: now(),
            updated_at: now(),
        }
    }

    fn shiftable(mode: UserRequestMode, start_in_h: i64) -> ShiftableLoad {
        ShiftableLoad {
            id: uuid::Uuid::new_v4(),
            asset_id: "washer".into(),
            power_kw: 2.0,
            duration_min: 60,
            earliest_start: now() + Duration::hours(start_in_h),
            latest_end: now() + Duration::hours(start_in_h + 4),
            mode,
            created_at: now(),
            updated_at: now(),
        }
    }

    #[test]
    fn each_request_is_served_as_its_mode_declares_with_its_target() {
        let mut session = EvSession::simulated("ev", 0.8, now(), now() + Duration::hours(5), now());
        session.mode = UserRequestMode::Opportunistic;
        let sessions = HashMap::from([("ev".to_string(), session)]);
        let heaters = BTreeMap::from([(
            "heater".to_string(),
            heater_target(UserRequestMode::ByDeadline),
        )]);
        let loads = [shiftable(UserRequestMode::Asap, 0)];
        let r = opportunistic_requests(now(), &sessions, &heaters, &loads);
        assert_eq!(r.dispatch_by_asset["ev"], SurplusOnly);
        assert_eq!(r.dispatch_by_asset["heater"], StartNow);
        assert_eq!(r.dispatch_by_asset["washer"], StartNow);
        assert_eq!(r.targets.ev_soc_target_frac_by_asset["ev"], 0.8);
        assert_eq!(r.targets.heater_ceiling_c_by_asset["heater"], 55.0);
    }

    #[test]
    fn a_surplus_only_heater_target_sets_no_ceiling_and_a_future_load_is_not_active() {
        let heaters = BTreeMap::from([(
            "heater".to_string(),
            heater_target(UserRequestMode::ByDeadlineFree),
        )]);
        let loads = [shiftable(UserRequestMode::Asap, 2)];
        let r = opportunistic_requests(now(), &HashMap::new(), &heaters, &loads);
        assert_eq!(r.dispatch_by_asset["heater"], SurplusOnly);
        assert!(r.targets.heater_ceiling_c_by_asset.is_empty());
        assert!(!r.dispatch_by_asset.contains_key("washer"));
    }
}
