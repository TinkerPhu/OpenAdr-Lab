//! Limit enforcement — the arbiter's second pass (GB-47).
//!
//! The deviation pass corrects deviation *from the plan*, so it cannot fix a
//! plan that itself breaks a hard import limit (a timed-out MILP incumbent
//! that put a heater stage inside a cap) and, being off by default, does not
//! catch unforecast load either. This pass does both: once per tick, last in
//! the setpoint pipeline, it projects site import from this tick's setpoint
//! map and sheds whatever exceeds the limit target, through the same lever
//! machinery (`super::apply_ranked_levers`) under its own `LeverPolicy`.
//!
//! Memoryless: the only state carried between ticks is its own incumbent
//! lever; while that is a switching lever (heater stage, EV minimum-charge
//! floor) the target sits `LIMIT_RELEASE_HYSTERESIS_KW` lower, so a shed stage
//! is restored only once it fits with room to spare.
//!
//! A lagging lever (the EV charger applies a command a tick late) has two powers:
//! what it draws this tick, which the lever loop sheds against and bridges, and
//! what this tick's command draws once it lands. Judged on this tick alone, a cut
//! charger reads as "within the limit" and is released; its full command lands a
//! tick later above the limit, and the pass alternates between the two every tick
//! (found by the opportunistic forecast, 051 group 7). `hold_where_commands_land`
//! keeps the landed power under the target as well. The opportunistic step applies
//! it; planning mode does not yet (R-145: it changes planning decisions, which this
//! change keeps identical).

use std::collections::HashMap;
use ts_rs::TS;

use super::arbiter_levers::{current_setpoint_kw, kind_of_key, LEVER_EV, LEVER_HEATER_PAUSE};
pub use super::SetpointBoundsKw;
use super::{
    apply_ranked_levers, projected_landed_net_kw, projected_net_kw, ArbiterTick, LeverInputs,
    LeverPolicy, DEAD_BAND_KW,
};

/// How far below a hard import limit both passes steer (kW) — room for the
/// one-tick lag between projection and physics.
pub const LIMIT_MARGIN_KW: f64 = 0.1;
/// While a switching lever led the limit pass last tick its target sits this
/// much lower (kW): restoring a shed stage needs room to spare, or a load
/// hovering at the threshold would switch a heater relay every second.
pub const LIMIT_RELEASE_HYSTERESIS_KW: f64 = 0.2;

/// Levers whose release switches discretely — a heater stage, the EV's
/// minimum-charge floor. The battery adjusts continuously and gets no
/// hysteresis, so it can hold import right at the target.
const SWITCHING_LEVERS: [&str; 2] = [LEVER_HEATER_PAUSE, LEVER_EV];

/// A hard limit may also cut charging the plan itself scheduled, and battery
/// discharge ignores `MaxRevenue`'s refusal — the limit outranks the objective.
pub(crate) const LIMIT_POLICY: LeverPolicy = LeverPolicy {
    ev_overrides_plan: true,
    battery_respects_objective: false,
    admits: super::ALL_LEVERS,
};

/// The hard import limit in force now (kW): 0 while an alert window is active
/// (strictest wins), else the VTN's capacity import limit, if any.
pub fn hard_import_limit_kw(
    capacity_import_limit_kw: Option<f64>,
    alert_active: bool,
) -> Option<f64> {
    if alert_active {
        Some(0.0)
    } else {
        capacity_import_limit_kw
    }
}

/// The import ceiling both passes steer to: `LIMIT_MARGIN_KW` below the hard
/// limit, a further `LIMIT_RELEASE_HYSTERESIS_KW` below while a switching lever
/// led the limit pass last tick (`incumbent_lever`).
pub fn limit_target_kw(hard_limit_kw: Option<f64>, incumbent_lever: Option<&str>) -> Option<f64> {
    let switching = |l: &str| SWITCHING_LEVERS.contains(&kind_of_key(l));
    let hysteresis_kw = if incumbent_lever.is_some_and(switching) {
        LIMIT_RELEASE_HYSTERESIS_KW
    } else {
        0.0
    };
    hard_limit_kw.map(|limit_kw| limit_kw - LIMIT_MARGIN_KW - hysteresis_kw)
}

