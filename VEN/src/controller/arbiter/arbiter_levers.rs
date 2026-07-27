//! Per-lever capacity/cost queries and `apply_*` functions for
//! `controller::arbiter` — split out to keep `arbiter.rs` under the file-size
//! cap. See that module's doc comment for the overall design.

use std::collections::HashMap;

use super::{
    DEAD_BAND_KW, HEATER_COMFORT_OVERRIDE_EUR_PER_KWH, LEVER_PREEMPTION_MARGIN_EUR_PER_KWH,
};
use crate::controller::dispatcher::predict_heater_forced_kw;
use crate::controller::SimSnapshot;
use crate::entities::plan::PlanTimeSlot;
use crate::entities::planner_params::PlannerObjective;

#[derive(Debug, Clone, Copy)]
pub(super) struct Lever {
    pub(super) id: &'static str,
    pub(super) available_capacity_kw: f64,
    pub(super) marginal_cost_eur_per_kwh: f64,
}

/// Battery lever capacity + cost. Direction-dependent: absorbing an import
/// deviation (`deviation_kw > 0`) needs headroom to discharge more (or charge
/// less); absorbing a surplus (`deviation_kw < 0`) needs headroom to charge
/// more (or discharge less).
pub(super) fn battery_lever(
    sim: &SimSnapshot,
    slot: &PlanTimeSlot,
    deviation_kw: f64,
) -> Option<Lever> {
    let snap = sim.assets.get(crate::ids::ASSET_BATTERY)?;
    let (power_headroom_kw, energy_headroom_kwh, marginal_cost_eur_per_kwh) = if deviation_kw > 0.0
    {
        (
            snap.cap_max_export_kw.abs() + snap.setpoint_kw.max(0.0),
            snap.available_discharge_kwh,
            slot.marginal_cost_import_eur_per_kwh,
        )
    } else {
        (
            snap.cap_max_import_kw + (-snap.setpoint_kw).max(0.0),
            snap.available_charge_kwh,
            slot.marginal_cost_export_eur_per_kwh,
        )
    };
    // Zero available energy (e.g. full/empty SoC) means zero capacity
    // regardless of power rating — excluded outright, not merely deprioritized.
    if energy_headroom_kwh.is_some_and(|kwh| kwh <= 0.0) {
        return None;
    }
    let available_capacity_kw = power_headroom_kw;
    if available_capacity_kw <= 0.0 {
        return None;
    }
    Some(Lever {
        id: "battery",
        available_capacity_kw,
        marginal_cost_eur_per_kwh,
    })
}

/// Dead-beat battery correction, metered against `assigned_kw` (this lever's
/// share of the deviation, from the shared `remaining_kw` pool) rather than
/// the full deviation — adapted from the former
/// `dispatcher::apply_battery_correction_overlay`, which computed and
/// canceled the *entire* deviation unconditionally. Uses the previously
/// applied setpoint (`snap.setpoint_kw`) as the integrator state, not the
/// plan allocation, to avoid a limit cycle (same rationale, see §3a's
/// stability re-verification test).
pub(super) fn apply_battery_lever(
    setpoints: &mut HashMap<String, f64>,
    sim: &SimSnapshot,
    assigned_kw: f64,
    objective: PlannerObjective,
) -> f64 {
    let Some(snap) = sim.assets.get(crate::ids::ASSET_BATTERY) else {
        return 0.0;
    };
    if objective == PlannerObjective::MaxRevenue && assigned_kw > 0.0 {
        return 0.0;
    }
    let soc = snap.val("soc").unwrap_or(0.0);
    let min_soc = snap.val("min_soc").unwrap_or(0.0);
    let max_discharge_kw = snap.cap_max_export_kw.abs();
    let max_charge_kw = snap.cap_max_import_kw;
    let current_sp = snap.setpoint_kw;

    let raw_target = current_sp - assigned_kw;
    let clamped = raw_target.clamp(-max_discharge_kw, max_charge_kw);
    let clamped = if clamped < 0.0 && soc <= min_soc + 0.01 {
        current_sp.max(0.0)
    } else if clamped > 0.0 && soc >= 1.0 - 0.01 {
        current_sp.min(0.0)
    } else {
        clamped
    };

    let delta = clamped - current_sp;
    if delta.abs() < 1e-6 {
        return 0.0;
    }
    setpoints.insert(crate::ids::ASSET_BATTERY.to_string(), clamped);
    delta.abs()
}

