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

const NEAR_ZERO_KW: f64 = 1e-3;
const ACTIVE_THRESHOLD_KW: f64 = 1e-2;
const COMPLETION_TOL_KWH: f64 = 1e-4;
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
/// NOTE: Asset history rows are pushed directly into each asset's per-asset ring buffer
/// in the tick loop (`entry.history.push(HistoryPoint { ... })`).
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
    let import_tariff = applicable
        .and_then(|r| r.import_tariff_eur_kwh)
        .unwrap_or(DEFAULT_IMPORT_PRICE);
    let co2_rate = applicable
        .and_then(|r| r.co2_g_kwh)
        .unwrap_or(DEFAULT_CO2_G_KWH);

    // ── 1. Update cumulative ledger ───────────────────────────────────────────

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

        // Pending/Scheduled → Active transition
        // MILP planner keeps packets at Pending (no explicit Scheduled assignment);
        // allow the transition from either pre-active status.
        if (pkt.status == PacketStatus::Scheduled || pkt.status == PacketStatus::Pending)
            && actual_kw > ACTIVE_THRESHOLD_KW
        {
            let from = format!("{:?}", pkt.status);
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
        if pkt.target_energy_kwh > 0.0 && new_energy >= pkt.target_energy_kwh - COMPLETION_TOL_KWH {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entities::energy_packet::{EnergyPacket, EnergySnapshot, PacketStatus, ValueCurve};
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

    fn make_scheduled_packet(asset_id: &str, target_kwh: f64) -> EnergyPacket {
        let now = Utc::now();
        let mut pkt = EnergyPacket::new(
            asset_id.to_string(),
            target_kwh,
            1.0,
            ValueCurve { comfort_rates: vec![], deadline_tiers: vec![], active_tier_index: 0 },
            now,
        );
        pkt.status = PacketStatus::Scheduled;
        pkt
    }

    // ── NEAR_ZERO_KW — ledger skip boundary ──────────────────────────────────

    #[test]
    fn ledger_skips_power_below_near_zero_kw() {
        // power = 0.5 × NEAR_ZERO_KW (0.0005 kW) → below threshold → no ledger entry
        let sub_threshold = NEAR_ZERO_KW * 0.5;
        let sim = make_sim("ev", sub_threshold);
        let mut ledger = HashMap::new();
        record_tick(&mut vec![], &mut ledger, &sim, &[], 1.0, Utc::now());
        assert!(ledger.is_empty(), "ledger must not accumulate sub-threshold power");
    }

    #[test]
    fn ledger_accumulates_power_above_near_zero_kw() {
        // power = 2.0 × NEAR_ZERO_KW (0.002 kW) → above threshold → entry created
        let above_threshold = NEAR_ZERO_KW * 2.0;
        let sim = make_sim("ev", above_threshold);
        let mut ledger = HashMap::new();
        record_tick(&mut vec![], &mut ledger, &sim, &[], 1.0, Utc::now());
        let entry = ledger.get("ev").expect("ledger must have an entry for above-threshold power");
        assert!(entry.energy_kwh > 0.0, "energy_kwh must be positive, got {}", entry.energy_kwh);
    }

    // ── ACTIVE_THRESHOLD_KW — Scheduled→Active boundary ─────────────────────

    #[test]
    fn scheduled_stays_below_active_threshold() {
        // power = 0.9 × ACTIVE_THRESHOLD_KW → packet stays Scheduled
        let below = ACTIVE_THRESHOLD_KW * 0.9;
        let sim = make_sim("ev", below);
        let mut ledger = HashMap::new();
        // Re-fetch via record_tick result: check status via the returned packet
        let mut packets = vec![make_scheduled_packet("ev", 10.0)];
        record_tick(&mut packets, &mut ledger, &sim, &[], 1.0, Utc::now());
        assert_eq!(packets[0].status, PacketStatus::Scheduled,
            "power below ACTIVE_THRESHOLD_KW must not trigger Scheduled→Active");
    }

    #[test]
    fn scheduled_transitions_above_active_threshold() {
        // power = 1.1 × ACTIVE_THRESHOLD_KW → packet transitions to Active
        let above = ACTIVE_THRESHOLD_KW * 1.1;
        let sim = make_sim("ev", above);
        let mut packets = vec![make_scheduled_packet("ev", 10.0)];
        let mut ledger = HashMap::new();
        record_tick(&mut packets, &mut ledger, &sim, &[], 1.0, Utc::now());
        assert_eq!(packets[0].status, PacketStatus::Active,
            "power above ACTIVE_THRESHOLD_KW must trigger Scheduled→Active");
    }

    // ── COMPLETION_TOL_KWH — packet completion boundary ──────────────────────

    #[test]
    fn packet_completes_within_completion_tolerance() {
        // Pre-fill packet to target - 0.5 × tolerance (within tolerance window).
        // With zero actual_kw the tick still pushes new_energy = prev_energy,
        // which satisfies: new_energy ≥ target - COMPLETION_TOL_KWH.
        let target = 1.0_f64;
        let prev_energy = target - COMPLETION_TOL_KWH * 0.5;
        let sim = make_sim("ev", 0.0);
        let mut pkt = make_scheduled_packet("ev", target);
        pkt.status = PacketStatus::Active;
        pkt.past_power_profile.push(EnergySnapshot {
            ts: Utc::now(),
            power_kw: 0.0,
            cumulative_energy_kwh: prev_energy,
        });
        let mut packets = vec![pkt];
        let mut ledger = HashMap::new();
        record_tick(&mut packets, &mut ledger, &sim, &[], 1.0, Utc::now());
        assert_eq!(packets[0].status, PacketStatus::Completed,
            "packet within COMPLETION_TOL_KWH of target must be marked Completed");
    }

    #[test]
    fn packet_does_not_complete_outside_tolerance() {
        // Pre-fill packet to target - 2.0 × tolerance (outside tolerance window).
        // Even with zero actual_kw, new_energy < target - COMPLETION_TOL_KWH.
        let target = 1.0_f64;
        let prev_energy = target - COMPLETION_TOL_KWH * 2.0;
        let sim = make_sim("ev", 0.0);
        let mut pkt = make_scheduled_packet("ev", target);
        pkt.status = PacketStatus::Active;
        pkt.past_power_profile.push(EnergySnapshot {
            ts: Utc::now(),
            power_kw: 0.0,
            cumulative_energy_kwh: prev_energy,
        });
        let mut packets = vec![pkt];
        let mut ledger = HashMap::new();
        record_tick(&mut packets, &mut ledger, &sim, &[], 1.0, Utc::now());
        assert_eq!(packets[0].status, PacketStatus::Active,
            "packet 2× COMPLETION_TOL_KWH from target must remain Active");
    }
}
