//! The deviation arbiter (see `docs/architecture/VEN_ARCHITECTURE.md`,
//! `openspec/changes/deviation-arbiter/`).
//!
//! Single owner of every reactive (non-plan, non-VTN-override) actuator
//! adjustment per tick. Absorbs — moves, does not duplicate — the former
//! `dispatcher::apply_surplus_ev_overlay` (EV lever) and
//! `dispatcher::apply_battery_correction_overlay` (battery lever), and adds
//! two new levers: heater (pause-within-comfort-band, plus
//! `HeaterEmergencyMode::Curtail`/`Absorb` for obligation-penalty-driven
//! cases) and PV curtailment (export-excess backstop).
//!
//! Structurally rules out feature 017's two root causes (§1): there is
//! exactly one function, called once per tick, with one internal ranked-
//! execution loop (no second writer to fight), and every physics-driven
//! input (`live_pv_kw`, `live_base_load_kw`) is this tick's previewed value,
//! never a stale snapshot.

use std::collections::HashMap;

mod arbiter_levers;

use crate::controller::dispatcher::predict_heater_forced_kw;
use crate::controller::SimSnapshot;
use crate::entities::plan::PlanTimeSlot;
use crate::entities::planner_params::PlannerObjective;
use arbiter_levers::{
    apply_battery_lever, apply_ev_lever, apply_ev_lever_opportunistic, apply_heater_pause_lever,
    battery_lever, ev_lever, heater_emergency_lever, heater_pause_lever, pv_curtailment_lever,
    Lever,
};

/// Illustrative defaults — the design doc's own open-question list notes none
/// of these have a numeric default from the source material; these are the
/// values chosen at implementation time, not derived from a worked example.
pub const HEATER_COMFORT_OVERRIDE_EUR_PER_KWH: f64 = 0.40;
/// A challenger lever must be cheaper than the incumbent by more than this
/// margin to preempt it (§4a.1 — prevents tick-to-tick chatter between two
/// near-equal-cost levers).
pub const LEVER_PREEMPTION_MARGIN_EUR_PER_KWH: f64 = 0.02;
/// Fraction of an SoC-coupled asset's capacity-at-last-plan that its
/// accumulated absorbed-kWh may reach before `PlanTrigger::ResidualThreshold`
/// fires (§5.5).
pub const RESIDUAL_THRESHOLD_FRACTION: f64 = 0.2;
/// Minimum interval between `PlanTrigger::ResidualThreshold` firings (§5b —
/// prevents replan thrashing if the underlying cause is persistent).
pub const RESIDUAL_COOLDOWN_S: i64 = 900;
/// Below this magnitude, a deviation is treated as noise and no lever fires
/// (mirrors the EV overlay's pre-existing 0.1 kW floor).
pub const DEAD_BAND_KW: f64 = 0.1;

/// Outcome of `reconcile`. Heater-mode and PV-limit decisions can't travel
/// through the plain setpoints map (both are separate `SimState::tick()`
/// parameters, not setpoint entries).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ArbiterOutcome {
    pub setpoints: HashMap<String, f64>,
    /// `(curtail, absorb)` — mirrors `SimInjectState`'s two independent flags.
    pub heater_emergency_mode: Option<(bool, bool)>,
    /// Additional export-limit tightening (kW, positive magnitude) the
    /// arbiter wants folded into `resolve_pv_export_limit_kw`'s tighter-wins
    /// comparison.
    pub pv_export_limit_tighten_kw: Option<f64>,
    /// kWh absorbed this tick, keyed by asset id — feeds the residual
    /// accumulator (§5.5). Only battery/EV are ever populated (the
    /// SoC-coupled resources the accumulator protects).
    pub absorbed_kwh_by_asset: HashMap<String, f64>,
    /// The cheapest lever actually used this tick, if any — fed back in as
    /// `incumbent_lever` next tick for the preemption-margin hysteresis.
    pub active_lever: Option<&'static str>,
}