/// EV lever: flat zero cost, only offered when the plan has no EV allocation
/// (opportunistic regime — the plan's own EV commitment is never
/// second-guessed). Capacity is direction-dependent: absorbing a surplus can
/// increase charging up to `max_charge_kw`; absorbing an import deviation can
/// only claw back whatever opportunistic charge is already flowing (BL-12's
/// discrete relay floor makes finer-grained reduction physically meaningless).
pub(super) fn ev_lever(
    sim: &SimSnapshot,
    deviation_kw: f64,
    plan_has_ev_allocation: bool,
    overlay_enabled: bool,
) -> Option<Lever> {
    if plan_has_ev_allocation || !overlay_enabled {
        return None;
    }
    let snap = sim.assets.get(crate::ids::ASSET_EV)?;
    let plugged = snap.val("plugged").unwrap_or(0.0) > 0.5;
    if !plugged {
        return None;
    }
    let soc = snap.val("soc").unwrap_or(0.0);
    let soc_target = snap.val("soc_target").unwrap_or(1.0);
    let max_charge_kw = snap.values.get("max_charge_kw").copied().unwrap_or(0.0);
    let current_sp = snap.setpoint_kw.max(0.0);

    let available_capacity_kw = if deviation_kw < 0.0 {
        if soc >= soc_target {
            0.0
        } else {
            (max_charge_kw - current_sp).max(0.0)
        }
    } else {
        current_sp
    };
    if available_capacity_kw <= 0.0 {
        return None;
    }
    Some(Lever {
        id: "ev",
        available_capacity_kw,
        marginal_cost_eur_per_kwh: 0.0,
    })
}

pub(super) fn apply_ev_lever(
    setpoints: &mut HashMap<String, f64>,
    sim: &SimSnapshot,
    assigned_kw: f64,
) {
    let Some(snap) = sim.assets.get(crate::ids::ASSET_EV) else {
        return;
    };
    let min_charge_kw = snap.values.get("min_charge_kw").copied().unwrap_or(0.0);
    let current_sp = snap.setpoint_kw.max(0.0);
    let new_sp = (current_sp - assigned_kw).max(0.0);
    // BL-12: the charger cannot sustain below min_charge_kw — snap to 0
    // rather than commanding a sub-minimum rate that yields 0 kW physically
    // while corrupting the arbiter's own next-tick accounting.
    let new_sp = if new_sp < min_charge_kw { 0.0 } else { new_sp };
    setpoints.insert(crate::ids::ASSET_EV.to_string(), new_sp);
}

/// Heater pause-within-comfort-band lever: flat zero cost, available
/// whenever the heater's plan-allocated setpoint is > 0 this slot (§5.4
/// scenario D — "not because a static rule ranked it third but because its
/// marginal cost is genuinely zero whenever available").
pub(super) fn heater_pause_lever(
    base_setpoints: &HashMap<String, f64>,
    deviation_kw: f64,
) -> Option<Lever> {
    if deviation_kw < 0.0 {
        return None; // pausing a load can't absorb a surplus
    }
    let planned_kw = base_setpoints
        .get(crate::ids::ASSET_HEATER)
        .copied()
        .unwrap_or(0.0);
    if planned_kw <= 0.0 {
        return None;
    }
    Some(Lever {
        id: "heater_pause",
        available_capacity_kw: planned_kw,
        marginal_cost_eur_per_kwh: 0.0,
    })
}

pub(super) fn apply_heater_pause_lever(setpoints: &mut HashMap<String, f64>, assigned_kw: f64) {
    let planned_kw = setpoints
        .get(crate::ids::ASSET_HEATER)
        .copied()
        .unwrap_or(0.0);
    setpoints.insert(
        crate::ids::ASSET_HEATER.to_string(),
        (planned_kw - assigned_kw).max(0.0),
    );
}

