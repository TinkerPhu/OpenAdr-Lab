/// Dispatcher: translates FIRM plan slot allocations into per-asset setpoints.
///
/// Single responsibility: given the current plan, simulator assets, and capacity
/// constraints, produce a HashMap<asset_id, kW> that drives the simulator tick.
/// The plan is the sole authority.
use crate::assets::{AssetConfig, AssetState};
use crate::entities::capacity::OadrCapacityState;
use crate::entities::plan::Plan;
use crate::profile::PlannerObjective;
use crate::simulator::AssetEntry;
use chrono::{DateTime, Utc};
use std::collections::HashMap;

/// Build a setpoints map for all known assets based on the active plan.
///
/// Algorithm:
/// 1. Start with each asset's `default_setpoint()` from its current state.
/// 2. Find the slot covering `now` in the plan.
/// 3. Overwrite entries for assets that have an allocation in that slot.
/// 4. If `heater_setpoint_c` override is set and the plan has no heater allocation,
///    compute ON/OFF setpoint based on current temperature vs. target.
/// 5. Enforce `export_limit_kw` on the `pv` key if capacity state has one.
/// 6. Apply opportunistic surplus EV charging (see `apply_surplus_ev_overlay`).
pub fn build_setpoints(
    plan: &Plan,
    assets: &[AssetEntry],
    asset_configs: &[AssetConfig],
    capacity: &OadrCapacityState,
    heater_setpoint_c: Option<f64>,
    now: DateTime<Utc>,
    overlay_enabled: bool,
) -> HashMap<String, f64> {
    // Start with defaults from current asset state
    let mut setpoints: HashMap<String, f64> = assets
        .iter()
        .zip(asset_configs.iter())
        .map(|(a, cfg)| (a.id.clone(), cfg.default_setpoint(&a.state)))
        .collect();

    // Find the slot covering now
    let slot_allocs: Option<&Vec<crate::entities::plan::AssetAllocation>> = plan
        .slots
        .iter()
        .find(|s| s.start <= now && now < s.end)
        .map(|s| &s.allocations);

    let mut plan_allocated_heater = false;
    let mut plan_allocated_ev = false;
    if let Some(allocs) = slot_allocs {
        for alloc in allocs {
            // Battery allocations have no associated packet
            if alloc.asset_id == crate::ids::ASSET_BATTERY {
                setpoints.insert(crate::ids::ASSET_BATTERY.to_string(), alloc.power_kw);
                continue;
            }
            if alloc.asset_id == crate::ids::ASSET_HEATER {
                plan_allocated_heater = true;
            }
            if alloc.asset_id == crate::ids::ASSET_EV {
                plan_allocated_ev = true;
            }
            setpoints.insert(alloc.asset_id.clone(), alloc.power_kw);
        }
    }

    // Heater setpoint override: compute ON/OFF based on current temp vs. target.
    // Only applies when the plan has no heater allocation for the current slot.
    if let Some(target_c) = heater_setpoint_c {
        if !plan_allocated_heater {
            if let Some((entry, cfg)) = assets.iter()
                .zip(asset_configs.iter())
                .find(|(a, _)| a.id == crate::ids::ASSET_HEATER)
            {
                if let Some(power_kw) = cfg.thermostat_setpoint_kw(&entry.state, target_c) {
                    setpoints.insert(crate::ids::ASSET_HEATER.to_string(), power_kw);
                }
            }
        }
    }

    // Enforce export capacity limit on PV
    if let Some(export_cap) = capacity.export_limit_kw {
        let pv_sp = setpoints.entry(crate::ids::ASSET_PV.to_string()).or_insert(0.0);
        // PV export is negative in sign convention; cap absolute value
        if *pv_sp < -export_cap {
            *pv_sp = -export_cap;
        }
    }

    // Opportunistic surplus EV charging: redirect live PV export to EV when no
    // plan-level EV allocation is active.
    apply_surplus_ev_overlay(&mut setpoints, assets, asset_configs, plan_allocated_ev, overlay_enabled);

    setpoints
}

