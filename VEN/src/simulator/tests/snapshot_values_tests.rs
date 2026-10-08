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
