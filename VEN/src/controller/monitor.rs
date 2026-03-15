/// Stage 4 — Monitor: per-asset energy ledger accumulation.
///
/// Tracks energy delivered per asset per tick and accumulates cost/CO₂.
/// Deviation detection and timeout checks are deferred to Stage 5.
use crate::entities::rate_snapshot::RateSnapshot;
use crate::simulator::SimSnapshot;
use crate::state::AssetLedgerEntry;
use chrono::{DateTime, Utc};
use std::collections::HashMap;

const DEFAULT_IMPORT_PRICE: f64 = 0.20;
const DEFAULT_CO2_G_KWH: f64 = 300.0;

/// Update the per-asset energy ledger for one simulator tick.
pub fn update_ledger(
    ledger: &mut HashMap<String, AssetLedgerEntry>,
    sim: &SimSnapshot,
    rates: &[RateSnapshot],
    dt_s: f64,
    now: DateTime<Utc>,
) {
    let dt_h = dt_s / 3600.0;

    // Look up current import price and CO₂ intensity from planned rates
    let (import_price, co2_rate) = rates
        .iter()
        .find(|r| r.interval_start <= now && now < r.interval_end)
        .map(|r| {
            (
                r.import_price_eur_kwh.unwrap_or(DEFAULT_IMPORT_PRICE),
                r.co2_g_kwh.unwrap_or(DEFAULT_CO2_G_KWH),
            )
        })
        .unwrap_or((DEFAULT_IMPORT_PRICE, DEFAULT_CO2_G_KWH));

    for (asset_id, asset_snap) in &sim.assets {
        let kw = asset_snap.power_kw;
        if kw.abs() <= 1e-6 {
            continue;
        }

        let entry = ledger
            .entry(asset_id.clone())
            .or_insert_with(|| AssetLedgerEntry::new(asset_id));
        entry.energy_kwh += kw.abs() * dt_h;

        // Importing assets (positive kw) incur cost and CO₂
        if kw > 0.0 {
            entry.cost_eur += kw * dt_h * import_price;
            entry.co2_g += kw * dt_h * co2_rate;
        }
        entry.updated_at = Some(now);
    }
}
