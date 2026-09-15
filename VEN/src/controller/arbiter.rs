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
    /// Additional generation-limit tightening (kW, positive magnitude) the
    /// arbiter wants folded into `resolve_pv_generation_limit_kw`'s tighter-wins
    /// comparison.
    pub pv_generation_limit_tighten_kw: Option<f64>,
    /// kWh absorbed this tick, keyed by asset id — feeds the residual
    /// accumulator (§5.5). Only battery/EV are ever populated (the
    /// SoC-coupled resources the accumulator protects).
    pub absorbed_kwh_by_asset: HashMap<String, f64>,
    /// The cheapest lever actually used this tick, if any — fed back in as
    /// `incumbent_lever` next tick for the preemption-margin hysteresis.
    pub active_lever: Option<&'static str>,
    /// This tick's `projected_net_kw`/`deviation_kw` (diagnostics surface —
    /// `state::arbiter::ArbiterDiagnostics`, `GET /arbiter-diagnostics`).
    /// `None` only in the no-plan-yet startup window, where no plan target
    /// exists to compute a deviation against.
    pub net_kw: Option<f64>,
    pub dev_kw: Option<f64>,
}

/// Generalizes the former `apply_surplus_ev_overlay`'s `net_other_kw`
/// calculation: this tick's projected net site power, preferring
/// `live_pv_kw`/`live_base_load_kw` over the necessarily-stale `SimSnapshot`
/// for those two physics-driven inputs, and `setpoints` for every controlled
/// asset.
///
/// For battery/EV, `setpoints` must hold the command the adjustments start
/// from: `reconcile` seeds them with `AssetSnapshot.setpoint_kw` (the
/// arbiter's own last-tick command, the dead-beat integrator state), never the
/// plan's static per-slot allocation. Reading the plan allocation instead would
/// make the deviation signal blind to a correction already applied — the next
/// tick "rediscovers" the same deviation and re-applies a fresh correction on
/// top of it, an unbounded per-tick runaway rather than settling once
/// corrected (see
/// `reconcile_battery_converges_under_stationary_disturbance_not_runaway_to_clamp`).
pub fn projected_net_kw(
    sim: &SimSnapshot,
    setpoints: &HashMap<String, f64>,
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
            if let Some(forced_kw) = snap.forced_power_kw {
                return forced_kw;
            }
            if id.as_str() == crate::ids::ASSET_BATTERY || id.as_str() == crate::ids::ASSET_EV {
                return arbiter_levers::current_setpoint_kw(setpoints, sim, id);
            }
            let sp = setpoints.get(id).copied().unwrap_or(snap.power_kw);
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

    let net_kw = projected_net_kw(sim, &setpoints, live_pv_kw, live_base_load_kw);
    let dev_kw = deviation_kw(slot, net_kw);

    if dev_kw.abs() < DEAD_BAND_KW {
        return ArbiterOutcome {
            setpoints,
            net_kw: Some(net_kw),
            dev_kw: Some(dev_kw),
            ..Default::default()
        };
    }

    let inputs = LeverInputs {
        sim,
        slot,
        objective,
        plan_has_ev_allocation,
        overlay_enabled,
        incumbent_lever,
    };
    let applied = apply_ranked_levers(&mut setpoints, &DEVIATION_POLICY, &inputs, dev_kw);

    ArbiterOutcome {
        setpoints,
        heater_emergency_mode: applied.heater_emergency_mode,
        pv_generation_limit_tighten_kw: applied.pv_generation_limit_tighten_kw,
        absorbed_kwh_by_asset: applied.absorbed_kwh_by_asset,
        active_lever: applied.active_lever,
        net_kw: Some(net_kw),
        dev_kw: Some(dev_kw),
    }
}

/// How a pass may use the levers — the only thing that differs between the
/// arbiter's passes; the candidate/rank/apply machinery is shared
/// (`apply_ranked_levers`). Declared as data, not branched on per call site.
#[derive(Debug, Clone, Copy)]
pub(crate) struct LeverPolicy {
    /// The EV lever may also reduce charging the plan itself scheduled, and
    /// regardless of the opportunistic-overlay toggle — not only opportunistic
    /// charge.
    pub(crate) ev_overrides_plan: bool,
    /// Battery discharge follows the planner objective's refusal under
    /// `MaxRevenue`.
    pub(crate) battery_respects_objective: bool,
}

