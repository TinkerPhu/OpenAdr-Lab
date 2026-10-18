//! The arbiter's shared lever machinery: every pass builds its candidates here (one lever
//! per declaring asset, `arbiter_levers`), ranks them by marginal cost and applies them
//! greedily. What differs between passes is declared as a `LeverPolicy`, never branched on
//! per call site.

use std::collections::HashMap;

use super::arbiter_levers::{
    apply_battery_lever, apply_ev_lever, apply_heater_pause_lever, battery_lever, ev_lever,
    heater_emergency_lever, heater_pause_lever, pv_curtailment_lever, Lever, LeverId,
    LEVER_BATTERY, LEVER_EV, LEVER_HEATER_EMERGENCY, LEVER_HEATER_PAUSE, LEVER_PV_CURTAIL,
};
use super::{
    lag_bridge, ArbiterTick, EvOverlay, DEAD_BAND_KW, LEVER_PREEMPTION_MARGIN_EUR_PER_KWH,
};
use crate::entities::asset::ArbiterLeverKind;
use crate::entities::planner_params::PlannerObjective;

/// The greedy ranking loop: exclude zero-or-below-capacity levers outright
/// (§5.3's explicit requirement — not merely deprioritize), sort remaining by
/// marginal cost ascending, consume `remaining_kw` lever by lever. The sort is
/// stable, so equal costs keep candidate order: lever-kind precedence, then
/// roster order. A challenger must beat the incumbent (last tick's
/// `active_lever`) by more than `LEVER_PREEMPTION_MARGIN_EUR_PER_KWH` to take the
/// top slot — prevents tick-to-tick chatter between two near-equal-cost levers
/// (§4a.1).
pub(super) fn rank_levers(mut levers: Vec<Lever>, incumbent_lever: Option<&str>) -> Vec<Lever> {
    levers.sort_by(|a, b| {
        a.marginal_cost_eur_per_kwh
            .partial_cmp(&b.marginal_cost_eur_per_kwh)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    if let (Some(incumbent_id), Some(cheapest)) = (incumbent_lever, levers.first().cloned()) {
        if cheapest.id.key() != incumbent_id {
            if let Some(incumbent_idx) = levers.iter().position(|l| l.id.key() == incumbent_id) {
                let incumbent = levers[incumbent_idx].clone();
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
    /// The lever kinds this pass may use (`LEVER_*`).
    pub(crate) admits: &'static [&'static str],
}

/// Every lever kind (the tie-break precedence between kinds is `lever_candidates`' build order).
pub(crate) const ALL_LEVERS: &[&str] = &[
    LEVER_BATTERY,
    LEVER_EV,
    LEVER_HEATER_PAUSE,
    LEVER_HEATER_EMERGENCY,
    LEVER_PV_CURTAIL,
];

/// Deviation correction: never second-guesses the plan's own EV commitment
/// and keeps `MaxRevenue`'s discharge refusal.
pub(super) const DEVIATION_POLICY: LeverPolicy = LeverPolicy {
    ev_overrides_plan: false,
    battery_respects_objective: true,
    admits: ALL_LEVERS,
};

/// A VTN dispatch window: met by storage alone, whatever the planner objective (the VTN's
/// instruction outranks it).
pub(crate) const DISPATCH_POLICY: LeverPolicy = LeverPolicy {
    ev_overrides_plan: false,
    battery_respects_objective: false,
    admits: &[LEVER_BATTERY],
};

/// Per-asset `(min, max)` setpoint bounds (kW) a pass must stay inside — the
/// comms-loss fail-safe's clamp (`tasks::sim_tick::dispatch_override`).
pub type SetpointBoundsKw = HashMap<String, (f64, f64)>;

/// Everything a pass hands the shared lever machinery besides the setpoints.
pub(crate) struct LeverInputs<'a> {
    pub(crate) tick: &'a ArbiterTick<'a>,
    /// This pass's own last-tick lever (preemption-margin hysteresis).
    pub(crate) incumbent_lever: Option<&'a str>,
    pub(crate) bounds_kw: &'a SetpointBoundsKw,
}

/// What the shared lever loop did this tick.
#[derive(Debug, Default)]
pub(crate) struct AppliedLevers {
    pub(crate) heater_emergency_mode: HashMap<String, (bool, bool)>,
    pub(crate) pv_generation_limit_tighten_kw: Option<f64>,
    pub(crate) active_lever: Option<String>,
    /// Part of `|deviation_kw|` no lever could take (kW).
    pub(crate) unresolved_kw: f64,
    /// Each storage asset's share of this tick's setpoint that only bridges a lagging
    /// command (kW, setpoint sign) — see `ArbiterTick::prev_bridge_kw_by_asset`.
    pub(crate) bridge_kw_by_asset: HashMap<String, f64>,
}

/// Every lever this tick offers, in candidate order: lever-kind precedence (storage, EV,
/// heater pause, emergency, PV — the order below),
/// then roster order. One lever per asset that declares it; the emergency lever is offered
/// by every asset answering the emergency what-ifs, PV curtailment by the site.
fn lever_candidates(
    setpoints: &HashMap<String, f64>,
    policy: &LeverPolicy,
    inputs: &LeverInputs,
    deviation_kw: f64,
) -> Vec<Lever> {
    let (tick, sim) = (inputs.tick, inputs.tick.sim);
    let roster = sim.assets_in_roster_order();
    let declares = |kind: ArbiterLeverKind| {
        roster
            .iter()
            .filter(move |(_, a)| a.arbiter_lever.is_some_and(|l| l.kind == kind))
            .map(|(id, _)| *id)
    };
    let overlay_of = |id: &str| {
        if policy.ev_overrides_plan {
            EvOverlay::OVERRIDING
        } else {
            tick.ev_overlay.get(id).copied().unwrap_or_default()
        }
    };
    let mut candidates = Vec::new();
    for id in declares(ArbiterLeverKind::Storage) {
        let bounds_kw = inputs.bounds_kw.get(id).copied();
        candidates.extend(battery_lever(
            setpoints,
            sim,
            id,
            tick.plan_slot,
            deviation_kw,
            bounds_kw,
        ));
    }
    for id in declares(ArbiterLeverKind::SurplusCharge) {
        candidates.extend(ev_lever(setpoints, sim, id, deviation_kw, overlay_of(id)));
    }
    for id in declares(ArbiterLeverKind::PausableLoad) {
        candidates.extend(heater_pause_lever(setpoints, sim, id, deviation_kw));
    }
    for (id, _) in roster
        .iter()
        .filter(|(_, a)| a.emergency_what_ifs.is_some())
    {
        let incumbent_key = LeverId::of_asset(LEVER_HEATER_EMERGENCY, id).key();
        candidates.extend(heater_emergency_lever(
            sim,
            id,
            tick.plan_slot,
            deviation_kw,
            inputs.incumbent_lever == Some(incumbent_key.as_str()),
            tick.alert_active,
        ));
    }
    candidates.extend(pv_curtailment_lever(tick.plan_slot, deviation_kw));
    candidates.retain(|l| policy.admits.contains(&l.id.kind));
    candidates
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
    let (tick, sim) = (inputs.tick, inputs.tick.sim);
    let storage_objective = if policy.battery_respects_objective {
        tick.objective
    } else {
        PlannerObjective::MinCost
    };
    let candidates = lever_candidates(setpoints, policy, inputs, deviation_kw);

    let mut applied = AppliedLevers::default();
    let mut remaining_kw = deviation_kw.abs();
    let mut bridge = lag_bridge::LagBridge::default();
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
        let achieved_kw = match (lever.id.kind, lever.id.asset_id.as_deref()) {
            (LEVER_BATTERY, Some(id)) => bridge.battery_moved(
                id,
                apply_battery_lever(
                    setpoints,
                    sim,
                    id,
                    signed_assigned_kw,
                    storage_objective,
                    inputs.bounds_kw.get(id).copied(),
                ),
                deviation_kw,
            ),
            (LEVER_EV, Some(id)) => {
                bridge.ev_moved(apply_ev_lever(setpoints, sim, id, signed_assigned_kw))
            }
            (LEVER_HEATER_PAUSE, Some(id)) => {
                apply_heater_pause_lever(setpoints, sim, id, signed_assigned_kw)
            }
            (LEVER_HEATER_EMERGENCY, Some(id)) => {
                let mode = if deviation_kw > 0.0 {
                    // Curtail (alerts only): no planned stage either.
                    setpoints.insert(id.to_string(), 0.0);
                    (true, false)
                } else {
                    (false, true) // Absorb
                };
                applied.heater_emergency_mode.insert(id.to_string(), mode);
                assigned_kw
            }
            (LEVER_PV_CURTAIL, _) => {
                applied.pv_generation_limit_tighten_kw = Some(assigned_kw);
                assigned_kw
            }
            _ => 0.0,
        };
        if achieved_kw > 0.0 {
            applied.active_lever.get_or_insert_with(|| lever.id.key());
            remaining_kw -= achieved_kw;
        }
    }
    applied.unresolved_kw = remaining_kw.max(0.0);
    applied.bridge_kw_by_asset = bridge.into_bridge_kw_by_asset();
    applied
}

#[cfg(test)]
#[path = "ranked_frozen_tests.rs"]
mod ranked_frozen_tests;
