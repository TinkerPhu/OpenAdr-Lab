//! What the plan reads back for each forecast participant (PV array, base load): its own
//! forecast and used power per slot, and the site's used generation as their sum. The one
//! place that turns a forecast participant's solution into plan fields; it never schedules.

use crate::entities::plan::ForecastSlotPower;

use super::asset_port::{AssetMilpContext, ForecastRole, Participation};
use super::types::SolveOutput;

/// Each forecast participant's own power in slot `t`, signed (generation negative), in roster
/// order: its forecast, and what the solution uses of it (`sol`; without one, the forecast
/// itself).
pub(super) fn forecast_by_asset(
    asset_contexts: &[Box<dyn AssetMilpContext>],
    sol: Option<&SolveOutput>,
    t: usize,
) -> Vec<(String, ForecastSlotPower)> {
    asset_contexts
        .iter()
        .enumerate()
        .filter_map(|(i, ctx)| match ctx.participation() {
            Participation::Forecast { role, forecast_kw } => {
                let generation = role == ForecastRole::Generation;
                let sign = if generation { -1.0 } else { 1.0 };
                let forecast = sign * forecast_kw[t];
                let used_kw = sol.map_or(forecast, |s| s.assets[i].net_power_kw[t]);
                let power = ForecastSlotPower {
                    forecast_kw: forecast,
                    used_kw,
                    generation,
                };
                Some((ctx.asset_id().to_string(), power))
            }
            Participation::Dispatched => None,
        })
        .collect()
}

/// The generation the plan uses in a slot [kW]: every generating forecast participant's own
/// used generation, summed in roster order. A single participant's value passes through
/// unchanged; 0 kW without one.
pub(super) fn generation_used_kw(forecasts: &[(String, ForecastSlotPower)]) -> f64 {
    forecasts
        .iter()
        .filter(|(_, f)| f.generation)
        .map(|(_, f)| -f.used_kw)
        .reduce(|a, b| a + b)
        .unwrap_or(0.0)
}