/// After the limit pass: cut each lagging lever (the EV charger, the one asset that applies a
/// command a tick late) so the commands in `setpoints` land within the pass's target too, and
/// record it on `outcome` (`None`: no limit in force, nothing to hold). This tick's import is
/// the lever loop's: it already shed and bridged it, and nothing done now changes what a lagging
/// asset draws this tick, so the cut here is neither bridged nor unresolved. The lever that held
/// becomes the pass's incumbent for next tick.
pub fn hold_where_commands_land(
    tick: &ArbiterTick,
    setpoints: &mut HashMap<String, f64>,
    outcome: Option<&mut LimitPassOutcome>,
) {
    use crate::entities::asset::ArbiterLeverKind;
    let Some(outcome) = outcome else {
        return;
    };
    let mut excess_kw =
        projected_landed_net_kw(tick.sim, setpoints, tick.live_kw) - outcome.target_kw;
    let mut held = None;
    for id in tick.sim.lever_asset_ids(&[ArbiterLeverKind::SurplusCharge]) {
        if excess_kw < DEAD_BAND_KW {
            break;
        }
        let before_kw = current_setpoint_kw(setpoints, tick.sim, id);
        let effect = super::arbiter_levers::apply_ev_lever(setpoints, tick.sim, id, excess_kw);
        if effect.landed_kw > 0.0 {
            excess_kw -= effect.landed_kw;
            let shift_kw = current_setpoint_kw(setpoints, tick.sim, id) - before_kw;
            *outcome
                .adjusted_kw_by_asset
                .entry(id.to_string())
                .or_default() += shift_kw;
            held.get_or_insert_with(|| {
                super::arbiter_levers::LeverId::of_asset(LEVER_EV, id).key()
            });
        }
    }
    if let Some(lever) = held {
        outcome.active_lever.get_or_insert(lever);
    }
}

/// What the limit pass saw and did this tick (diagnostics + residual feed).
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, TS)]
pub struct LimitPassOutcome {
    /// The import ceiling steered to (kW).
    pub target_kw: f64,
    /// Projected import above `target_kw` before the pass (kW; ≤ 0 = within).
    pub excess_kw: f64,
    /// Excess no lever could shed — forced power, the heater's comfort floor
    /// outside an alert, or exhausted levers (kW). Reported, never hidden.
    pub unresolved_kw: f64,
    /// The first lever that shed anything (`<kind>:<asset_id>`); next tick's incumbent.
    pub active_lever: Option<String>,
    /// `(curtail, absorb)` per asset, set only by an alert's Curtail.
    pub heater_emergency_mode: HashMap<String, (bool, bool)>,
    /// Setpoint change per asset, after − before (kW).
    pub adjusted_kw_by_asset: HashMap<String, f64>,
    /// Each storage asset's share that only bridges a lagging command (kW) — see
    /// `ArbiterTick::prev_bridge_kw_by_asset`.
    pub bridge_kw_by_asset: HashMap<String, f64>,
}

/// Runs the limit pass on `setpoints` (the tick's final map, after deviation
/// correction, dispatch override and comms-loss clamp). `None` when no hard
/// import limit is in force. `bounds_kw` — per-asset setpoint bounds the
/// pass must stay inside (the comms-loss clamp's).
pub fn enforce_import_limit(
    tick: &ArbiterTick,
    setpoints: &mut HashMap<String, f64>,
    incumbent_lever: Option<&str>,
    bounds_kw: &SetpointBoundsKw,
) -> Option<LimitPassOutcome> {
    let target_kw = tick.limit_target_kw?;
    let net_kw = projected_net_kw(tick.sim, setpoints, tick.live_kw);
    let excess_kw = net_kw - target_kw;
    let mut outcome = LimitPassOutcome {
        target_kw,
        excess_kw,
        ..Default::default()
    };
    if excess_kw < DEAD_BAND_KW {
        return Some(outcome);
    }

    let before = setpoints.clone();
    let inputs = LeverInputs {
        tick,
        incumbent_lever,
        bounds_kw,
    };
    let applied = apply_ranked_levers(setpoints, &LIMIT_POLICY, &inputs, excess_kw);
    outcome.unresolved_kw = applied.unresolved_kw;
    outcome.active_lever = applied.active_lever;
    outcome.heater_emergency_mode = applied.heater_emergency_mode;
    outcome.bridge_kw_by_asset = applied.bridge_kw_by_asset;
    let moved_ids: Vec<String> = setpoints.keys().cloned().collect();
    outcome.adjusted_kw_by_asset =
        super::release::setpoint_shift_kw(setpoints, moved_ids.iter().map(String::as_str), |id| {
            current_setpoint_kw(&before, tick.sim, id)
        });
    Some(outcome)
}
