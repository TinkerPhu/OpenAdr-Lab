/// Stage 4/5 — Monitor: per-asset energy ledger update.
///
/// `record_tick` updates the cumulative per-asset energy ledger from the
/// current simulation snapshot. Packet attribution has been removed;
/// device sessions (EvSession, HeaterTarget) are managed directly.
use crate::entities::tariff_snapshot::TariffSnapshot;
use crate::simulator::SimSnapshot;
use crate::state::AssetLedgerEntry;
use chrono::{DateTime, Utc};

const NEAR_ZERO_KW: f64 = 1e-3;
use std::collections::HashMap;

const DEFAULT_IMPORT_PRICE: f64 = 0.20;
const DEFAULT_CO2_G_KWH: f64 = 300.0;

/// Update the per-asset cumulative energy ledger from the current sim snapshot.
pub fn record_tick(
    ledger: &mut HashMap<String, AssetLedgerEntry>,
    sim: &SimSnapshot,
    tariffs: &[TariffSnapshot],
    dt_s: f64,
    now: DateTime<Utc>,
) {
    let dt_h = dt_s / 3600.0;

    // Find applicable tariff for this tick
    let applicable = tariffs
        .iter()
        .find(|r| r.interval_start <= now && now < r.interval_end);
    let import_tariff = applicable
        .and_then(|r| r.import_tariff_eur_kwh)
        .unwrap_or(DEFAULT_IMPORT_PRICE);
    let co2_rate = applicable
        .and_then(|r| r.co2_g_kwh)
        .unwrap_or(DEFAULT_CO2_G_KWH);

    for (asset_id, asset_snap) in &sim.assets {
        let kw = asset_snap.power_kw;
        if kw.abs() <= NEAR_ZERO_KW {
            continue;
        }
        let entry = ledger
            .entry(asset_id.clone())
            .or_insert_with(|| AssetLedgerEntry::new(asset_id));
        entry.energy_kwh += kw.abs() * dt_h;
        if kw > 0.0 {
            entry.cost_eur += kw * dt_h * import_tariff;
            entry.co2_g += kw * dt_h * co2_rate;
        }
        entry.updated_at = Some(now);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::simulator::{AssetSnapshot, GridSnapshot, SimSnapshot};
    use chrono::Utc;
    use std::collections::HashMap;

    fn make_sim(asset_id: &str, power_kw: f64) -> SimSnapshot {
        SimSnapshot {
            ts: Utc::now(),
            grid: GridSnapshot { net_power_w: 0.0, voltage_v: 230.0, import_kwh: 0.0, export_kwh: 0.0 },
            assets: HashMap::from([(
                asset_id.to_string(),
                AssetSnapshot { power_kw, values: HashMap::new() },
            )]),
        }
    }

    #[test]
    fn ledger_skips_power_below_near_zero_kw() {
        let sub_threshold = NEAR_ZERO_KW * 0.5;
        let sim = make_sim("ev", sub_threshold);
        let mut ledger = HashMap::new();
        record_tick(&mut ledger, &sim, &[], 1.0, Utc::now());
        assert!(ledger.is_empty(), "ledger must not accumulate sub-threshold power");
    }

    #[test]
    fn ledger_accumulates_power_above_near_zero_kw() {
        let above_threshold = NEAR_ZERO_KW * 2.0;
        let sim = make_sim("ev", above_threshold);
        let mut ledger = HashMap::new();
        record_tick(&mut ledger, &sim, &[], 1.0, Utc::now());
        let entry = ledger.get("ev").expect("ledger must have an entry for above-threshold power");
        assert!(entry.energy_kwh > 0.0, "energy_kwh must be positive, got {}", entry.energy_kwh);
    }
}
