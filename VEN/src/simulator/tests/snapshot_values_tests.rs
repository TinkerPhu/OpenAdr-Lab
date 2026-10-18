//! `AssetSnapshot.values` is flattened into the same JSON object as the typed fields, so a
//! `state_values()` key named like a typed field silently overwrites it in `GET /sim` (R-116:
//! a shiftable load's rated `power_kw` hid its actual power). No asset kind may do that.

use super::super::*;
use crate::assets::ShiftableLoadAsset;
use crate::entities::asset_params::{
    AssetParams, BaseLoadParams, BatteryParams, EvParams, HeaterParams, PvParams,
};

#[test]
fn typed_answers_are_carried_only_by_the_assets_that_have_them() {
    let params = [
        AssetParams::Battery(BatteryParams::default()),
        AssetParams::Ev(EvParams::default()),
        AssetParams::Heater(HeaterParams::default()),
        AssetParams::Pv(PvParams::default()),
        AssetParams::BaseLoad(BaseLoadParams::default()),
    ];
    let snapshot = SimState::from_params(&params, Utc::now()).to_sim_snapshot();
    for (id, asset) in &snapshot.assets {
        let is_heater = id == crate::ids::ASSET_HEATER;
        let is_pv = id == crate::ids::ASSET_PV;
        assert_eq!(
            asset.emergency_what_ifs.is_some(),
            is_heater,
            "what-ifs on '{id}'"
        );
        assert_eq!(asset.ac_ceiling_kw.is_some(), is_pv, "AC ceiling on '{id}'");
    }
}

#[test]
fn no_asset_kind_reports_a_state_value_named_like_a_typed_snapshot_field() {
    let params = [
        AssetParams::Battery(BatteryParams::default()),
        AssetParams::Ev(EvParams::default()),
        AssetParams::Heater(HeaterParams::default()),
        AssetParams::Pv(PvParams::default()),
        AssetParams::BaseLoad(BaseLoadParams::default()),
    ];
    let mut sim = SimState::from_params(&params, Utc::now());
    let load = ShiftableLoadAsset {
        power_kw: 2.0,
        duration_min: 60,
        earliest_start: Utc::now(),
        latest_end: Utc::now() + chrono::Duration::hours(4),
    };
    sim.add_shiftable("wm", load).unwrap();

    let snapshot = sim.to_sim_snapshot();
    assert!(snapshot.assets.len() >= 6, "every asset kind is covered");
    for (id, asset) in &snapshot.assets {
        let mut typed = asset.clone();
        typed.values.clear();
        let typed = serde_json::to_value(&typed).unwrap();
        let typed_keys = typed.as_object().unwrap();
        for key in asset.values.keys() {
            assert!(
                !typed_keys.contains_key(key),
                "asset '{id}': state value '{key}' collides with the typed snapshot field of the same name"
            );
        }
    }
}

/// One surplus-claim order for plan attribution and live dispatch (openspec
/// `opportunistic-objective` D3): each kind's snapshot carries its kind's rank, EV first and
/// storage last; PV and base load never take surplus.
#[test]
fn every_asset_carries_its_kinds_surplus_claim_rank() {
    let params = [
        AssetParams::Battery(BatteryParams::default()),
        AssetParams::Ev(EvParams::default()),
        AssetParams::Heater(HeaterParams::default()),
        AssetParams::Pv(PvParams::default()),
        AssetParams::BaseLoad(BaseLoadParams::default()),
    ];
    let snapshot = SimState::from_params(&params, Utc::now()).to_sim_snapshot();
    let rank = |id: &str| snapshot.assets[id].surplus_claim_rank;
    assert_eq!(rank(crate::ids::ASSET_EV), Some(0));
    assert_eq!(rank(crate::ids::ASSET_HEATER), Some(1));
    assert_eq!(rank(crate::ids::ASSET_BATTERY), Some(3));
    assert_eq!(rank(crate::ids::ASSET_PV), None);
    assert_eq!(rank(crate::ids::ASSET_BASE_LOAD), None);
    assert_eq!(
        crate::assets::Asset::surplus_claim_rank(&ShiftableLoadAsset {
            power_kw: 2.0,
            duration_min: 60,
            earliest_start: Utc::now(),
            latest_end: Utc::now() + chrono::Duration::hours(4),
        }),
        Some(2)
    );
}

/// The opportunistic pass reads only these snapshot fields to decide what an asset can take
/// or give (D2), so each asset must say "nothing" through them when that is the truth.
#[test]
fn snapshot_capability_says_nothing_to_absorb_or_give_when_an_asset_cannot() {
    let snap = |params: AssetParams, id: &str| {
        SimState::from_params(&[params], Utc::now())
            .to_sim_snapshot()
            .assets[id]
            .clone()
    };
    let full = snap(
        AssetParams::Battery(BatteryParams {
            initial_soc_frac: 1.0,
            ..BatteryParams::default()
        }),
        crate::ids::ASSET_BATTERY,
    );
    assert_eq!(
        full.cap_max_import_kw, 0.0,
        "a full battery takes no surplus"
    );
    let empty_params = BatteryParams::default();
    let empty = snap(
        AssetParams::Battery(BatteryParams {
            initial_soc_frac: empty_params.min_soc_frac,
            ..empty_params
        }),
        crate::ids::ASSET_BATTERY,
    );
    assert_eq!(
        empty.cap_max_export_kw, 0.0,
        "an empty battery covers no import"
    );

    let at_target = snap(
        AssetParams::Ev(EvParams {
            initial_soc_frac: 0.8,
            soc_target_frac: 0.8,
            ..EvParams::default()
        }),
        crate::ids::ASSET_EV,
    );
    assert_eq!(
        at_target.cap_max_import_kw, 0.0,
        "an EV at its target takes no surplus"
    );
    let ev = snap(AssetParams::Ev(EvParams::default()), crate::ids::ASSET_EV);
    let min_kw = EvParams::default().min_charge_kw;
    assert_eq!(
        ev.power_when_command_lands_kw(min_kw * 0.5),
        0.0,
        "a surplus below the charger's minimum rate is not taken"
    );

    let hot_params = HeaterParams::default();
    let hot = snap(
        AssetParams::Heater(HeaterParams {
            temp_initial_c: hot_params.temp_max_c,
            ..hot_params
        }),
        crate::ids::ASSET_HEATER,
    );
    assert_eq!(
        hot.cap_max_import_kw, 0.0,
        "a heater at its ceiling takes no surplus"
    );
}

/// An unplugged EV takes no surplus.
#[test]
fn an_unplugged_ev_takes_no_surplus() {
    let mut sim = SimState::from_params(&[AssetParams::Ev(EvParams::default())], Utc::now());
    if let Some(entry) = sim.asset_mut(crate::ids::ASSET_EV) {
        if let crate::assets::AssetState::Ev(s) = &mut entry.state {
            s.plugged = false;
        }
    }
    let snap = sim.to_sim_snapshot();
    assert_eq!(snap.assets[crate::ids::ASSET_EV].cap_max_import_kw, 0.0);
}
