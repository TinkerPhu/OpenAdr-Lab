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

pub(crate) mod arbiter_levers;
mod decision;
mod dispatch;
mod emergency;
mod lag_bridge;
pub mod limit;
mod live;
mod opportunistic;
mod ranked;
mod release;
mod stages;

pub use decision::decision_event;
pub use dispatch::meet_dispatch_setpoint;
pub use emergency::heater_emergency_modes;
pub use live::{projected_net_kw, projected_net_kw_except, tighten_generation, LiveKwByAsset};
pub use opportunistic::{
    opportunistic_requests, opportunistic_step, OpportunisticOutcome, OpportunisticRequests,
    RequestTargets,
};
#[cfg(test)]
use ranked::rank_levers;
pub use ranked::SetpointBoundsKw;
pub(crate) use ranked::{apply_ranked_levers, LeverInputs, LeverPolicy, ALL_LEVERS};
use ranked::{DEVIATION_POLICY, DISPATCH_POLICY};
pub use stages::{dispatch_stages, DispatchStages};

use crate::controller::SimSnapshot;
use crate::entities::asset::ArbiterLeverKind;
use crate::entities::plan::PlanTimeSlot;
use crate::entities::planner_params::PlannerObjective;
use arbiter_levers::apply_ev_lever_opportunistic;
#[cfg(test)]
use arbiter_levers::{
    apply_battery_lever, apply_heater_pause_lever, heater_emergency_lever, heater_pause_lever,
    Lever, LeverId,
};

/// One EV's opportunistic regime this tick: whether the plan allocates it charging now (its
/// own commitment, never second-guessed) and whether its own settings enable opportunistic
/// charging.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct EvOverlay {
    pub plan_allocates: bool,
    pub enabled: bool,
}

impl EvOverlay {
    /// A pass that may also cut what the plan scheduled (the limit pass) treats every EV so.
    pub(crate) const OVERRIDING: EvOverlay = EvOverlay {
        plan_allocates: false,
        enabled: true,
    };

    /// Whether this EV may be offered a surplus (and have that charge clawed back).
    pub fn offers_surplus(&self) -> bool {
        !self.plan_allocates && self.enabled
    }
}

/// Each EV's overlay this tick, by asset id. An EV without an entry is offered nothing.
pub type EvOverlays = HashMap<String, EvOverlay>;

/// The overlay of every EV in the roster (`ArbiterLeverKind::SurplusCharge`): the plan's
/// allocation from `plan`, the setting from `enabled`, which answers per asset id.
pub fn ev_overlays(
    sim: &SimSnapshot,
    plan: Option<&crate::entities::plan::Plan>,
    now: chrono::DateTime<chrono::Utc>,
    enabled: impl Fn(&str) -> bool,
) -> EvOverlays {
    sim.lever_asset_ids(&[ArbiterLeverKind::SurplusCharge])
        .into_iter()
        .map(|id| {
            let overlay = EvOverlay {
                plan_allocates: plan
                    .is_some_and(|p| crate::controller::dispatcher::plan_allocates(p, now, id)),
                enabled: enabled(id),
            };
            (id.to_string(), overlay)
        })
        .collect()
}

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
    /// `(curtail, absorb)` per asset whose emergency mode the arbiter set — mirrors
    /// `SimInjectState`'s two independent flags. Empty when it set none.
    pub heater_emergency_mode: HashMap<String, (bool, bool)>,
    /// Additional generation-limit tightening (kW, positive magnitude) the
    /// arbiter wants folded into `resolve_pv_generation_limit_kw`'s tighter-wins
    /// comparison.
    pub pv_generation_limit_tighten_kw: Option<f64>,
    /// How far the deviation pass holds battery/EV off their plan values this tick (kW,
    /// signed) — the correction's displacement, which `residual_kwh_by_asset` turns into
    /// energy for the replan backstop. Empty when nothing is held.
    pub displaced_kw_by_asset: HashMap<String, f64>,
    /// The cheapest lever actually used this tick, if any (`<kind>:<asset_id>`) — fed back
    /// in as `incumbent_lever` next tick for the preemption-margin hysteresis.
    pub active_lever: Option<String>,
    /// This tick's `projected_net_kw`/`deviation_kw` (diagnostics surface —
    /// `state::arbiter::ArbiterDiagnostics`, `GET /arbiter-diagnostics`).
    /// `None` only in the no-plan-yet startup window, where no plan target
    /// exists to compute a deviation against.
    pub net_kw: Option<f64>,
    pub dev_kw: Option<f64>,
    /// The deviation there would be with battery and EV back at their plan values — what
    /// decides whether a held correction is released (R-88). `None` without a plan slot.
    pub dev_without_correction_kw: Option<f64>,
    /// Part of `dev_kw` no lever could take (kW).
    pub unresolved_kw: f64,
    /// The limit-enforcement pass's result (`None` while it is switched off).
    pub limit: Option<limit::LimitPassOutcome>,
    /// Each storage asset's share of this tick's setpoint that only bridges a lagging
    /// command (kW) — next tick's `ArbiterTick::prev_bridge_kw_by_asset`, with the limit
    /// pass's.
    pub bridge_kw_by_asset: HashMap<String, f64>,
    /// The opportunistic pass's result (`None` under every planning objective). Kept apart
    /// from the deviation fields above so nothing the deviation pass, its decision trace or
    /// the residual tracker reads is ever written by it (openspec `opportunistic-objective` D0).
    pub opportunistic: Option<opportunistic::OpportunisticOutcome>,
}