/// Generalizes the former `apply_surplus_ev_overlay`'s `net_other_kw`
/// calculation: this tick's projected net site power, preferring
/// `live_pv_kw`/`live_base_load_kw` over the necessarily-stale `SimSnapshot`
/// for those two physics-driven inputs, and `base_setpoints` (the plan's own
/// allocation, before any arbiter adjustment) for heater.
///
/// Battery/EV are deliberately excluded from the `base_setpoints` fallback:
/// both are dead-beat correctors whose `apply_*_lever` already treats
/// `AssetSnapshot.setpoint_kw` (the arbiter's own last-tick command) as the
/// integrator state, not the plan's static per-slot allocation. Reading
/// `base_setpoints` here instead would make the deviation signal blind to a
/// correction already applied — the next tick "rediscovers" the same
/// deviation and re-applies a fresh correction on top of it, an unbounded
/// per-tick runaway rather than settling once corrected (see
/// `reconcile_battery_converges_under_stationary_disturbance_not_runaway_to_clamp`).
pub fn projected_net_kw(
    sim: &SimSnapshot,
    base_setpoints: &HashMap<String, f64>,
    live_pv_kw: Option<f64>,
    live_base_load_kw: Option<f64>,
) -> f64 {
    sim.assets
        .iter()
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
            if id.as_str() == crate::ids::ASSET_BATTERY || id.as_str() == crate::ids::ASSET_EV {
                return snap.setpoint_kw;
            }
            let sp = base_setpoints.get(id).copied().unwrap_or(snap.power_kw);
            if sp.abs() > 1e20 {
                snap.power_kw
            } else {
                sp
            }
        })
        .sum()
}

/// `projected_net_kw − plan_signed_net_kw`. Positive = importing more than
/// planned (need an import-reducing lever); negative = exporting more than
/// planned / surplus (need an export-absorbing lever).
pub fn deviation_kw(plan_slot: &PlanTimeSlot, projected_net_kw: f64) -> f64 {
    let plan_signed_net_kw = plan_slot.net_import_kw - plan_slot.net_export_kw;
    projected_net_kw - plan_signed_net_kw
}