/// Opportunistic surplus EV charging overlay.
///
/// When PV is exporting more power than base load consumes, offer the surplus
/// to the EV (up to its max charge rate). No EnergyPacket is created — this is
/// dispatcher-only and does not appear in the plan or VTN reports.
///
/// Does nothing when:
/// - `overlay_enabled` is false (user disabled or auto-paused by active EvSession)
/// - `plan_has_ev_allocation` is true (plan-level commitment takes priority)
/// - EV is unplugged
/// - EV SoC has reached its target
/// - Surplus is below the 100 W noise floor
pub fn apply_surplus_ev_overlay(
    setpoints: &mut HashMap<String, f64>,
    assets: &[AssetEntry],
    asset_configs: &[AssetConfig],
    plan_has_ev_allocation: bool,
    overlay_enabled: bool,
) {
    if plan_has_ev_allocation || !overlay_enabled {
        return;
    }
    // Live PV power (negative = export) and base load (positive = import).
    let pv_kw = assets.iter().find(|a| a.id == crate::ids::ASSET_PV).map(|a| a.last_power_kw).unwrap_or(0.0);
    let base_kw = assets
        .iter()
        .find(|a| a.id == crate::ids::ASSET_BASE_LOAD)
        .map(|a| a.last_power_kw)
        .unwrap_or(0.0);
    // Also account for any battery charging that the plan has already allocated this
    // tick (positive setpoint = charging). This prevents double-allocating PV surplus
    // to both the battery and the EV — EV only gets what the battery leaves behind.
    let battery_charge_kw = setpoints.get(crate::ids::ASSET_BATTERY).copied().unwrap_or(0.0).max(0.0);
    // surplus_kw: net generation after base consumption and planned battery charging
    let surplus_kw = (-(pv_kw + base_kw) - battery_charge_kw).max(0.0);
    if surplus_kw < 0.1 {
        return;
    }
    for (entry, cfg) in assets.iter().zip(asset_configs.iter()) {
        if let Some(charge_kw) = cfg.surplus_charge_kw(&entry.state, surplus_kw) {
            setpoints.insert(entry.id.clone(), charge_kw);
            break;
        }
    }
}