/// Deviation correction: never second-guesses the plan's own EV commitment
/// and keeps `MaxRevenue`'s discharge refusal.
pub(crate) const DEVIATION_POLICY: LeverPolicy = LeverPolicy {
    ev_overrides_plan: false,
    battery_respects_objective: true,
};

/// Everything a pass hands the shared lever machinery besides the setpoints.
pub(crate) struct LeverInputs<'a> {
    pub(crate) sim: &'a SimSnapshot,
    pub(crate) slot: &'a PlanTimeSlot,
    pub(crate) objective: PlannerObjective,
    pub(crate) plan_has_ev_allocation: bool,
    pub(crate) overlay_enabled: bool,
    pub(crate) incumbent_lever: Option<&'a str>,
}

/// What the shared lever loop did this tick.
#[derive(Debug, Default)]
pub(crate) struct AppliedLevers {
    pub(crate) heater_emergency_mode: Option<(bool, bool)>,
    pub(crate) pv_generation_limit_tighten_kw: Option<f64>,
    pub(crate) absorbed_kwh_by_asset: HashMap<String, f64>,
    pub(crate) active_lever: Option<&'static str>,
    /// Part of `|deviation_kw|` no lever could take (kW).
    pub(crate) unresolved_kw: f64,
}

/// Candidate → `rank_levers` → greedy apply, shared by every arbiter pass.
/// Positive `deviation_kw` = import to shed; negative = surplus to absorb.
/// Each lever consumes what it actually achieved, not what it was assigned.
pub(crate) fn apply_ranked_levers(
    setpoints: &mut HashMap<String, f64>,
    policy: &LeverPolicy,
    inputs: &LeverInputs,
    deviation_kw: f64,
) -> AppliedLevers {
    let sim = inputs.sim;
    let (ev_plan_allocated, ev_overlay_enabled) = if policy.ev_overrides_plan {
        (false, true)
    } else {
        (inputs.plan_has_ev_allocation, inputs.overlay_enabled)
    };
    let battery_objective = if policy.battery_respects_objective {
        inputs.objective
    } else {
        PlannerObjective::MinCost
    };

    let mut candidates = Vec::new();
    candidates.extend(battery_lever(setpoints, sim, inputs.slot, deviation_kw));
    candidates.extend(ev_lever(
        setpoints,
        sim,
        deviation_kw,
        ev_plan_allocated,
        ev_overlay_enabled,
    ));
    candidates.extend(heater_pause_lever(setpoints, deviation_kw));
    candidates.extend(heater_emergency_lever(
        sim,
        inputs.slot,
        deviation_kw,
        inputs.incumbent_lever == Some("heater_emergency"),
    ));
    candidates.extend(pv_curtailment_lever(inputs.slot, deviation_kw));

    let mut applied = AppliedLevers::default();
    let mut remaining_kw = deviation_kw.abs();
    for lever in rank_levers(candidates, inputs.incumbent_lever) {
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
        let signed_assigned_kw = if deviation_kw > 0.0 {
            assigned_kw
        } else {
            -assigned_kw
        };
        let achieved_kw = match lever.id {
            "battery" => {
                let delta = apply_battery_lever(setpoints, sim, signed_assigned_kw, battery_objective);
                if delta > 0.0 {
                    *applied
                        .absorbed_kwh_by_asset
                        .entry(crate::ids::ASSET_BATTERY.to_string())
                        .or_insert(0.0) += delta;
                }
                delta
            }
            "ev" => {
                let delta = apply_ev_lever(setpoints, sim, signed_assigned_kw);
                *applied
                    .absorbed_kwh_by_asset
                    .entry(crate::ids::ASSET_EV.to_string())
                    .or_insert(0.0) += delta;
                delta
            }
            "heater_pause" => apply_heater_pause_lever(setpoints, signed_assigned_kw),
            "heater_emergency" => {
                applied.heater_emergency_mode = Some(if deviation_kw > 0.0 {
                    (true, false) // Curtail
                } else {
                    (false, true) // Absorb
                });
                assigned_kw
            }
            "pv_curtail" => {
                applied.pv_generation_limit_tighten_kw = Some(assigned_kw);
                assigned_kw
            }
            _ => 0.0,
        };
        if achieved_kw > 0.0 {
            applied.active_lever.get_or_insert(lever.id);
            remaining_kw -= achieved_kw;
        }
    }
    applied.unresolved_kw = remaining_kw.max(0.0);
    applied
}

#[cfg(test)]
#[path = "tests/arbiter_tests.rs"]
mod arbiter_tests;
