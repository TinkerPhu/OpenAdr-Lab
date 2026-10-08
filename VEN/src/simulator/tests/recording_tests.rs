//! What the sim records and what is injected into it from outside a tick (`recording.rs`).

use super::super::*;
use crate::assets::LoadWindowStats;
use crate::entities::asset_params::{AssetParams, BaseLoadParams, BatteryParams};

fn sim() -> SimState {
    SimState::from_params(
        &[
            AssetParams::Battery(BatteryParams::default()),
            AssetParams::BaseLoad(BaseLoadParams::default()),
        ],
        Utc::now(),
    )
}

#[test]
fn record_history_pushes_one_point_per_asset_with_its_power_and_state() {
    let mut sim = sim();
    sim.asset_mut(crate::ids::ASSET_BATTERY)
        .unwrap()
        .last_power_kw = 2.5;
    let now = Utc::now();

    sim.record_history(now, 10.0, -5.0);

    for entry in &sim.assets {
        assert_eq!(entry.history.len(), 1, "one point for {}", entry.id);
    }
    let battery = sim.asset(crate::ids::ASSET_BATTERY).unwrap();
    let point = battery.history.latest().expect("a recorded point");
    assert_eq!((point.ts, point.power_kw), (now, 2.5));
}

#[test]
fn record_history_updates_the_grid_asset_in_kw_with_the_signed_limits() {
    let mut sim = sim();
    sim.grid.net_power_w = 3_000.0;

    sim.record_history(Utc::now(), 10.0, -5.0);

    let grid = &sim.grid_asset.state;
    assert_eq!(grid.net_power_kw, 3.0, "watts converted once, by the meter");
    assert_eq!((grid.import_limit_kw, grid.export_limit_kw), (10.0, -5.0));
}

#[test]
fn record_history_appends_on_every_call() {
    let mut sim = sim();
    sim.record_history(Utc::now(), 10.0, -5.0);
    sim.record_history(Utc::now(), 10.0, -5.0);
    let battery = sim.asset(crate::ids::ASSET_BATTERY).unwrap();
    assert_eq!(battery.history.len(), 2);
}

#[test]
fn set_base_load_observed_window_shows_up_in_the_base_loads_key_features() {
    let mut sim = sim();
    sim.set_base_load_observed_window(LoadWindowStats::from_power_kw([0.2, 0.4]));
    let (entry, cfg) = sim.find_asset(crate::ids::ASSET_BASE_LOAD).unwrap();
    let features = cfg.key_features(&entry.state);
    assert_eq!(features.len(), 2, "avg and max");
}

#[test]
fn set_base_load_observed_window_is_a_no_op_without_a_base_load() {
    let mut sim = SimState::from_params(
        &[AssetParams::Battery(BatteryParams::default())],
        Utc::now(),
    );
    sim.set_base_load_observed_window(LoadWindowStats::from_power_kw([1.0]));
    assert_eq!(sim.assets.len(), 1);
}
