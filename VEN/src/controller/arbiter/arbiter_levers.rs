//! Per-lever capacity/cost queries and `apply_*` functions for
//! `controller::arbiter` — split out to keep `arbiter.rs` under the file-size
//! cap. See that module's doc comment for the overall design.

use std::collections::HashMap;

use super::{
    DEAD_BAND_KW, HEATER_COMFORT_OVERRIDE_EUR_PER_KWH, LEVER_PREEMPTION_MARGIN_EUR_PER_KWH,
};
use crate::controller::SimSnapshot;
use crate::entities::asset::{highest_power_step_at_or_below_kw, nearest_power_step_kw};
use crate::entities::plan::PlanTimeSlot;
use crate::entities::planner_params::PlannerObjective;

#[derive(Debug, Clone, Copy)]
pub(super) struct Lever {
    pub(super) id: &'static str,
    pub(super) available_capacity_kw: f64,
    pub(super) marginal_cost_eur_per_kwh: f64,
}

/// The setpoint this tick's adjustments start from: the value already in the
/// setpoint map, else the asset's last applied command. `reconcile` seeds the
/// map with the last applied command for battery/EV (their dead-beat
/// integrator state), so both passes read the same value through here.
pub(super) fn current_setpoint_kw(
    setpoints: &HashMap<String, f64>,
    sim: &SimSnapshot,
    asset_id: &str,
) -> f64 {
    setpoints
        .get(asset_id)
        .copied()
        .or_else(|| sim.assets.get(asset_id).map(|s| s.setpoint_kw))
        .unwrap_or(0.0)
}

/// The battery's setpoint range: its own capability (it already reports 0 in
/// a direction it can't sustain near full/empty — no SoC re-interpretation
/// here), narrowed by a pass's `bounds_kw` (the comms-loss clamp).
fn battery_setpoint_range_kw(
    snap: &crate::controller::simulator_port::AssetSnapshot,
    bounds_kw: Option<(f64, f64)>,
) -> (f64, f64) {
    let (min_kw, max_kw) = (-snap.cap_max_export_kw.abs(), snap.cap_max_import_kw);
    match bounds_kw {
        Some((lo_kw, hi_kw)) => (min_kw.max(lo_kw).min(0.0), max_kw.min(hi_kw).max(0.0)),
        None => (min_kw, max_kw),
    }
}

