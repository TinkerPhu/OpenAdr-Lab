//! The per-asset capability route. Reads the asset's own answer through `SimReadPort`
//! and serializes it whole; never computes or adjusts a range itself.

use axum::{
    extract::{Path, State},
    response::IntoResponse,
    Json,
};

use crate::app_ctx::SimRead;

use crate::routes::error::unknown_asset;

/// GET /capability/:asset_id — point-in-time feasible power range for one asset (Phase A).
/// Returns `{"max_import_kw": ..., "max_export_kw": ..., "min_import_kw": ...,
/// "min_export_kw": ..., "is_fixed": ..., "adjustability": ..., "key_features":
/// [{"label": ..., "value": ...}]}` plus the asset's
/// declared setpoint response, flattened (`power_steps_kw`, `step_rule`,
/// `snap_to_zero_below_kw`, `power_next_tick_kw` — R-81). BL-27: `adjustability`
/// is a static control-mode classification, not part of the live feasible range.
pub async fn get_asset_capability(
    State(sim_read): State<SimRead>,
    Path(asset_id): Path<String>,
) -> impl IntoResponse {
    match sim_read.asset_capability(&asset_id).await {
        None => unknown_asset(&asset_id),
        // The asset's own display facts and default request targets travel with its range
        // (`AssetCapabilityView`), serialized whole: no second list to keep in step (R-123).
        Some(view) => Json(view).into_response(),
    }
}
