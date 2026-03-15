/// Stage 4 — Dispatcher: translates FIRM plan slot allocations into sim setpoints
/// and accumulates actual delivered energy into packet profiles.
use crate::entities::asset::PlanTrigger;
use crate::entities::energy_packet::{EnergyPacket, EnergySnapshot, PacketStatus};
use crate::entities::plan::Plan;
use crate::simulator::SimSnapshot;
use chrono::{DateTime, Utc};
use uuid::Uuid;

/// Setpoints produced by the Dispatcher for the current plan slot.
/// None means "do not override reactor" for that asset.
#[derive(Debug, Default)]
pub struct DispatcherSetpoints {
    pub ev_kw: Option<f64>,
    pub battery_kw: Option<f64>,
    pub heater_kw: Option<f64>,
}

/// Compute setpoints from the current FIRM slot of the active plan.
/// Only returns Some(...) for assets that have an allocation in the current slot.
pub fn get_setpoints(
    plan: &Plan,
    packets: &[EnergyPacket],
    now: DateTime<Utc>,
) -> DispatcherSetpoints {
    let mut out = DispatcherSetpoints::default();

    // Find the FIRM slot that covers `now`
    let slot = match plan
        .firm_slots
        .iter()
        .find(|s| s.start <= now && now < s.end)
    {
        Some(s) => s,
        None => return out,
    };

    for alloc in &slot.allocations {
        // Battery allocations have no associated packet (packet_id = Uuid::nil())
        if alloc.asset_id == "battery" {
            out.battery_kw = Some(alloc.power_kw);
            continue;
        }

        // For packet-linked allocations, skip if the packet is terminal or missing
        if alloc.packet_id != Uuid::nil() {
            match packets.iter().find(|p| p.id == alloc.packet_id) {
                Some(pkt) if pkt.is_terminal() => continue,
                None => continue, // stale allocation; packet was removed
                _ => {}
            }
        }

        match alloc.asset_id.as_str() {
            "ev" => out.ev_kw = Some(alloc.power_kw),
            "heater" => out.heater_kw = Some(alloc.power_kw),
            _ => {}
        }
    }

    out
}

/// Post-tick: accumulate actual simulator power into packet profiles and
/// transition packet statuses (Scheduled→Active, Active→Completed, etc.).
///
/// Returns an optional PlanTrigger when a replan is warranted (e.g. completion).
pub fn update_packets(
    packets: &mut Vec<EnergyPacket>,
    sim: &SimSnapshot,
    dt_s: f64,
    now: DateTime<Utc>,
) -> Option<PlanTrigger> {
    let dt_h = dt_s / 3600.0;
    let mut trigger: Option<PlanTrigger> = None;

    for pkt in packets.iter_mut() {
        if pkt.is_terminal() {
            continue;
        }

        // Actual delivered power this tick from the simulator
        let actual_kw = actual_power_kw(pkt, sim);

        // Accumulate energy
        let prev_energy = pkt.past_energy_kwh();
        let new_energy = prev_energy + actual_kw * dt_h;

        pkt.past_power_profile.push(EnergySnapshot {
            ts: now,
            power_kw: actual_kw,
            cumulative_energy_kwh: new_energy,
        });
        pkt.updated_at = now;

        // Transition Scheduled → Active once energy starts flowing
        if pkt.status == PacketStatus::Scheduled && actual_kw > 0.01 {
            pkt.status = PacketStatus::Active;
        }

        // Completion check: target energy reached
        if pkt.target_energy_kwh > 0.0 && new_energy >= pkt.target_energy_kwh - 1e-4 {
            pkt.status = PacketStatus::Completed;
            trigger = Some(PlanTrigger::DeviceDeviation); // freed capacity → replan
            continue;
        }

        // Deadline check: past latest_end
        if let Some(latest) = pkt.latest_end() {
            if now > latest {
                let fill = pkt.fill();
                if fill >= 0.99 {
                    pkt.status = PacketStatus::Completed;
                } else {
                    pkt.status = PacketStatus::PartialCompleted;
                }
                trigger = Some(PlanTrigger::DeviceDeviation);
            }
        }
    }

    trigger
}

/// Extract the actual delivered power for a packet's asset from the simulator snapshot.
fn actual_power_kw(pkt: &EnergyPacket, sim: &SimSnapshot) -> f64 {
    sim.assets.get(&pkt.asset_id).map(|a| a.power_kw).unwrap_or(0.0)
}
