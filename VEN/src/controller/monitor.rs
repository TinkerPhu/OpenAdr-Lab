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

    // EV (consumption only; negative current means fault — ignore)
    if let Some(ev) = &sim.ev {
        let kw = ev.current_kw.max(0.0);
        if kw > 1e-6 {
            let entry = ledger
                .entry("ev".to_string())
                .or_insert_with(|| AssetLedgerEntry::new("ev"));
            entry.energy_kwh += kw * dt_h;
            entry.cost_eur += kw * dt_h * import_price;
            entry.co2_g += kw * dt_h * co2_rate;
            entry.updated_at = Some(now);
        }
    }

    // Heater (consumption only)
    if let Some(h) = &sim.heater {
        let kw = h.current_kw.max(0.0);
        if kw > 1e-6 {
            let entry = ledger
                .entry("heater".to_string())
                .or_insert_with(|| AssetLedgerEntry::new("heater"));
            entry.energy_kwh += kw * dt_h;
            entry.cost_eur += kw * dt_h * import_price;
            entry.co2_g += kw * dt_h * co2_rate;
            entry.updated_at = Some(now);
        }
    }

    // Battery (bidirectional: positive = charging/import, negative = discharging/export)
    if let Some(bat) = &sim.battery {
        if bat.current_kw.abs() > 1e-6 {
            let entry = ledger
                .entry("battery".to_string())
                .or_insert_with(|| AssetLedgerEntry::new("battery"));
            entry.energy_kwh += bat.current_kw.abs() * dt_h;
            if bat.current_kw > 0.0 {
                // Charging costs money
                entry.cost_eur += bat.current_kw * dt_h * import_price;
                entry.co2_g += bat.current_kw * dt_h * co2_rate;
            }
            entry.updated_at = Some(now);
        }
    }

    // PV (generation — track energy produced, no cost)
    if let Some(pv) = &sim.pv {
        let kw = pv.current_kw.max(0.0);
        if kw > 1e-6 {
            let entry = ledger
                .entry("pv".to_string())
                .or_insert_with(|| AssetLedgerEntry::new("pv"));
            entry.energy_kwh += kw * dt_h;
            entry.updated_at = Some(now);
        }
    }
}