/// The greedy ranking loop: exclude zero-or-below-capacity levers outright
/// (§5.3's explicit requirement — not merely deprioritize), sort remaining by
/// marginal cost ascending, consume `remaining_kw` lever by lever. A
/// challenger must beat the incumbent (last tick's `active_lever`) by more
/// than `LEVER_PREEMPTION_MARGIN_EUR_PER_KWH` to take the top slot — prevents
/// tick-to-tick chatter between two near-equal-cost levers (§4a.1).
fn rank_levers(mut levers: Vec<Lever>, incumbent_lever: Option<&str>) -> Vec<Lever> {
    levers.sort_by(|a, b| {
        a.marginal_cost_eur_per_kwh
            .partial_cmp(&b.marginal_cost_eur_per_kwh)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    if let (Some(incumbent_id), Some(cheapest)) = (incumbent_lever, levers.first().copied()) {
        if cheapest.id != incumbent_id {
            if let Some(incumbent_idx) = levers.iter().position(|l| l.id == incumbent_id) {
                let incumbent = levers[incumbent_idx];
                let beats_by_more_than_margin = incumbent.marginal_cost_eur_per_kwh
                    - cheapest.marginal_cost_eur_per_kwh
                    > LEVER_PREEMPTION_MARGIN_EUR_PER_KWH;
                if !beats_by_more_than_margin {
                    // Keep the incumbent in front — swap it to the head.
                    levers.swap(0, incumbent_idx);
                }
            }
        }
    }
    levers
}

/// Top-level entry point, called once per tick from
/// `tasks::sim_tick::helpers::build_tick_setpoints` in place of the former
/// direct call to `apply_surplus_ev_overlay`.
#[allow(clippy::too_many_arguments)]
pub fn reconcile(
    sim: &SimSnapshot,
    base_setpoints: &HashMap<String, f64>,
    plan_slot: Option<&PlanTimeSlot>,
    objective: PlannerObjective,
    plan_has_ev_allocation: bool,
    overlay_enabled: bool,
    live_pv_kw: Option<f64>,
    live_base_load_kw: Option<f64>,
    incumbent_lever: Option<&str>,
) -> ArbiterOutcome {
    let mut setpoints = base_setpoints.clone();
    // Carry forward the dead-beat correctors' own last-applied setpoint as
    // the baseline, not the plan's static per-slot allocation — otherwise a
    // tick where the corresponding lever doesn't fire (deviation within dead
    // band, or a cheaper lever absorbed it) would silently revert the
    // correction, immediately re-creating the very deviation it just
    // resolved whenever the underlying disturbance is persistent rather than
    // transient. Mirrors `projected_net_kw`'s use of `snap.setpoint_kw` for
    // the same two assets, above.
    for id in [crate::ids::ASSET_BATTERY, crate::ids::ASSET_EV] {
        if let Some(snap) = sim.assets.get(id) {
            setpoints.insert(id.to_string(), snap.setpoint_kw);
        }
    }

    let Some(slot) = plan_slot else {
        // No active plan yet (startup window): same fallback as the
        // pre-arbiter no-plan branch — opportunistic EV-only, since there's
        // no plan target to compute a deviation against.
        apply_ev_lever_opportunistic(
            &mut setpoints,
            sim,
            live_pv_kw,
            live_base_load_kw,
            plan_has_ev_allocation,
            overlay_enabled,
        );
        return ArbiterOutcome {
            setpoints,
            ..Default::default()
        };
    };

    let net_kw = projected_net_kw(sim, base_setpoints, live_pv_kw, live_base_load_kw);
    let dev_kw = deviation_kw(slot, net_kw);

    if dev_kw.abs() < DEAD_BAND_KW {
        return ArbiterOutcome {
            setpoints,
            ..Default::default()
        };
    }

    let mut candidates = Vec::new();
    candidates.extend(battery_lever(sim, slot, dev_kw));
    candidates.extend(ev_lever(
        sim,
        dev_kw,
        plan_has_ev_allocation,
        overlay_enabled,
    ));
    candidates.extend(heater_pause_lever(base_setpoints, dev_kw));
    candidates.extend(heater_emergency_lever(
        sim,
        slot,
        dev_kw,
        incumbent_lever == Some("heater_emergency"),
    ));
    candidates.extend(pv_curtailment_lever(slot, dev_kw));

    let ranked = rank_levers(candidates, incumbent_lever);

    let mut remaining_kw = dev_kw.abs();
    let mut absorbed_kwh_by_asset = HashMap::new();
    let mut heater_emergency_mode = None;
    let mut pv_export_limit_tighten_kw = None;
    let mut active_lever = None;

    for lever in ranked {
        if remaining_kw < DEAD_BAND_KW {
            break;
        }
        let assigned_kw = remaining_kw.min(lever.available_capacity_kw);
        if assigned_kw <= 0.0 {
            continue;
        }
        // Sign convention: positive assigned_kw always means "reduce import /
        // increase export by this much" — apply_* functions below translate
        // that into the correct setpoint-delta direction per asset.
        let signed_assigned_kw = if dev_kw > 0.0 {
            assigned_kw
        } else {
            -assigned_kw
        };
        match lever.id {
            "battery" => {
                let delta = apply_battery_lever(&mut setpoints, sim, signed_assigned_kw, objective);
                if delta > 0.0 {
                    *absorbed_kwh_by_asset
                        .entry(crate::ids::ASSET_BATTERY.to_string())
                        .or_insert(0.0) += delta;
                    active_lever.get_or_insert(lever.id);
                    remaining_kw -= delta;
                    continue;
                }
            }
            "ev" => {
                apply_ev_lever(&mut setpoints, sim, signed_assigned_kw);
                *absorbed_kwh_by_asset
                    .entry(crate::ids::ASSET_EV.to_string())
                    .or_insert(0.0) += assigned_kw;
                active_lever.get_or_insert(lever.id);
            }
            "heater_pause" => {
                apply_heater_pause_lever(&mut setpoints, signed_assigned_kw);
                active_lever.get_or_insert(lever.id);
            }
            "heater_emergency" => {
                heater_emergency_mode = Some(if dev_kw > 0.0 {
                    (true, false) // Curtail
                } else {
                    (false, true) // Absorb
                });
                active_lever.get_or_insert(lever.id);
            }
            "pv_curtail" => {
                pv_export_limit_tighten_kw = Some(assigned_kw);
                active_lever.get_or_insert(lever.id);
            }
            _ => {}
        }
        remaining_kw -= assigned_kw;
    }

    ArbiterOutcome {
        setpoints,
        heater_emergency_mode,
        pv_export_limit_tighten_kw,
        absorbed_kwh_by_asset,
        active_lever,
    }
}

#[cfg(test)]
#[path = "tests/arbiter_tests.rs"]
mod arbiter_tests;
