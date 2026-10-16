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