/// Battery lever capacity + cost. Direction-dependent: absorbing an import
/// deviation (`deviation_kw > 0`) needs headroom to discharge more (or charge
/// less); absorbing a surplus (`deviation_kw < 0`) needs headroom to charge
/// more (or discharge less). Without a plan slot (startup window) the cost is
/// 0 — ranking then falls back to candidate order.
pub(super) fn battery_lever(
    setpoints: &HashMap<String, f64>,
    sim: &SimSnapshot,
    slot: Option<&PlanTimeSlot>,
    deviation_kw: f64,
    bounds_kw: Option<(f64, f64)>,
) -> Option<Lever> {
    let snap = sim.assets.get(crate::ids::ASSET_BATTERY)?;
    let current_sp = current_setpoint_kw(setpoints, sim, crate::ids::ASSET_BATTERY);
    let (min_kw, max_kw) = battery_setpoint_range_kw(snap, bounds_kw);
    let (power_headroom_kw, energy_headroom_kwh, marginal_cost_eur_per_kwh) = if deviation_kw > 0.0
    {
        (
            current_sp - min_kw,
            snap.available_discharge_kwh,
            slot.map_or(0.0, |s| s.marginal_cost_import_eur_per_kwh),
        )
    } else {
        (
            max_kw - current_sp,
            snap.available_charge_kwh,
            slot.map_or(0.0, |s| s.marginal_cost_export_eur_per_kwh),
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
/// canceled the *entire* deviation unconditionally. Starts from
/// `current_setpoint_kw`: for the deviation pass that is the previously
/// applied setpoint (`reconcile` seeds it), not the plan allocation, to avoid
/// a limit cycle (see §3a's stability re-verification tests). Returns the
/// achieved change (kW, magnitude).
pub(super) fn apply_battery_lever(
    setpoints: &mut HashMap<String, f64>,
    sim: &SimSnapshot,
    assigned_kw: f64,
    objective: PlannerObjective,
    bounds_kw: Option<(f64, f64)>,
) -> f64 {
    let Some(snap) = sim.assets.get(crate::ids::ASSET_BATTERY) else {
        return 0.0;
    };
    if objective == PlannerObjective::MaxRevenue && assigned_kw > 0.0 {
        return 0.0;
    }
    let (min_kw, max_kw) = battery_setpoint_range_kw(snap, bounds_kw);
    let current_sp = current_setpoint_kw(setpoints, sim, crate::ids::ASSET_BATTERY);

    let raw_target = current_sp - assigned_kw;
    let clamped = raw_target.clamp(min_kw, max_kw);

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
    setpoints: &HashMap<String, f64>,
    sim: &SimSnapshot,
    deviation_kw: f64,
    plan_has_ev_allocation: bool,
    overlay_enabled: bool,
) -> Option<Lever> {
    if plan_has_ev_allocation || !overlay_enabled {
        return None;
    }
    let snap = sim.assets.get(crate::ids::ASSET_EV)?;
    // The EV's own capability is 0 while unplugged or at/above its target.
    let charge_ceiling_kw = snap.cap_max_import_kw;
    if charge_ceiling_kw <= 0.0 {
        return None;
    }
    let current_sp = current_setpoint_kw(setpoints, sim, crate::ids::ASSET_EV).max(0.0);

    let available_capacity_kw = if deviation_kw < 0.0 {
        (charge_ceiling_kw - current_sp).max(0.0)
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

/// Returns the achieved change (kW, magnitude) — can exceed `assigned_kw`
/// when BL-12's `min_charge_kw` floor snaps a reduction to 0.
pub(super) fn apply_ev_lever(
    setpoints: &mut HashMap<String, f64>,
    sim: &SimSnapshot,
    assigned_kw: f64,
) -> f64 {
    let Some(snap) = sim.assets.get(crate::ids::ASSET_EV) else {
        return 0.0;
    };
    let min_charge_kw = snap.values.get("min_charge_kw").copied().unwrap_or(0.0);
    let current_sp = current_setpoint_kw(setpoints, sim, crate::ids::ASSET_EV).max(0.0);
    let new_sp = (current_sp - assigned_kw).max(0.0);
    // BL-12: the charger cannot sustain below min_charge_kw — snap to 0
    // rather than commanding a sub-minimum rate that yields 0 kW physically
    // while corrupting the arbiter's own next-tick accounting.
    let new_sp = if new_sp < min_charge_kw { 0.0 } else { new_sp };
    setpoints.insert(crate::ids::ASSET_EV.to_string(), new_sp);
    (new_sp - current_sp).abs()
}

/// Heater pause-within-comfort-band lever: flat zero cost, available
/// whenever the heater draws a stage this tick (§5.4 scenario D — "not because
/// a static rule ranked it third but because its marginal cost is genuinely
/// zero whenever available"). Nothing to pause while the thermostat forces the
/// heater's power (`forced_power_kw`) — the setpoint is ignored then.
pub(super) fn heater_pause_lever(
    setpoints: &HashMap<String, f64>,
    sim: &SimSnapshot,
    deviation_kw: f64,
) -> Option<Lever> {
    if deviation_kw < 0.0 {
        return None; // pausing a load can't absorb a surplus
    }
    let snap = sim.assets.get(crate::ids::ASSET_HEATER)?;
    if snap.forced_power_kw.is_some() {
        return None;
    }
    let drawn_kw = heater_drawn_kw(setpoints, sim);
    if drawn_kw <= 0.0 {
        return None;
    }
    Some(Lever {
        id: "heater_pause",
        available_capacity_kw: drawn_kw,
        marginal_cost_eur_per_kwh: 0.0,
    })
}

/// Commands the highest stage at or below `drawn − assigned` — an in-between
/// setpoint would round back up to the stage it came from and shed nothing.
/// Returns the achieved change (kW, magnitude): a whole stage, which may
/// exceed `assigned_kw`.
pub(super) fn apply_heater_pause_lever(
    setpoints: &mut HashMap<String, f64>,
    sim: &SimSnapshot,
    assigned_kw: f64,
) -> f64 {
    let drawn_kw = heater_drawn_kw(setpoints, sim);
    let new_kw = highest_power_step_at_or_below_kw(
        heater_power_steps_kw(sim),
        (drawn_kw - assigned_kw).max(0.0),
    );
    setpoints.insert(crate::ids::ASSET_HEATER.to_string(), new_kw);
    drawn_kw - new_kw
}

/// The stage the heater draws for its current setpoint (through the same
/// quantization its own step physics uses).
fn heater_drawn_kw(setpoints: &HashMap<String, f64>, sim: &SimSnapshot) -> f64 {
    let setpoint_kw = setpoints
        .get(crate::ids::ASSET_HEATER)
        .copied()
        .unwrap_or(0.0);
    nearest_power_step_kw(heater_power_steps_kw(sim), setpoint_kw)
}

fn heater_power_steps_kw(sim: &SimSnapshot) -> &[f64] {
    sim.assets
        .get(crate::ids::ASSET_HEATER)
        .map_or(&[], |snap| snap.power_steps_kw.as_slice())
}

/// Heater emergency-mode lever (`HeaterEmergencyMode::Curtail`/`Absorb`).
///
/// Curtail (import direction) is offered only during an alert window: the
/// heater's own thermostat is its safety, and nothing short of a grid alert
/// overrides it — not a capacity limit, whose violation penalty inflates the
/// slot's marginal cost just like an alert's would (GB-47). Priced at
/// `HEATER_COMFORT_OVERRIDE_EUR_PER_KWH`, so cheaper levers go first.
///
/// Absorb (surplus direction) is offered when the export marginal cost exceeds
/// `HEATER_COMFORT_OVERRIDE_EUR_PER_KWH` (§5.4 scenario H — routine tariff
/// swings must never invade the safety envelope). `is_incumbent` (§4a.2):
/// when the mode was already active last tick, that entry threshold is
/// lowered by the preemption margin, making the mode "stickier" to exit than
/// to enter — a marginal cost hovering right at the threshold cannot flip the
/// mode on and off every tick.
pub(super) fn heater_emergency_lever(
    sim: &SimSnapshot,
    slot: Option<&PlanTimeSlot>,
    deviation_kw: f64,
    is_incumbent: bool,
    alert_active: bool,
) -> Option<Lever> {
    let snap = sim.assets.get(crate::ids::ASSET_HEATER)?;
    // The heater's own answers (its thermostat rule under Normal/Absorb),
    // not a re-derivation from temperatures here.
    let emergency_heat_kw = snap.val("emergency_heat_kw")?;
    let absorb_headroom_kw = snap.val("absorb_headroom_kw")?;
    let threshold = if is_incumbent {
        HEATER_COMFORT_OVERRIDE_EUR_PER_KWH - LEVER_PREEMPTION_MARGIN_EUR_PER_KWH
    } else {
        HEATER_COMFORT_OVERRIDE_EUR_PER_KWH
    };

    if deviation_kw > 0.0 {
        // Import deviation: Curtail lets the tank drift toward ambient below
        // temp_min_c instead of the forced-on emergency heat — capacity is
        // however much of the currently-forced emergency draw that would free up.
        if !alert_active {
            return None;
        }
        if emergency_heat_kw <= 0.0 {
            return None; // thermostat isn't forcing emergency heat
        }
        Some(Lever {
            id: "heater_emergency",
            available_capacity_kw: emergency_heat_kw,
            marginal_cost_eur_per_kwh: HEATER_COMFORT_OVERRIDE_EUR_PER_KWH,
        })
    } else {
        // Surplus/export deviation: Absorb lets the tank heat past temp_max_c
        // up to temp_safety_max_c, soaking up otherwise-exported surplus.
        let slot = slot?;
        if slot.marginal_cost_export_eur_per_kwh <= threshold {
            return None;
        }
        if absorb_headroom_kw <= 0.0 {
            return None; // already at the true safety ceiling
        }
        Some(Lever {
            id: "heater_emergency",
            available_capacity_kw: absorb_headroom_kw,
            marginal_cost_eur_per_kwh: slot.marginal_cost_export_eur_per_kwh,
        })
    }
}

/// PV curtailment: backstop only, export-excess direction, priced at the
/// forgone export tariff — naturally ranks last.
pub(super) fn pv_curtailment_lever(
    slot: Option<&PlanTimeSlot>,
    deviation_kw: f64,
) -> Option<Lever> {
    let slot = slot?;
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
    let net_other_kw = super::projected_net_kw_except(
        sim,
        setpoints,
        live_pv_kw,
        live_base_load_kw,
        &[crate::ids::ASSET_EV, crate::ids::ASSET_BATTERY],
    );
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
    // The EV's own capability is 0 while unplugged or at/above its target.
    let charge_ceiling_kw = snap.cap_max_import_kw;
    if charge_ceiling_kw > 0.0 {
        let min_charge_kw = snap.values.get("min_charge_kw").copied().unwrap_or(0.0);
        let charge_kw = surplus_kw.min(charge_ceiling_kw);
        if charge_kw >= min_charge_kw {
            setpoints.insert(crate::ids::ASSET_EV.to_string(), charge_kw);
        }
    }
}
