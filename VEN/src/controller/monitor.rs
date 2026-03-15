/// Stage 4/5 — Monitor: per-asset energy ledger + packet accounting.
///
/// `record_tick` supersedes both `update_ledger` and `dispatcher::update_packets`.
/// It attributes measured energy to active packets, updates the cumulative ledger,
/// fires PacketTransition events on status changes, and returns a PlanTrigger when
/// a replan is warranted.
use crate::controller::trace::ControllerEvent;
use crate::entities::asset::PlanTrigger;
use crate::entities::energy_packet::{EnergyPacket, EnergySnapshot, PacketStatus};
use crate::entities::tariff_snapshot::TariffSnapshot;
use crate::simulator::SimSnapshot;
use crate::state::AssetLedgerEntry;
use chrono::{DateTime, Utc};
use std::collections::HashMap;

const DEFAULT_IMPORT_PRICE: f64 = 0.20;
const DEFAULT_CO2_G_KWH: f64 = 300.0;

/// Consolidated per-tick accounting:
///
/// 1. Update the per-asset cumulative energy ledger.
/// 2. Attribute measured power to active packets (Scheduled→Active, Completed, etc.).
/// 3. Collect PacketTransition events for any status changes.
///
/// Returns `(Option<PlanTrigger>, Vec<ControllerEvent>)`.
/// The caller is responsible for pushing the returned events into the controller trace.
///
/// NOTE: Asset history rows are pushed separately in the tick loop via
/// `state.push_asset_row(...)` to avoid cloning the full ControllerTrace buffer.
pub fn record_tick(
    packets: &mut Vec<EnergyPacket>,
    ledger: &mut HashMap<String, AssetLedgerEntry>,
    sim: &SimSnapshot,
    tariffs: &[TariffSnapshot],
    dt_s: f64,
    now: DateTime<Utc>,
) -> (Option<PlanTrigger>, Vec<ControllerEvent>) {
    let dt_h = dt_s / 3600.0;
    let mut trigger: Option<PlanTrigger> = None;
    let mut events: Vec<ControllerEvent> = Vec::new();

    // Find applicable tariff for this tick
    let applicable = tariffs
        .iter()
        .find(|r| r.interval_start <= now && now < r.interval_end);
    let import_price = applicable
        .and_then(|r| r.import_price_eur_kwh)
        .unwrap_or(DEFAULT_IMPORT_PRICE);
    let co2_rate = applicable
        .and_then(|r| r.co2_g_kwh)
        .unwrap_or(DEFAULT_CO2_G_KWH);

    // ── 1. Update cumulative ledger ───────────────────────────────────────────

    for (asset_id, asset_snap) in &sim.assets {
        let kw = asset_snap.power_kw;
        if kw.abs() <= 1e-6 {
            continue;
        }
        let entry = ledger
            .entry(asset_id.clone())
            .or_insert_with(|| AssetLedgerEntry::new(asset_id));
        entry.energy_kwh += kw.abs() * dt_h;
        if kw > 0.0 {
            entry.cost_eur += kw * dt_h * import_price;
            entry.co2_g += kw * dt_h * co2_rate;
        }
        entry.updated_at = Some(now);
    }

    // ── 2. Attribute measured power to packets ────────────────────────────────

    for pkt in packets.iter_mut() {
        if pkt.is_terminal() {
            continue;
        }

        let actual_kw = sim
            .assets
            .get(&pkt.asset_id)
            .map(|a| a.power_kw)
            .unwrap_or(0.0);

        let prev_energy = pkt.past_energy_kwh();
        let new_energy = prev_energy + actual_kw * dt_h;

        pkt.past_power_profile.push(EnergySnapshot {
            ts: now,
            power_kw: actual_kw,
            cumulative_energy_kwh: new_energy,
        });
        pkt.updated_at = now;

        // Scheduled → Active transition
        if pkt.status == PacketStatus::Scheduled && actual_kw > 0.01 {
            let from = "Scheduled".to_string();
            pkt.status = PacketStatus::Active;
            events.push(ControllerEvent::PacketTransition {
                ts: now,
                packet_id: pkt.id,
                asset_id: pkt.asset_id.clone(),
                from_status: from,
                to_status: "Active".to_string(),
            });
        }

        // Completion check: energy target reached
        if pkt.target_energy_kwh > 0.0 && new_energy >= pkt.target_energy_kwh - 1e-4 {
            let from = format!("{:?}", pkt.status);
            pkt.status = PacketStatus::Completed;
            trigger = Some(PlanTrigger::DeviceDeviation);
            events.push(ControllerEvent::PacketTransition {
                ts: now,
                packet_id: pkt.id,
                asset_id: pkt.asset_id.clone(),
                from_status: from,
                to_status: "Completed".to_string(),
            });
            continue;
        }

        // Completion check: deadline passed
        if let Some(latest) = pkt.latest_end() {
            if now > latest {
                let fill = pkt.fill();
                let from = format!("{:?}", pkt.status);
                let to = if fill >= 0.99 {
                    pkt.status = PacketStatus::Completed;
                    "Completed"
                } else {
                    pkt.status = PacketStatus::PartialCompleted;
                    "PartialCompleted"
                };
                trigger = Some(PlanTrigger::DeviceDeviation);
                events.push(ControllerEvent::PacketTransition {
                    ts: now,
                    packet_id: pkt.id,
                    asset_id: pkt.asset_id.clone(),
                    from_status: from,
                    to_status: to.to_string(),
                });
            }
        }
    }

    (trigger, events)
}
