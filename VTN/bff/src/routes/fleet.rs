//! Serving the fleet's live state.
//!
//! Phase 0's query surface: what every VEN last said about itself, and the
//! fleet's summed power. The historical/report-sourced series of §7
//! (`?source=report`, `from`/`to`/`step`) needs the telemetry store, which is
//! the next step; this is the live half, which is what a dashboard opens with.

use axum::{extract::State, Json};
use serde_json::json;

use crate::AppCtx;

/// `GET /api/fleet/power` — every VEN's latest reading, and the fleet sum.
///
/// The sum is over what each VEN reported, signed: import positive, export
/// negative. A VEN that has said nothing contributes nothing and is listed
/// with a null value rather than a zero -- "we have not heard from it" and
/// "it is drawing nothing" are different facts, and a fleet total that
/// silently treats the first as the second is wrong in exactly the way that
/// is hardest to notice.
pub async fn fleet_power(State(ctx): State<AppCtx>) -> Json<serde_json::Value> {
    let state = ctx.fleet.read().await;

    let mut vens = Vec::with_capacity(state.len());
    let mut sum_w = 0.0_f64;
    let mut contributing = 0usize;

    for (name, live) in state.iter() {
        let net_power_w = live
            .telemetry
            .as_ref()
            .and_then(|t| t.get("grid")?.get("net_power_w")?.as_f64());
        if let Some(w) = net_power_w {
            sum_w += w;
            contributing += 1;
        }
        vens.push(json!({
            "venName": name,
            "netPowerW": net_power_w,
            "state": live.state,
            "receivedAt": live.received_at,
        }));
    }

    Json(json!({
        "vens": vens,
        "fleet": {
            "netPowerW": sum_w,
            // The sum is only as complete as the VENs behind it. Saying how
            // many contributed lets a reader judge it rather than trust it.
            "contributingVens": contributing,
            "knownVens": state.len(),
        }
    }))
}