impl ArbiterOutcome {
    /// What this tick adds to the residual accumulator (§5.5), per SoC-coupled
    /// asset: what both passes hold such an asset away from its input, as energy over
    /// `dt_s` seconds — the deviation pass's displacement from plan and the limit pass's
    /// adjustment. Energy, not a per-tick kW delta: a correction that is held, not
    /// moving, must keep counting, or the backstop never hears it (R-88). Which assets
    /// have a state of charge to protect is theirs to say: those whose snapshot carries
    /// one (`AssetHistoryView::soc_frac`, battery and EV), never an id list (R-128).
    pub fn residual_kwh_by_asset(&self, dt_s: f64, sim: &SimSnapshot) -> HashMap<String, f64> {
        let dt_h = crate::entities::units::dt_h_from_s(dt_s);
        let mut residual_kwh: HashMap<String, f64> = HashMap::new();
        let limit_adjustments = self.limit.iter().flat_map(|l| &l.adjusted_kw_by_asset);
        for (asset_id, adjusted_kw) in self.displaced_kw_by_asset.iter().chain(limit_adjustments) {
            let has_soc = sim
                .assets
                .get(asset_id)
                .is_some_and(|a| a.has_state_of_charge());
            if has_soc {
                *residual_kwh.entry(asset_id.clone()).or_insert(0.0) +=
                    crate::entities::units::energy_kwh(adjusted_kw.abs(), dt_h);
            }
        }
        residual_kwh
    }
}

/// This tick's inputs, shared by both arbiter passes.
#[derive(Clone, Copy)]
pub struct ArbiterTick<'a> {
    pub sim: &'a SimSnapshot,
    /// `None` in the no-plan-yet startup window.
    pub plan_slot: Option<&'a PlanTimeSlot>,
    pub objective: PlannerObjective,
    /// Each EV's opportunistic regime (`ev_overlays`).
    pub ev_overlay: &'a EvOverlays,
    /// This tick's previewed power of every physics-driven asset (PV arrays, base loads), by
    /// asset id (`SimState::peek_pv` / `peek_base_load_kw`): what the projection counts for
    /// them instead of their necessarily-stale snapshot.
    pub live_kw: &'a LiveKwByAsset,
    /// This tick's previews again, with each array under the generation limit that would apply
    /// **without** the arbiter's own tightening — the PV a released correction would face.
    /// `live_kw` already includes last tick's arbiter curtailment (it clamps by the inverter's
    /// current limit), so judging "is the cause gone" on it would release whenever the
    /// arbiter's own curtailment happened to hide the deviation (R-88). `None`: use `live_kw`.
    pub live_released_kw: Option<&'a LiveKwByAsset>,
    /// An alert window is active — the only case that may curtail the heater's
    /// thermostat-forced emergency heat.
    pub alert_active: bool,
    /// The import ceiling both passes steer to (`limit::limit_target_kw`);
    /// `None` = no hard import limit in force.
    pub limit_target_kw: Option<f64>,
    /// The part of each storage asset's last command that only bridged a lagging lever's
    /// command (kW, setpoint sign; `bridge_kw_by_asset` of both passes last tick). That
    /// command has landed since, so the bridge is no correction to carry: carried, it read
    /// as a deviation of its own, and the arbiter undid the EV's now-landed change to remove
    /// it — the EV/battery hunt of R-104.
    pub prev_bridge_kw_by_asset: &'a HashMap<String, f64>,
}