/// Heater emergency-mode lever (`HeaterEmergencyMode::Curtail`/`Absorb`):
/// only offered when the directional marginal cost exceeds
/// `HEATER_COMFORT_OVERRIDE_EUR_PER_KWH` (§5.4 scenario H — routine tariff
/// swings must never invade the safety envelope; an obligation breach
/// penalty, baked into the slot's marginal cost, does).
///
/// `is_incumbent` (§4a.2): when the heater emergency mode was already active
/// last tick, the entry threshold is lowered by the preemption margin,
/// making the mode "stickier" to exit than to enter — a marginal cost
/// hovering right at `HEATER_COMFORT_OVERRIDE_EUR_PER_KWH` cannot flip the
/// mode on and off every tick, since leaving requires dropping below
/// `threshold − margin`, not just below `threshold`.
pub(super) fn heater_emergency_lever(
    sim: &SimSnapshot,
    slot: &PlanTimeSlot,
    deviation_kw: f64,
    is_incumbent: bool,
) -> Option<Lever> {
    let snap = sim.assets.get(crate::ids::ASSET_HEATER)?;
    let temp_c = snap.val("temp_c")?;
    let temp_min_c = snap.val("temp_min_c")?;
    let temp_max_c = snap.val("temp_max_c")?;
    let temp_safety_max_c = snap.val("temp_safety_max_c").unwrap_or(temp_max_c);
    let max_kw = snap.val("max_kw").unwrap_or(0.0);
    let threshold = if is_incumbent {
        HEATER_COMFORT_OVERRIDE_EUR_PER_KWH - LEVER_PREEMPTION_MARGIN_EUR_PER_KWH
    } else {
        HEATER_COMFORT_OVERRIDE_EUR_PER_KWH
    };

    if deviation_kw > 0.0 {
        // Import deviation: Curtail lets the tank drift toward ambient below
        // temp_min_c instead of the forced-on emergency heat — capacity is
        // however much of the currently-forced emergency draw that would free up.
        if slot.marginal_cost_import_eur_per_kwh <= threshold {
            return None;
        }
        if temp_c > temp_min_c || max_kw <= 0.0 {
            return None; // not currently in the forced-on band
        }
        Some(Lever {
            id: "heater_emergency",
            available_capacity_kw: max_kw,
            marginal_cost_eur_per_kwh: slot.marginal_cost_import_eur_per_kwh,
        })
    } else {
        // Surplus/export deviation: Absorb lets the tank heat past temp_max_c
        // up to temp_safety_max_c, soaking up otherwise-exported surplus.
        if slot.marginal_cost_export_eur_per_kwh <= threshold {
            return None;
        }
        if temp_c >= temp_safety_max_c {
            return None; // already at the true safety ceiling
        }
        Some(Lever {
            id: "heater_emergency",
            available_capacity_kw: max_kw,
            marginal_cost_eur_per_kwh: slot.marginal_cost_export_eur_per_kwh,
        })
    }
}

/// PV curtailment: backstop only, export-excess direction, priced at the
/// forgone export tariff — naturally ranks last.
pub(super) fn pv_curtailment_lever(slot: &PlanTimeSlot, deviation_kw: f64) -> Option<Lever> {
    if deviation_kw >= 0.0 || slot.pv_used_kw <= 0.0 {
        return None;
    }
    Some(Lever {
        id: "pv_curtail",
        available_capacity_kw: slot.pv_used_kw,
        marginal_cost_eur_per_kwh: slot.export_tariff_eur_kwh,
    })
}

/// No-plan-yet fallback: reproduces the former `apply_surplus_ev_overlay`'s
/// exact surplus computation (independent of any plan target, since none
/// exists during the startup window before the first plan is adopted).
pub(super) fn apply_ev_lever_opportunistic(
    setpoints: &mut HashMap<String, f64>,
    sim: &SimSnapshot,
    live_pv_kw: Option<f64>,
    live_base_load_kw: Option<f64>,
    plan_has_ev_allocation: bool,
    overlay_enabled: bool,
) {
    if plan_has_ev_allocation || !overlay_enabled {
        return;
    }
    let net_other_kw: f64 = sim
        .assets
        .iter()
        .filter(|(id, _)| {
            id.as_str() != crate::ids::ASSET_EV && id.as_str() != crate::ids::ASSET_BATTERY
        })
        .map(|(id, snap)| {
            if id.as_str() == crate::ids::ASSET_PV {
                if let Some(pv_kw) = live_pv_kw {
                    return pv_kw;
                }
            }
            if id.as_str() == crate::ids::ASSET_BASE_LOAD {
                if let Some(bl_kw) = live_base_load_kw {
                    return bl_kw;
                }
            }
            if id.as_str() == crate::ids::ASSET_HEATER {
                if let Some(forced_kw) = predict_heater_forced_kw(snap) {
                    return forced_kw;
                }
            }
            let sp = setpoints.get(id).copied().unwrap_or(snap.power_kw);
            if sp.abs() > 1e20 {
                snap.power_kw
            } else {
                sp
            }
        })
        .sum();
    let battery_charge_kw = setpoints
        .get(crate::ids::ASSET_BATTERY)
        .copied()
        .unwrap_or(0.0)
        .max(0.0);
    let surplus_kw = (-net_other_kw - battery_charge_kw).max(0.0);
    if surplus_kw < DEAD_BAND_KW {
        return;
    }
    let Some(snap) = sim.assets.get(crate::ids::ASSET_EV) else {
        return;
    };
    let plugged = snap.val("plugged").unwrap_or(0.0) > 0.5;
    let soc = snap.val("soc").unwrap_or(0.0);
    let soc_target = snap.val("soc_target").unwrap_or(1.0);
    if plugged && soc < soc_target {
        let max_charge_kw = snap.values.get("max_charge_kw").copied().unwrap_or(0.0);
        let min_charge_kw = snap.values.get("min_charge_kw").copied().unwrap_or(0.0);
        let charge_kw = surplus_kw.min(max_charge_kw);
        if charge_kw >= min_charge_kw {
            setpoints.insert(crate::ids::ASSET_EV.to_string(), charge_kw);
        }
    }
}
