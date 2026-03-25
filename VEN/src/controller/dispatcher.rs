/// Dispatcher: translates FIRM plan slot allocations into per-asset setpoints.
///
/// Single responsibility: given the current plan, simulator assets, and capacity
/// constraints, produce a HashMap<asset_id, kW> that drives the simulator tick.
/// The plan is the sole authority.
use crate::assets::{AssetConfig, AssetState};
use crate::entities::capacity::OadrCapacityState;
use crate::entities::plan::Plan;
use crate::simulator::AssetEntry;
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
/// 5. If `heater_setpoint_c` override is set and the plan has no heater allocation,
///    compute ON/OFF setpoint based on current temperature vs. target.
/// 6. Enforce `export_limit_kw` on the `pv` key if capacity state has one.
pub fn build_setpoints(
    plan: &Plan,
    assets: &[AssetEntry],
    asset_configs: &[AssetConfig],
    capacity: &OadrCapacityState,
    heater_setpoint_c: Option<f64>,
    now: DateTime<Utc>,
) -> HashMap<String, f64> {
    // Start with defaults from current asset state
    let mut setpoints: HashMap<String, f64> = assets
        .iter()
        .zip(asset_configs.iter())
        .map(|(a, cfg)| (a.id.clone(), cfg.default_setpoint(&a.state)))
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

    let mut plan_allocated_heater = false;
    if let Some(allocs) = slot_allocs {
        for alloc in allocs {
            // Battery allocations have no associated packet
            if alloc.asset_id == "battery" {
                setpoints.insert("battery".to_string(), alloc.power_kw);
                continue;
            }
            if alloc.asset_id == "heater" {
                plan_allocated_heater = true;
            }
            // Skip stale allocations (terminal packet or missing)
            if alloc.packet_id != Uuid::nil() {
                // We don't have packet list here; caller should filter stale allocs
                // For now, trust the allocation if packet_id is set
            }
            setpoints.insert(alloc.asset_id.clone(), alloc.power_kw);
        }
    }

    // Heater setpoint override: compute ON/OFF based on current temp vs. target.
    // Only applies when the plan has no heater allocation for the current slot.
    if let Some(target_c) = heater_setpoint_c {
        if !plan_allocated_heater {
            if let Some(idx) = assets.iter().position(|a| a.id == "heater") {
                if let (AssetState::Heater(hs), AssetConfig::Heater(hcfg)) =
                    (&assets[idx].state, &asset_configs[idx])
                {
                    let power_kw = if hs.temperature_c < target_c {
                        hcfg.max_kw
                    } else {
                        0.0
                    };
                    setpoints.insert("heater".to_string(), power_kw);
                }
            }
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
