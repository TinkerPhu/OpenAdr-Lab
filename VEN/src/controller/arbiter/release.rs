//! R-88: when a held correction is released, and how much it has displaced from plan.
//!
//! The deviation pass carries each storage asset's and EV's last applied setpoint forward as next
//! tick's baseline — otherwise every quiet tick would snap them back to plan and recreate the
//! deviation they just cancelled. That makes a correction state the arbiter holds, so it needs
//! an explicit end: it is released, all at once, on the first tick where putting the carried
//! levers back at their plan values would leave no deviation — the cause is gone.

use std::collections::HashMap;

use super::arbiter_levers::{current_setpoint_kw, lever_kind_of, LeverId};
use super::{deviation_kw, projected_net_kw_except, ArbiterTick};
use crate::controller::SimSnapshot;
use crate::entities::asset::ArbiterLeverKind;
use crate::entities::plan::PlanTimeSlot;

/// The assets whose setpoint the deviation pass carries from tick to tick — the only state a
/// correction holds: every storage asset, then every EV, each in roster order.
pub(super) fn carried_ids(sim: &SimSnapshot) -> Vec<&str> {
    let mut ids = sim.lever_asset_ids(&[ArbiterLeverKind::Storage]);
    ids.extend(sim.lever_asset_ids(&[ArbiterLeverKind::SurplusCharge]));
    ids
}

/// `setpoints` with every carried lever put back at its plan value from `plan_setpoints`.
pub(super) fn at_plan(
    setpoints: &HashMap<String, f64>,
    plan_setpoints: &HashMap<String, f64>,
    sim: &SimSnapshot,
) -> HashMap<String, f64> {
    let mut released = setpoints.clone();
    for id in carried_ids(sim) {
        if let Some(&plan_kw) = plan_setpoints.get(id) {
            released.insert(id.to_string(), plan_kw);
        }
    }
    released
}

/// The deviation that would remain once the correction is released: every carried lever at
/// its plan value, counted at what that value draws **once the command has landed**, and PV
/// under the generation limit that applies without the arbiter's own tightening. "Is the
/// cause gone" is a steady-state question — the next-tick projection includes a charger's
/// response lag, under which an EV commanded back to plan still draws for a few seconds and
/// the release would never be granted.
pub(super) fn deviation_without_correction_kw(
    tick: &ArbiterTick,
    slot: &PlanTimeSlot,
    setpoints: &HashMap<String, f64>,
    plan_setpoints: &HashMap<String, f64>,
) -> f64 {
    // PV as it would be without the arbiter's own curtailment — the third correction the
    // arbiter holds, carried through the inverter's generation limit instead of a setpoint.
    let live_pv_kw = tick.live_pv_released_kw.or(tick.live_pv_kw);
    let (sim, live_base_kw) = (tick.sim, tick.live_base_load_kw);
    let carried = carried_ids(sim);
    let others_kw = projected_net_kw_except(sim, setpoints, live_pv_kw, live_base_kw, &carried);
    let released = at_plan(setpoints, plan_setpoints, sim);
    let levers_at_plan_kw: f64 = carried
        .iter()
        .map(|id| {
            sim.assets[*id].power_when_command_lands_kw(current_setpoint_kw(&released, sim, id))
        })
        .sum();
    deviation_kw(slot, others_kw + levers_at_plan_kw, tick.limit_target_kw)
}

/// How far each asset in `ids` moved from `before_kw` to `after` (kW, signed), leaving out
/// the ones that did not move. One rule for both arbiter passes: the deviation pass measures
/// its carried levers against the plan, the limit pass its adjustments against its input.
pub(crate) fn setpoint_shift_kw<'a>(
    after: &HashMap<String, f64>,
    ids: impl IntoIterator<Item = &'a str>,
    before_kw: impl Fn(&str) -> f64,
) -> HashMap<String, f64> {
    ids.into_iter()
        .filter_map(|id| {
            let shift_kw = after.get(id).copied()? - before_kw(id);
            (shift_kw.abs() > 1e-9).then(|| (id.to_string(), shift_kw))
        })
        .collect()
}

/// The carried levers' displacement from plan — what a held correction costs the battery/EV.
pub(super) fn displaced_from_plan_kw(
    setpoints: &HashMap<String, f64>,
    plan_setpoints: &HashMap<String, f64>,
    sim: &SimSnapshot,
) -> HashMap<String, f64> {
    setpoint_shift_kw(setpoints, carried_ids(sim), |id| {
        plan_setpoints.get(id).copied().unwrap_or(0.0)
    })
}

/// The lever a held correction is held by: the first carried lever still off-plan.
pub(super) fn holding_lever(
    displaced_kw: &HashMap<String, f64>,
    sim: &SimSnapshot,
) -> Option<String> {
    let id = carried_ids(sim)
        .into_iter()
        .find(|id| displaced_kw.contains_key(*id))?;
    let kind = sim.assets[id].arbiter_lever?.kind;
    Some(LeverId::of_asset(lever_kind_of(kind), id).key())
}