/// `projected_net_kw − target`, where the target is the plan's signed net
/// power, capped at `limit_target_kw` when a hard import limit is in force —
/// the one target both passes share, so deviation correction never steers
/// above a limit the limit pass would then have to push back down. Positive =
/// importing more than wanted (need an import-reducing lever); negative =
/// exporting more than planned / surplus (need an export-absorbing lever).
pub fn deviation_kw(
    plan_slot: &PlanTimeSlot,
    projected_net_kw: f64,
    limit_target_kw: Option<f64>,
) -> f64 {
    let plan_signed_net_kw = plan_slot.net_import_kw - plan_slot.net_export_kw;
    let target_kw = limit_target_kw.map_or(plan_signed_net_kw, |t| plan_signed_net_kw.min(t));
    projected_net_kw - target_kw
}

/// Top-level entry point, called once per tick from
/// `tasks::sim_tick::helpers::build_tick_setpoints` in place of the former
/// direct call to `apply_surplus_ev_overlay`.
pub fn reconcile(
    tick: &ArbiterTick,
    base_setpoints: &HashMap<String, f64>,
    incumbent_lever: Option<&str>,
) -> ArbiterOutcome {
    let sim = tick.sim;
    let mut setpoints = base_setpoints.clone();
    // Carry forward the dead-beat correctors' own last-applied setpoint as
    // the baseline, not the plan's static per-slot allocation — otherwise a
    // tick where the corresponding lever doesn't fire (deviation within dead
    // band, or a cheaper lever absorbed it) would silently revert the
    // correction, immediately re-creating the very deviation it just
    // resolved whenever the underlying disturbance is persistent rather than
    // transient. Mirrors `projected_net_kw`'s use of `snap.setpoint_kw` for
    // the same assets, above.
    for id in release::carried_ids(sim) {
        setpoints.insert(id.to_string(), sim.assets[id].setpoint_kw);
    }
    lag_bridge::drop_last_bridge(&mut setpoints, tick.prev_bridge_kw_by_asset);

    let Some(slot) = tick.plan_slot else {
        // No active plan yet (startup window): same fallback as the
        // pre-arbiter no-plan branch — opportunistic EV-only, since there's
        // no plan target to compute a deviation against.
        apply_ev_lever_opportunistic(&mut setpoints, sim, tick.live_kw, tick.ev_overlay);
        return ArbiterOutcome {
            setpoints,
            ..Default::default()
        };
    };

    let net_kw = projected_net_kw(sim, &setpoints, tick.live_kw);
    let dev_kw = deviation_kw(slot, net_kw, tick.limit_target_kw);
    let without_kw =
        release::deviation_without_correction_kw(tick, slot, &setpoints, base_setpoints);
    let quiet = ArbiterOutcome {
        net_kw: Some(net_kw),
        dev_kw: Some(dev_kw),
        dev_without_correction_kw: Some(without_kw),
        ..Default::default()
    };

    // R-88: the cause is gone — release the whole correction to plan in one tick, the
    // one true "cleared", instead of undoing it step by step.
    if without_kw.abs() < DEAD_BAND_KW {
        let setpoints = release::at_plan(&setpoints, base_setpoints, sim);
        return ArbiterOutcome { setpoints, ..quiet };
    }
    // Settled but still held: nothing to move this tick, yet the levers are off-plan, so the
    // correction is active (no false "cleared") and keeps counting toward the backstop.
    if dev_kw.abs() < DEAD_BAND_KW {
        let displaced_kw_by_asset =
            release::displaced_from_plan_kw(&setpoints, base_setpoints, sim);
        let active_lever = release::holding_lever(&displaced_kw_by_asset, sim);
        return ArbiterOutcome {
            setpoints,
            displaced_kw_by_asset,
            active_lever,
            ..quiet
        };
    }

    let inputs = LeverInputs {
        tick,
        incumbent_lever,
        bounds_kw: &SetpointBoundsKw::new(),
    };
    let applied = apply_ranked_levers(&mut setpoints, &DEVIATION_POLICY, &inputs, dev_kw);
    let displaced_kw_by_asset = release::displaced_from_plan_kw(&setpoints, base_setpoints, sim);

    ArbiterOutcome {
        setpoints,
        heater_emergency_mode: applied.heater_emergency_mode,
        pv_generation_limit_tighten_kw: applied.pv_generation_limit_tighten_kw,
        // A lever that acted leads; otherwise a carried lever still off-plan holds the
        // correction (it may have no capacity left this tick) — still active, not cleared.
        active_lever: applied
            .active_lever
            .or_else(|| release::holding_lever(&displaced_kw_by_asset, sim)),
        displaced_kw_by_asset,
        net_kw: Some(net_kw),
        dev_kw: Some(dev_kw),
        unresolved_kw: applied.unresolved_kw,
        dev_without_correction_kw: Some(without_kw),
        limit: None,
        bridge_kw_by_asset: applied.bridge_kw_by_asset,
        opportunistic: None,
    }
}