/// Layer 1 reactive correction: adjust battery setpoint when actual grid
/// power deviates from the plan's expectation by more than `threshold_kw`.
///
/// Uses the previously applied setpoint (`assets[idx].setpoint_kw`, stored
/// by `SimState::tick` after each `cfg.step()`) as the integrator state,
/// NOT the plan allocation. This gives a dead-beat (P-gain = 1.0) controller:
///
///   new_sp = prev_applied_sp − deviation_kw
///
/// which eliminates tracking error in one tick for a stationary disturbance.
/// Using the plan allocation instead would reset the integrator every tick
/// and cause a ±max-discharge limit cycle.
///
/// Returns a non-zero delta when a correction is applied. When the deviation
/// falls back within threshold, returns 0.0. The caller (`spawn_sim_tick` in
/// loops.rs) is responsible for "holding" the previous corrected setpoint in
/// that case to prevent the plan allocation from reverting the battery and
/// restarting the limit cycle (see `prev_correction_kw` in loops.rs).
///
/// For sustained deviations, Layer 2 fires `DeviceDeviation` after
/// `deviation_trigger_ticks` consecutive ticks, triggering a full MILP replan.
///
/// Sign: positive setpoint = charging (import), negative = discharging (export).
/// Returns the delta applied (0.0 if below threshold, SoC-limited, or < min_correction_kw).
pub fn apply_battery_correction_overlay(
    setpoints: &mut HashMap<String, f64>,
    assets: &[AssetEntry],
    asset_configs: &[AssetConfig],
    plan_signed_net_kw: f64,
    actual_net_kw: f64,
    objective: PlannerObjective,
    threshold_kw: f64,
    min_correction_kw: f64,
) -> f64 {
    let deviation_kw = actual_net_kw - plan_signed_net_kw;

    // Find battery asset and config (needed before threshold check to read setpoint_kw).
    let Some(idx) = assets.iter().position(|a| a.id == crate::ids::ASSET_BATTERY) else {
        return 0.0;
    };
    let (AssetState::Battery(bs), AssetConfig::Battery(bcfg)) =
        (&assets[idx].state, &asset_configs[idx])
    else {
        return 0.0;
    };

    let current_sp = assets[idx].setpoint_kw;

    if deviation_kw.abs() <= threshold_kw {
        return 0.0;
    }

    // Objective gate: MaxRevenue suppresses discharge corrections (preserve for export)
    if objective == PlannerObjective::MaxRevenue && deviation_kw > 0.0 {
        return 0.0;
    }

    // Dead-beat: new_sp = prev_applied_sp − deviation eliminates error in one tick.
    let raw_target = current_sp - deviation_kw;

    // Clamp to power limits
    let clamped = raw_target.clamp(-bcfg.max_discharge_kw, bcfg.max_charge_kw);

    // SoC feasibility: don't discharge below min_soc, don't charge above 1.0
    let clamped = if clamped < 0.0 && bs.soc <= bcfg.min_soc + 0.01 {
        current_sp.max(0.0) // already at floor, suppress discharge
    } else if clamped > 0.0 && bs.soc >= 1.0 - 0.01 {
        current_sp.min(0.0) // already at ceiling, suppress charge
    } else {
        clamped
    };

    let delta = clamped - current_sp;
    if delta.abs() < min_correction_kw {
        return 0.0;
    }

    setpoints.insert(crate::ids::ASSET_BATTERY.to_string(), clamped);
    delta
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assets::{
        AssetConfig, AssetState, BaseLoad, BaseLoadState, Battery, BatteryState,
        EvCharger, EvState, PvInverter, PvState,
    };
    use crate::simulator::{energy::EnergyCounter, AssetEntry};
    use crate::assets::AssetHistoryBuffer;

    fn battery_entry(soc: f64) -> (AssetEntry, AssetConfig) {
        let cfg = Battery {
            capacity_kwh: 10.0,
            max_charge_kw: 5.0,
            max_discharge_kw: 5.0,
            round_trip_efficiency: 0.95,
            min_soc: 0.1,
        };
        let entry = AssetEntry {
            id: "battery".to_string(),
            state: AssetState::Battery(BatteryState { soc, actual_power_kw: 0.0 }),
            setpoint_kw: 0.0,
            last_power_kw: 0.0,
            energy: EnergyCounter::new(),
            history: AssetHistoryBuffer::new(0),
        };
        (entry, AssetConfig::Battery(cfg))
    }

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
        apply_surplus_ev_overlay(&mut sp, &assets, &configs, false, true);
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
        apply_surplus_ev_overlay(&mut sp, &assets, &configs, false, true);
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
        apply_surplus_ev_overlay(&mut sp, &assets, &configs, false, true);
        assert!(
            sp.get("ev").is_none(),
            "must not charge EV when soc >= soc_target"
        );
    }

    #[test]
    fn surplus_not_applied_when_ev_unplugged() {
        let (assets, configs) = build_assets(-3.0, 1.0, 0.4, false, 0.8);
        let mut sp: HashMap<String, f64> = HashMap::new();
        apply_surplus_ev_overlay(&mut sp, &assets, &configs, false, true);
        assert!(sp.get("ev").is_none(), "must not charge unplugged EV");
    }

    #[test]
    fn surplus_not_applied_when_plan_has_ev_allocation() {
        let (assets, configs) = build_assets(-3.0, 1.0, 0.4, true, 0.8);
        let mut sp: HashMap<String, f64> = HashMap::new();
        sp.insert("ev".to_string(), 5.0); // plan allocation already present
        apply_surplus_ev_overlay(&mut sp, &assets, &configs, true, true);
        // Plan's 5.0 kW must be preserved, not overwritten
        let ev_sp = sp.get("ev").copied().unwrap_or(0.0);
        assert!(
            (ev_sp - 5.0).abs() < 1e-6,
            "plan allocation must not be overridden by surplus overlay"
        );
    }

    #[test]
    fn battery_charging_reduces_ev_surplus() {
        // PV 4 kW, base 0.5 kW → raw PV surplus 3.5 kW.
        // Battery plan setpoint = 3.0 kW → EV should only get 0.5 kW.
        let (assets, configs) = build_assets(-4.0, 0.5, 0.4, true, 0.8);
        let mut sp: HashMap<String, f64> = HashMap::new();
        sp.insert("battery".to_string(), 3.0); // battery plan allocation
        apply_surplus_ev_overlay(&mut sp, &assets, &configs, false, true);
        let ev_sp = sp.get("ev").copied().unwrap_or(0.0);
        assert!(
            (ev_sp - 0.5).abs() < 1e-6,
            "EV should receive only the remaining surplus after battery, got {ev_sp}"
        );
    }

    #[test]
    fn battery_claiming_full_surplus_leaves_ev_idle() {
        // PV 4 kW, base 0.5 kW → surplus 3.5 kW; battery claims all 3.5 kW.
        let (assets, configs) = build_assets(-4.0, 0.5, 0.4, true, 0.8);
        let mut sp: HashMap<String, f64> = HashMap::new();
        sp.insert("battery".to_string(), 3.5);
        apply_surplus_ev_overlay(&mut sp, &assets, &configs, false, true);
        assert!(sp.get("ev").is_none(), "EV must not charge when battery claims full surplus");
    }

    #[test]
    fn no_surplus_when_base_load_exceeds_pv() {
        // PV exports 1 kW, base consumes 2 kW → net import, no surplus
        let (assets, configs) = build_assets(-1.0, 2.0, 0.4, true, 0.8);
        let mut sp: HashMap<String, f64> = HashMap::new();
        apply_surplus_ev_overlay(&mut sp, &assets, &configs, false, true);
        assert!(sp.get("ev").is_none(), "no surplus when base_load > pv");
    }

    #[test]
    fn no_surplus_when_pv_not_generating() {
        // PV at 0 kW (night), base consumes 1 kW
        let (assets, configs) = build_assets(0.0, 1.0, 0.4, true, 0.8);
        let mut sp: HashMap<String, f64> = HashMap::new();
        apply_surplus_ev_overlay(&mut sp, &assets, &configs, false, true);
        assert!(sp.get("ev").is_none(), "no surplus when PV is not generating");
    }

    #[test]
    fn overlay_disabled_suppresses_ev_even_with_surplus() {
        // PV exports 3 kW, base 1 kW → surplus 2 kW; EV plugged and below target.
        // overlay_enabled=false means nothing is written regardless.
        let (assets, configs) = build_assets(-3.0, 1.0, 0.4, true, 0.8);
        let mut sp: HashMap<String, f64> = HashMap::new();
        apply_surplus_ev_overlay(&mut sp, &assets, &configs, false, false);
        assert!(
            sp.get("ev").is_none(),
            "overlay must not fire when overlay_enabled=false"
        );
    }

    // ── battery correction overlay tests ──────────────────────────────────────

    #[test]
    fn correction_discharges_battery_on_pv_shortfall() {
        // actual_net=3.0, planned_net=0.0, threshold=1.0 → deviation=3.0
        // Battery should discharge (negative delta) to compensate
        let (bat_e, bat_c) = battery_entry(0.5);
        let assets = vec![bat_e];
        let configs = vec![bat_c];
        let mut sp: HashMap<String, f64> = HashMap::new();
        sp.insert("battery".to_string(), 0.0);
        let delta = apply_battery_correction_overlay(
            &mut sp, &assets, &configs,
            0.0, 3.0, PlannerObjective::MinCost, 1.0, 0.2,
        );
        assert!(delta < 0.0, "expected negative delta (discharge), got {delta}");
        let bat_sp = sp.get("battery").copied().unwrap();
        assert!(bat_sp < 0.0, "battery setpoint should be negative (discharge), got {bat_sp}");
    }

    #[test]
    fn correction_suppressed_below_threshold() {
        let (bat_e, bat_c) = battery_entry(0.5);
        let assets = vec![bat_e];
        let configs = vec![bat_c];
        let mut sp: HashMap<String, f64> = HashMap::new();
        sp.insert("battery".to_string(), 0.0);
        let delta = apply_battery_correction_overlay(
            &mut sp, &assets, &configs,
            0.0, 0.5, PlannerObjective::MinCost, 1.0, 0.2,
        );
        assert_eq!(delta, 0.0, "deviation 0.5 below threshold 1.0 must return 0");
    }

    #[test]
    fn correction_suppressed_when_battery_at_min_soc() {
        // soc at min_soc + 0.005 → discharge correction should be suppressed
        let (bat_e, bat_c) = battery_entry(0.105);
        let assets = vec![bat_e];
        let configs = vec![bat_c];
        let mut sp: HashMap<String, f64> = HashMap::new();
        sp.insert("battery".to_string(), 0.0);
        let delta = apply_battery_correction_overlay(
            &mut sp, &assets, &configs,
            0.0, 3.0, PlannerObjective::MinCost, 1.0, 0.2,
        );
        assert_eq!(delta, 0.0, "discharge must be suppressed near min_soc");
    }

    #[test]
    fn correction_suppressed_for_maxrevenue_discharge() {
        // MaxRevenue + positive deviation (importing more) → suppress discharge
        let (bat_e, bat_c) = battery_entry(0.5);
        let assets = vec![bat_e];
        let configs = vec![bat_c];
        let mut sp: HashMap<String, f64> = HashMap::new();
        sp.insert("battery".to_string(), 0.0);
        let delta = apply_battery_correction_overlay(
            &mut sp, &assets, &configs,
            0.0, 3.0, PlannerObjective::MaxRevenue, 1.0, 0.2,
        );
        assert_eq!(delta, 0.0, "MaxRevenue must suppress discharge corrections");
    }

    #[test]
    fn correction_allows_maxrevenue_on_export_excess() {
        // MaxRevenue + negative deviation (exporting more than planned) → charge more
        let (bat_e, bat_c) = battery_entry(0.5);
        let assets = vec![bat_e];
        let configs = vec![bat_c];
        let mut sp: HashMap<String, f64> = HashMap::new();
        sp.insert("battery".to_string(), 0.0);
        let delta = apply_battery_correction_overlay(
            &mut sp, &assets, &configs,
            0.0, -3.0, PlannerObjective::MaxRevenue, 1.0, 0.2,
        );
        assert!(delta > 0.0, "MaxRevenue should allow charge corrections on export excess, got {delta}");
    }

    #[test]
    fn correction_clamped_to_max_discharge_kw() {
        // Large deviation → setpoint must not exceed -max_discharge_kw (5.0)
        let (bat_e, bat_c) = battery_entry(0.5);
        let assets = vec![bat_e];
        let configs = vec![bat_c];
        let mut sp: HashMap<String, f64> = HashMap::new();
        sp.insert("battery".to_string(), 0.0);
        let _delta = apply_battery_correction_overlay(
            &mut sp, &assets, &configs,
            0.0, 20.0, PlannerObjective::MinCost, 1.0, 0.2,
        );
        let bat_sp = sp.get("battery").copied().unwrap();
        assert!(bat_sp >= -5.0, "battery setpoint must not go below -max_discharge_kw, got {bat_sp}");
    }

    fn battery_entry_with_setpoint(soc: f64, setpoint_kw: f64) -> (AssetEntry, AssetConfig) {
        let (mut entry, cfg) = battery_entry(soc);
        entry.setpoint_kw = setpoint_kw;
        (entry, cfg)
    }

    #[test]
    fn correction_converges_not_oscillates_using_prev_setpoint() {
        // Regression: previous tick applied +4.17 kW correction (battery charging to absorb PV).
        // sp_map["battery"] = -0.5 (plan allocation).
        // Actual grid = -4.5 kW (still exporting; PV exceeds battery charge capacity).
        // deviation = -4.5 - 0.0 = -4.5.
        //
        // Dead-beat using setpoint_kw (fix):  raw = 4.17 - (-4.5) = 8.67 → clamped 5.0 kW charge.
        // Dead-beat using sp_map value (bug):  raw = -0.5 - (-4.5) = +4.0 kW charge.
        //
        // Both charge, but the fix pushes harder (5.0 vs 4.0), absorbing more export.
        // Key check: correction does NOT oscillate to discharge (bat_sp must be > prev setpoint).
        let (bat_e, bat_c) = battery_entry_with_setpoint(0.5, 4.17);
        let assets = vec![bat_e];
        let configs = vec![bat_c];
        let mut sp: HashMap<String, f64> = HashMap::new();
        sp.insert("battery".to_string(), -0.5); // plan allocation
        let _delta = apply_battery_correction_overlay(
            &mut sp, &assets, &configs,
            0.0, -4.5, PlannerObjective::MinCost, 1.0, 0.2,
        );
        let bat_sp = sp.get("battery").copied().unwrap();
        assert!(
            bat_sp > 4.17,
            "correction must increase charging above prev setpoint (4.17), not oscillate to discharge; got {bat_sp}"
        );
    }

    #[test]
    fn correction_at_startup_uses_zero_setpoint_kw() {
        // On the first tick, setpoint_kw = 0.0 (SimState not yet ticked).
        // sp_map["battery"] = -0.5 (plan allocation).
        // actual_net = 3.0 kW (import excess), deviation = 3.0.
        // raw = 0.0 - 3.0 = -3.0 kW → discharges to compensate. Direction correct.
        let (bat_e, bat_c) = battery_entry_with_setpoint(0.5, 0.0);
        let assets = vec![bat_e];
        let configs = vec![bat_c];
        let mut sp: HashMap<String, f64> = HashMap::new();
        sp.insert("battery".to_string(), -0.5);
        let delta = apply_battery_correction_overlay(
            &mut sp, &assets, &configs,
            0.0, 3.0, PlannerObjective::MinCost, 1.0, 0.2,
        );
        let bat_sp = sp.get("battery").copied().unwrap();
        assert!(delta < 0.0 && bat_sp < 0.0, "discharge direction correct at startup, got delta={delta} sp={bat_sp}");
        assert!(bat_sp >= -5.0, "clamped to max_discharge_kw, got {bat_sp}");
    }

    #[test]
    fn correction_returns_zero_without_modifying_sp_when_within_threshold() {
        // When deviation falls within threshold after a correction was active,
        // apply_battery_correction_overlay returns 0.0 and does NOT modify sp_map.
        // The "hold" (preventing plan reversion) is the caller's responsibility
        // via prev_correction_kw tracking in loops.rs (spawn_sim_tick).
        let (bat_e, bat_c) = battery_entry_with_setpoint(0.5, -4.98); // previous correction
        let assets = vec![bat_e];
        let configs = vec![bat_c];
        let mut sp: HashMap<String, f64> = HashMap::new();
        sp.insert("battery".to_string(), -0.5); // plan allocation
        let delta = apply_battery_correction_overlay(
            &mut sp, &assets, &configs,
            0.0, 0.02, PlannerObjective::MinCost, 1.0, 0.2,
        );
        let bat_sp = sp.get("battery").copied().unwrap();
        assert_eq!(delta, 0.0, "no delta when deviation is within threshold");
        assert!(
            (bat_sp - (-0.5)).abs() < 1e-9,
            "sp_map must not be modified when within threshold; hold is handled by loops.rs, got {bat_sp}"
        );
    }

    #[test]
    fn correction_suppressed_when_plan_expects_export_and_actual_matches() {
        // Regression test for Issue A: when the plan expects export (net_import=0, net_export=3.1),
        // plan_signed_net_kw = 0 - 3.1 = -3.1. If actual_net_kw is also -3.1 (matching the plan),
        // deviation = actual - plan = -3.1 - (-3.1) = 0.0 → no correction must fire.
        // Before the fix, plan_signed_net_kw was wrongly taken as net_import_kw (= 0.0),
        // giving deviation = -3.1 which triggered a charge correction that cancelled the export.
        let (bat_e, bat_c) = battery_entry(0.5);
        let assets = vec![bat_e];
        let configs = vec![bat_c];
        let mut sp: HashMap<String, f64> = HashMap::new();
        sp.insert("battery".to_string(), 0.0);
        let delta = apply_battery_correction_overlay(
            &mut sp, &assets, &configs,
            -3.1, -3.1, PlannerObjective::MinCost, 1.0, 0.2,
        );
        assert_eq!(delta, 0.0,
            "no correction when actual matches planned export (deviation = 0); got {delta}");
    }

    #[test]
    fn correction_converges_after_deviation_clears_using_dead_beat() {
        // Verify that the dead-beat formula uses setpoint_kw (not sp_map plan allocation).
        // setpoint_kw=-4.98 (held by loops.rs from previous tick), deviation = +4.48.
        // raw = -4.98 - 4.48 = -9.46 → clamped to -5.0 (max_discharge_kw).
        // Resulting delta = -5.0 - (-4.98) = -0.02, which is below min_correction_kw=0.2.
        // The correction is correctly suppressed: battery is already at effective maximum
        // discharge; no further meaningful correction is possible.
        let (bat_e, bat_c) = battery_entry_with_setpoint(0.5, -4.98);
        let assets = vec![bat_e];
        let configs = vec![bat_c];
        let mut sp: HashMap<String, f64> = HashMap::new();
        sp.insert("battery".to_string(), -0.5); // plan allocation
        let delta = apply_battery_correction_overlay(
            &mut sp, &assets, &configs,
            0.0, 4.48, PlannerObjective::MinCost, 1.0, 0.2,
        );
        let bat_sp = sp.get("battery").copied().unwrap();
        assert_eq!(delta, 0.0, "delta suppressed: battery already at max discharge, residual -0.02 < min_correction_kw; got {delta}");
        assert!(
            (bat_sp - (-0.5)).abs() < 1e-9,
            "sp_map unchanged when correction below min threshold; got {bat_sp}"
        );
    }
}
