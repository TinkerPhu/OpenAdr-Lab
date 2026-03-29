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
/// 7. Apply opportunistic surplus EV charging (see `apply_surplus_ev_overlay`).
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
    let mut plan_allocated_ev = false;
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
            if alloc.asset_id == "ev" {
                plan_allocated_ev = true;
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

    // Opportunistic surplus EV charging: redirect live PV export to EV when no
    // plan-level EV allocation is active.
    apply_surplus_ev_overlay(&mut setpoints, assets, asset_configs, plan_allocated_ev);

    setpoints
}

/// Opportunistic surplus EV charging overlay.
///
/// When PV is exporting more power than base load consumes, offer the surplus
/// to the EV (up to its max charge rate). No EnergyPacket is created — this is
/// dispatcher-only and does not appear in the plan or VTN reports.
///
/// Does nothing when:
/// - `plan_has_ev_allocation` is true (plan-level commitment takes priority)
/// - EV is unplugged
/// - EV SoC has reached its target
/// - Surplus is below the 100 W noise floor
pub fn apply_surplus_ev_overlay(
    setpoints: &mut HashMap<String, f64>,
    assets: &[AssetEntry],
    asset_configs: &[AssetConfig],
    plan_has_ev_allocation: bool,
) {
    if plan_has_ev_allocation {
        return;
    }
    // Live PV power (negative = export) and base load (positive = import).
    let pv_kw = assets.iter().find(|a| a.id == "pv").map(|a| a.last_power_kw).unwrap_or(0.0);
    let base_kw = assets
        .iter()
        .find(|a| a.id == "base_load")
        .map(|a| a.last_power_kw)
        .unwrap_or(0.0);
    // surplus_kw: generation that exceeds base consumption (positive = excess export)
    let surplus_kw = (-(pv_kw + base_kw)).max(0.0);
    if surplus_kw < 0.1 {
        return;
    }
    let Some(idx) = assets.iter().position(|a| a.id == "ev") else {
        return;
    };
    let (AssetState::Ev(es), AssetConfig::Ev(ecfg)) =
        (&assets[idx].state, &asset_configs[idx])
    else {
        return;
    };
    if !es.plugged || es.soc >= ecfg.soc_target {
        return;
    }
    let charge_kw = surplus_kw.min(ecfg.max_charge_kw);
    setpoints.insert("ev".to_string(), charge_kw);
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assets::{
        AssetConfig, AssetState, BaseLoad, BaseLoadState, EvCharger, EvState, PvInverter, PvState,
    };
    use crate::simulator::{energy::EnergyCounter, AssetEntry};
    use crate::assets::AssetHistoryBuffer;

    fn ev_entry(soc: f64, plugged: bool, soc_target: f64) -> (AssetEntry, AssetConfig) {
        let cfg = EvCharger {
            max_charge_kw: 7.4,
            max_discharge_kw: 0.0,
            battery_kwh: 60.0,
            soc_target,
            soc_target_profile: soc_target,
            default_charge_kw: 0.0,
            min_soc: 0.0,
        };
        let entry = AssetEntry {
            id: "ev".to_string(),
            state: AssetState::Ev(EvState { soc, plugged, actual_power_kw: 0.0 }),
            setpoint_kw: 0.0,
            last_power_kw: 0.0,
            energy: EnergyCounter::new(),
            history: AssetHistoryBuffer::new(0),
        };
        (entry, AssetConfig::Ev(cfg))
    }

    fn pv_entry(last_power_kw: f64) -> (AssetEntry, AssetConfig) {
        let cfg = PvInverter { rated_kw: 10.0, irradiance: 0.0, irradiance_offset: 0.0, pv_alpha: 0.1, export_limit_kw: None };
        let entry = AssetEntry {
            id: "pv".to_string(),
            state: AssetState::Pv(PvState { actual_power_kw: last_power_kw }),
            setpoint_kw: 0.0,
            last_power_kw,
            energy: EnergyCounter::new(),
            history: AssetHistoryBuffer::new(0),
        };
        (entry, AssetConfig::Pv(cfg))
    }

    fn base_entry(last_power_kw: f64) -> (AssetEntry, AssetConfig) {
        let cfg = BaseLoad {
            baseline_kw: last_power_kw.max(0.0),
            baseline_kw_profile: last_power_kw.max(0.0),
        };
        let entry = AssetEntry {
            id: "base_load".to_string(),
            state: AssetState::BaseLoad(BaseLoadState { actual_power_kw: last_power_kw }),
            setpoint_kw: 0.0,
            last_power_kw,
            energy: EnergyCounter::new(),
            history: AssetHistoryBuffer::new(0),
        };
        (entry, AssetConfig::BaseLoad(cfg))
    }

    fn build_assets(
        pv_kw: f64,
        base_kw: f64,
        ev_soc: f64,
        ev_plugged: bool,
        ev_target: f64,
    ) -> (Vec<AssetEntry>, Vec<AssetConfig>) {
        let (pv_e, pv_c) = pv_entry(pv_kw);
        let (base_e, base_c) = base_entry(base_kw);
        let (ev_e, ev_c) = ev_entry(ev_soc, ev_plugged, ev_target);
        (vec![pv_e, base_e, ev_e], vec![pv_c, base_c, ev_c])
    }

    // ── surplus_ev_overlay tests ──────────────────────────────────────────────

    #[test]
    fn surplus_charges_ev_when_pv_exceeds_base() {
        // PV exports 3 kW, base consumes 1 kW → surplus = 2 kW
        let (assets, configs) = build_assets(-3.0, 1.0, 0.4, true, 0.8);
        let mut sp: HashMap<String, f64> = HashMap::new();
        apply_surplus_ev_overlay(&mut sp, &assets, &configs, false);
        let ev_sp = sp.get("ev").copied().unwrap_or(0.0);
        assert!(
            (ev_sp - 2.0).abs() < 1e-6,
            "expected EV setpoint 2.0 kW, got {ev_sp}"
        );
    }

    #[test]
    fn surplus_capped_at_ev_max_charge_kw() {
        // PV exports 10 kW, base 0 kW → surplus 10 kW, but EV max is 7.4 kW
        let (assets, configs) = build_assets(-10.0, 0.0, 0.1, true, 0.8);
        let mut sp: HashMap<String, f64> = HashMap::new();
        apply_surplus_ev_overlay(&mut sp, &assets, &configs, false);
        let ev_sp = sp.get("ev").copied().unwrap_or(0.0);
        assert!(
            (ev_sp - 7.4).abs() < 1e-6,
            "EV setpoint must be capped at max_charge_kw=7.4, got {ev_sp}"
        );
    }

    #[test]
    fn surplus_not_applied_when_ev_at_target_soc() {
        // EV already at target — must not charge even with surplus
        let (assets, configs) = build_assets(-3.0, 1.0, 0.8, true, 0.8);
        let mut sp: HashMap<String, f64> = HashMap::new();
        apply_surplus_ev_overlay(&mut sp, &assets, &configs, false);
        assert!(
            sp.get("ev").is_none(),
            "must not charge EV when soc >= soc_target"
        );
    }

    #[test]
    fn surplus_not_applied_when_ev_unplugged() {
        let (assets, configs) = build_assets(-3.0, 1.0, 0.4, false, 0.8);
        let mut sp: HashMap<String, f64> = HashMap::new();
        apply_surplus_ev_overlay(&mut sp, &assets, &configs, false);
        assert!(sp.get("ev").is_none(), "must not charge unplugged EV");
    }

    #[test]
    fn surplus_not_applied_when_plan_has_ev_allocation() {
        let (assets, configs) = build_assets(-3.0, 1.0, 0.4, true, 0.8);
        let mut sp: HashMap<String, f64> = HashMap::new();
        sp.insert("ev".to_string(), 5.0); // plan allocation already present
        apply_surplus_ev_overlay(&mut sp, &assets, &configs, true);
        // Plan's 5.0 kW must be preserved, not overwritten
        let ev_sp = sp.get("ev").copied().unwrap_or(0.0);
        assert!(
            (ev_sp - 5.0).abs() < 1e-6,
            "plan allocation must not be overridden by surplus overlay"
        );
    }

    #[test]
    fn no_surplus_when_base_load_exceeds_pv() {
        // PV exports 1 kW, base consumes 2 kW → net import, no surplus
        let (assets, configs) = build_assets(-1.0, 2.0, 0.4, true, 0.8);
        let mut sp: HashMap<String, f64> = HashMap::new();
        apply_surplus_ev_overlay(&mut sp, &assets, &configs, false);
        assert!(sp.get("ev").is_none(), "no surplus when base_load > pv");
    }

    #[test]
    fn no_surplus_when_pv_not_generating() {
        // PV at 0 kW (night), base consumes 1 kW
        let (assets, configs) = build_assets(0.0, 1.0, 0.4, true, 0.8);
        let mut sp: HashMap<String, f64> = HashMap::new();
        apply_surplus_ev_overlay(&mut sp, &assets, &configs, false);
        assert!(sp.get("ev").is_none(), "no surplus when PV is not generating");
    }
}
