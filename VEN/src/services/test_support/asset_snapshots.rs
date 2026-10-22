//! Test snapshots built the way `SimState::to_sim_snapshot` builds them:
//! capability, storage, forced power and values all come from the real asset,
//! so a fixture can never disagree with the asset about its own state.

use crate::assets::{Asset, AssetState};
use crate::controller::sim_snapshot::AssetSnapshot;

pub fn snapshot_from_asset(
    asset: &dyn Asset,
    state: AssetState,
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
        asset_type: asset.kind(),
        cap_max_import_kw: cap.max_import_kw,
        cap_max_export_kw: cap.max_export_kw,
        available_discharge_kwh,
        available_charge_kwh,
        forced_power_kw: asset.forced_power_kw(&state),
        response: cap.response.clone(),
        default_setpoint_kw: 0.0,
        setpoint_kw,
        values: asset.state_values(&state).into_iter().collect(),
        history: asset.history_view(&state),
        embodied_co2_g_kwh: asset.embodied_co2_g_kwh(),
        emergency_what_ifs: asset.emergency_what_ifs(&state),
        ac_ceiling_kw: asset.ac_ceiling_kw(),
        arbiter_lever: asset.arbiter_lever(),
        surplus_claim_rank: asset.surplus_claim_rank(),
        full_effort_time_to_target_s: asset.full_effort_time_to_target_s(&state),
        dispatch_without_request: asset.dispatch_without_request(&state),
        unserved_energy_kwh: asset.unserved_energy_kwh(&state),
        stored_energy_kwh: asset.stored_energy_kwh(&state),
        roster_index: 0,
    }
}
