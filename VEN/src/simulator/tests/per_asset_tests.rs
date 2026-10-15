//! W5a/W5b: every asset is a lever, a tick input and a feed recipient under its own id.

use super::super::*;
use crate::assets::heater::{Heater, HeaterEmergencyMode};
use crate::entities::asset::ArbiterLeverKind;
use crate::entities::asset_params::{AssetParams, BaseLoadParams, PvParams};
use crate::simulator::AssetTickFeeds;

/// W5a: each asset declares the arbiter lever it offers, and the snapshot carries it with the
/// asset's roster position; nothing decides by id which asset is a lever.

#[test]
fn to_sim_snapshot_carries_each_assets_declared_lever_and_roster_position() {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("profiles")
        .join("test.yaml");
    let profile: crate::profile::Profile =
        serde_yaml::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let params = profile.asset_params();
    let snap = SimState::from_params(&params, Utc::now()).to_sim_snapshot();

    let kind_of = |id: &str| snap.assets[id].arbiter_lever.map(|l| l.kind);
    assert_eq!(kind_of("battery"), Some(ArbiterLeverKind::Storage));
    assert_eq!(kind_of("ev"), Some(ArbiterLeverKind::SurplusCharge));
    assert_eq!(kind_of("heater"), Some(ArbiterLeverKind::PausableLoad));
    assert_eq!(kind_of("pv"), None);
    assert_eq!(kind_of("base_load"), None);

    let battery = snap.assets["battery"].arbiter_lever.unwrap();
    assert!(battery.rated_import_kw > 0.0 && battery.rated_export_kw > 0.0);
    assert_eq!(
        snap.assets["heater"].arbiter_lever.unwrap().rated_export_kw,
        0.0
    );

    let in_order: Vec<&str> = snap
        .assets_in_roster_order()
        .iter()
        .map(|(id, _)| *id)
        .collect();
    let roster: Vec<String> = params.iter().map(|p| p.id().to_string()).collect();
    assert_eq!(in_order, roster);
}

// W5a: a per-asset tick input (the arbiter's emergency mode) reaches only its own asset.

fn two_heaters() -> SimState {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("profiles")
        .join("test.yaml");
    let profile: crate::profile::Profile =
        serde_yaml::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let heater = profile
        .asset_params()
        .into_iter()
        .find_map(|p| match p {
            AssetParams::Heater(h) => Some(h),
            _ => None,
        })
        .unwrap();
    let named = |id: &str| {
        let mut h = heater.clone();
        h.id = id.to_string();
        AssetParams::Heater(h)
    };
    SimState::from_params(&[named("heater-a"), named("heater-b")], Utc::now())
}

fn mode_of(sim: &SimState, id: &str) -> HeaterEmergencyMode {
    let (_, asset) = sim.find_asset(id).unwrap();
    asset
        .as_any()
        .downcast_ref::<Heater>()
        .unwrap()
        .emergency_mode
}

#[test]
fn a_heaters_emergency_mode_reaches_only_that_heater() {
    let mut sim = two_heaters();
    let now = Utc::now();
    sim.tick(TickInputs {
        heater_emergency_by_asset: HashMap::from([(
            "heater-a".to_string(),
            (Some(true), Some(false)),
        )]),
        ..TickInputs::new(30.0, now, HashMap::new())
    });
    assert_eq!(mode_of(&sim, "heater-a"), HeaterEmergencyMode::Curtail);
    assert_eq!(mode_of(&sim, "heater-b"), HeaterEmergencyMode::Normal);
}

// W5b: each PV array and base load is driven by its own feeds.

fn array(id: &str, inverter_max_kw: f64) -> AssetParams {
    AssetParams::Pv(PvParams {
        id: id.to_string(),
        rated_kw: inverter_max_kw,
        inverter_max_kw,
        co2_g_kwh: 0.0,
    })
}

fn power_kw(sim: &SimState, id: &str) -> f64 {
    sim.asset(id).unwrap().last_power_kw
}

#[test]
fn each_array_follows_its_own_weather_and_only_the_measured_one_its_reading() {
    let now = Utc::now();
    let mut sim = SimState::from_params(&[array("pv-east", 8.0), array("pv-west", 8.0)], now);
    let feeds = AssetTickFeeds {
        weather_pv_kw: HashMap::from([("pv-east".into(), 5.0), ("pv-west".into(), 2.0)]),
        measured_kw: HashMap::from([("pv-west".into(), 3.0)]),
        ..AssetTickFeeds::default()
    };
    sim.tick(TickInputs {
        feeds,
        ..TickInputs::new(1.0, now, HashMap::new())
    });
    assert!(
        (power_kw(&sim, "pv-east") - (-5.0)).abs() < 1e-9,
        "its own weather"
    );
    assert!(
        (power_kw(&sim, "pv-west") - (-3.0)).abs() < 1e-9,
        "its own reading"
    );
}

#[test]
fn a_site_generation_limit_is_split_by_the_arrays_ac_ceilings() {
    let now = Utc::now();
    let mut sim = SimState::from_params(&[array("pv-a", 6.0), array("pv-b", 4.0)], now);
    let feeds = AssetTickFeeds {
        weather_pv_kw: HashMap::from([("pv-a".into(), 6.0), ("pv-b".into(), 4.0)]),
        ..AssetTickFeeds::default()
    };
    sim.tick(TickInputs {
        feeds,
        pv_generation_limit_override: Some(-5.0),
        ..TickInputs::new(1.0, now, HashMap::new())
    });
    assert!(
        (power_kw(&sim, "pv-a") - (-3.0)).abs() < 1e-9,
        "6/10 of 5 kW"
    );
    assert!(
        (power_kw(&sim, "pv-b") - (-2.0)).abs() < 1e-9,
        "4/10 of 5 kW"
    );
}

#[test]
fn two_base_loads_follow_their_own_heuristic() {
    let now = Utc::now();
    let load = |id: &str| {
        AssetParams::BaseLoad(BaseLoadParams {
            id: id.to_string(),
            ..BaseLoadParams::default()
        })
    };
    let mut sim = SimState::from_params(&[load("house"), load("workshop")], now);
    let mut feeds = AssetTickFeeds::heuristic_flat_kw("house", 1.5);
    feeds
        .heuristic
        .extend(AssetTickFeeds::heuristic_flat_kw("workshop", 0.25).heuristic);
    sim.tick(TickInputs {
        feeds,
        base_load_alpha: 1.0,
        ..TickInputs::new(1.0, now, HashMap::new())
    });
    assert!((power_kw(&sim, "house") - 1.5).abs() < 1e-9);
    assert!((power_kw(&sim, "workshop") - 0.25).abs() < 1e-9);
}