/// Overlays and bridges the arbiter tests hand a tick: built once, so a tick built by a
/// helper function can borrow them for `'static`.
#[cfg(test)]
pub(crate) mod test_ticks {
    use super::{EvOverlay, EvOverlays};
    use std::collections::HashMap;
    use std::sync::LazyLock;

    static EV_ON: LazyLock<EvOverlays> = LazyLock::new(|| {
        let on = EvOverlay {
            plan_allocates: false,
            enabled: true,
        };
        HashMap::from([("ev".to_string(), on)])
    });
    static EV_PLAN_ALLOCATED: LazyLock<EvOverlays> = LazyLock::new(|| {
        let allocated = EvOverlay {
            plan_allocates: true,
            enabled: false,
        };
        HashMap::from([("ev".to_string(), allocated)])
    });
    static NO_BRIDGE: LazyLock<HashMap<String, f64>> = LazyLock::new(HashMap::new);

    /// The EV `ev` with opportunistic charging on and nothing allocated by the plan.
    pub(crate) fn ev_on() -> &'static EvOverlays {
        &EV_ON
    }

    /// The EV `ev` charging as the plan allocates it, overlay off.
    pub(crate) fn ev_plan_allocated() -> &'static EvOverlays {
        &EV_PLAN_ALLOCATED
    }

    pub(crate) fn no_bridge() -> &'static HashMap<String, f64> {
        &NO_BRIDGE
    }

    /// This tick's previews of the PV array `pv` and the base load `base_load`, those given.
    pub(crate) fn live(pv_kw: Option<f64>, base_load_kw: Option<f64>) -> super::LiveKwByAsset {
        [("pv", pv_kw), ("base_load", base_load_kw)]
            .into_iter()
            .filter_map(|(id, kw)| Some((id.to_string(), kw?)))
            .collect()
    }

    /// `live`, for a tick a helper function returns.
    pub(crate) fn live_static(
        pv_kw: Option<f64>,
        base_load_kw: Option<f64>,
    ) -> &'static super::LiveKwByAsset {
        Box::leak(Box::new(live(pv_kw, base_load_kw)))
    }
}

#[cfg(test)]
#[path = "tests/arbiter_tests.rs"]
mod arbiter_tests;
