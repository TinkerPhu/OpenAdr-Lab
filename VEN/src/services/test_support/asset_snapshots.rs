//! Test snapshots built the way `SimState::to_sim_snapshot` builds them:
//! capability, storage, forced power and values all come from the real asset,
//! so a fixture can never disagree with the asset about its own state.

use crate::assets::{Asset, AssetState};
use crate::controller::simulator_port::AssetSnapshot;

pub fn snapshot_from_asset(
    asset: &dyn Asset,
    state: AssetState,
    asset_type: &str,
    power_kw: f64,
    setpoint_kw: f64,
) -> AssetSnapshot {
    let cap = asset.capability(&state);
    let (available_discharge_kwh, available_charge_kwh) = match asset
        .as_request_resolvable()
        .and_then(|r| r.available_storage_kwh(&state))
    {
        Some((dis, ch)) => (Some(dis), Some(ch)),
        None => (None, None),
    };
    AssetSnapshot {
        power_kw,
        asset_type: asset_type.into(),
        cap_max_import_kw: cap.max_import_kw,
        cap_max_export_kw: cap.max_export_kw,
        available_discharge_kwh,
        available_charge_kwh,
        forced_power_kw: asset.forced_power_kw(&state),
        default_setpoint_kw: 0.0,
        setpoint_kw,
        values: asset.state_values(&state).into_iter().collect(),
    }
}
