/// Dispatcher: translates FIRM plan slot allocations into per-asset setpoints.
///
/// Single responsibility: given the current plan, simulator assets, and capacity
/// constraints, produce a HashMap<asset_id, kW> that drives the simulator tick.
/// There is no concept of "reactor mode" here — the plan is the sole authority.
use crate::entities::capacity::OadrCapacityState;
use crate::entities::energy_packet::{EnergyPacket, EnergySnapshot, PacketStatus};
use crate::entities::plan::Plan;
use crate::entities::asset::PlanTrigger;
use crate::simulator::{AssetEntry, SimSnapshot};
use chrono::{DateTime, Utc};
use std::collections::HashMap;
use uuid::Uuid;

/// Build a setpoints map for all known assets based on the active plan.
///
/// Algorithm:
/// 1. Start with each asset's `default_setpoint()` from its current state.
/// 2. Find the FIRM slot covering `now` in the plan.
/// 3. Overwrite entries for assets that have an allocation in that slot.
/// 4. If a FIRM slot is not found, try the FLEXIBLE slot covering `now`.
/// 5. Enforce `export_limit_kw` on the `pv` key if capacity state has one.
pub fn build_setpoints(
    plan: &Plan,
    assets: &[AssetEntry],
    capacity: &OadrCapacityState,
    now: DateTime<Utc>,
) -> HashMap<String, f64> {
    // Start with defaults from current asset state
    let mut setpoints: HashMap<String, f64> = assets
        .iter()
        .map(|a| (a.id.clone(), a.state.default_setpoint()))
        .collect();

    // Try FIRM slot first, then FLEXIBLE
    let slot_allocs: Option<&Vec<crate::entities::plan::PacketAllocation>> = plan
        .firm_slots
        .iter()
        .find(|s| s.start <= now && now < s.end)
        .map(|s| &s.allocations)
        .or_else(|| {
            plan.flexible_slots
                .iter()
                .find(|s| s.start <= now && now < s.end)
                .map(|s| &s.allocations)
        });

    if let Some(allocs) = slot_allocs {
        for alloc in allocs {
            // Battery allocations have no associated packet
            if alloc.asset_id == "battery" {
                setpoints.insert("battery".to_string(), alloc.power_kw);
                continue;
            }
            // Skip stale allocations (terminal packet or missing)
            if alloc.packet_id != Uuid::nil() {
                // We don't have packet list here; caller should filter stale allocs
                // For now, trust the allocation if packet_id is set
            }
            setpoints.insert(alloc.asset_id.clone(), alloc.power_kw);
        }
    }

    // Enforce export capacity limit on PV
    if let Some(export_cap) = capacity.export_limit_kw {
        let pv_sp = setpoints.entry("pv".to_string()).or_insert(0.0);
        // PV export is negative in sign convention; cap absolute value
        if *pv_sp < -export_cap {
            *pv_sp = -export_cap;
        }
    }

    setpoints
}

/// Post-tick: accumulate actual simulator power into packet profiles and
/// transition packet statuses (Scheduled→Active, Active→Completed, etc.).
///
/// Returns an optional PlanTrigger when a replan is warranted (e.g. completion).
/// NOTE: This function will be superseded by `monitor::record_tick` in Phase 5 (T041).
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

        let actual_kw = sim.assets.get(&pkt.asset_id).map(|a| a.power_kw).unwrap_or(0.0);

        let prev_energy = pkt.past_energy_kwh();
        let new_energy = prev_energy + actual_kw * dt_h;

        pkt.past_power_profile.push(EnergySnapshot {
            ts: now,
            power_kw: actual_kw,
            cumulative_energy_kwh: new_energy,
        });
        pkt.updated_at = now;

        if pkt.status == PacketStatus::Scheduled && actual_kw > 0.01 {
            pkt.status = PacketStatus::Active;
        }

        if pkt.target_energy_kwh > 0.0 && new_energy >= pkt.target_energy_kwh - 1e-4 {
            pkt.status = PacketStatus::Completed;
            trigger = Some(PlanTrigger::DeviceDeviation);
            continue;
        }

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
