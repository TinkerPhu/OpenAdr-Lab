//! The opportunistic pass: dispatch without look-ahead (openspec `opportunistic-objective`
//! D2), the common non-planning HEMS.
//!
//! Decides, each tick, what every asset with a surplus-claim rank draws: the site's balance
//! without those assets is offered to them in rank order, lowest first. While it is a
//! surplus, each takes what it can toward its own target; once it is an import, a storage
//! asset covers it from what it holds and every other asset draws only what it must (its own
//! command, e.g. a thermostat's forced heat). No asset moves beyond the balance, so storage
//! never charges from the grid or discharges into export.
//!
//! An open-loop allocation, like `arbiter_levers::apply_ev_lever_opportunistic` (which it
//! generalizes to every ranked asset): each asset is commanded what it should draw once its
//! command lands, from this tick's balance, so a charger's response lag neither drops its
//! command nor winds anything up. Not the closed-loop lever machinery (`apply_ranked_levers`),
//! which corrects by what an asset already draws this tick. Reads only what every asset reports
//! on its snapshot (capability, response, available energy, rank, lever kind), never a kind.
//!
//! Runs only under the opportunistic objective, starts from the base setpoints every tick and
//! writes none of the deviation pass's state (D0). A planning objective never reaches it.

use std::collections::{BTreeSet, HashMap};

use super::{projected_net_kw_except, ArbiterOutcome, ArbiterTick, DEAD_BAND_KW};
use crate::controller::sim_snapshot::AssetSnapshot;
use crate::entities::asset::ArbiterLeverKind;
use crate::entities::design_vocabulary::NonPlanningDispatch;
use crate::entities::device_session::{EvSession, HeaterTarget, ShiftableLoad};

/// What one opportunistic tick did (diagnostics, `GET /arbiter-diagnostics`).
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, ts_rs::TS)]
pub struct OpportunisticOutcome {
    /// The site's balance before the ranked assets (kW, import positive): base load, PV,
    /// start-now requests and every unranked or excluded asset.
    pub site_kw: f64,
    /// The first asset that took surplus or covered import this tick, if any.
    pub leading_asset_id: Option<String>,
    /// Whether that was a surplus (`true`) or an import (`false`).
    pub absorbing: bool,
    /// What no asset could take (kW, import positive): exported surplus or uncovered import.
    pub unresolved_kw: f64,
    /// Assets holding a BY_DEADLINE request that wait for their latest start, taking surplus
    /// meanwhile (`request_regime`).
    pub waiting: Vec<WaitingAsset>,
}

/// An asset waiting for the latest start of its BY_DEADLINE request.
#[derive(Debug, Clone, PartialEq, serde::Serialize, ts_rs::TS)]
pub struct WaitingAsset {
    pub asset_id: String,
    /// When it starts at full effort unless surplus has shortened its time to target by then.
    pub start_by: chrono::DateTime<chrono::Utc>,
}

/// How long before the latest possible start a BY_DEADLINE request starts anyway (15 minutes).
/// Additive, not proportional: once the asset runs at full effort its time to target falls
/// by exactly what the clock advances, so the slack stays put and the request stays started
/// with nothing stored (openspec `opportunistic-deadline-dispatch` D3).
pub const DEADLINE_MARGIN_S: f64 = 900.0;

/// The active requests under the opportunistic objective, per asset: how each is served
/// (`UserRequestMode::non_planning_dispatch`) and the targets the assets must know to say
/// when they are done (D4). Read only in the opportunistic arm of the tick.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct OpportunisticRequests {
    pub dispatch_by_asset: HashMap<String, NonPlanningDispatch>,
    pub targets: RequestTargets,
}

/// Request targets handed to the simulator per asset (`TickInputs::request_targets`): each
/// EV's session target SoC, and each heater's start-now target as its cut-out ceiling (the
/// heater caps it at its own `temp_max_c`). The sim's comfort target wins over a request's.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RequestTargets {
    pub ev_soc_target_frac_by_asset: HashMap<String, f64>,
    pub heater_ceiling_c_by_asset: HashMap<String, f64>,
}

/// Collect the requests that are active at `now`: each EV's current session, each heater's
/// target, each shiftable load inside its window.
pub fn opportunistic_requests(
    now: chrono::DateTime<chrono::Utc>,
    ev_sessions_now: &HashMap<String, EvSession>,
    heater_targets: &std::collections::BTreeMap<String, HeaterTarget>,
    shiftable_loads: &[ShiftableLoad],
) -> OpportunisticRequests {
    let mut out = OpportunisticRequests::default();
    for (id, session) in ev_sessions_now {
        out.dispatch_by_asset.insert(
            id.clone(),
            session.mode.non_planning_dispatch(session.departure_time),
        );
        out.targets
            .ev_soc_target_frac_by_asset
            .insert(id.clone(), session.target_soc_frac);
    }
    for (id, target) in heater_targets {
        let dispatch = target.mode.non_planning_dispatch(target.ready_by);
        out.dispatch_by_asset.insert(id.clone(), dispatch);
        if dispatch != NonPlanningDispatch::SurplusOnly {
            out.targets
                .heater_ceiling_c_by_asset
                .insert(id.clone(), target.target_temp_c);
        }
    }
    for load in shiftable_loads
        .iter()
        .filter(|l| l.earliest_start <= now && now < l.latest_end)
    {
        out.dispatch_by_asset.insert(
            load.asset_id.clone(),
            load.mode.non_planning_dispatch(load.latest_end),
        );
    }
    out
}

/// What the requests (and the assets' own declarations) make of this tick: the base commands
/// the pass starts from, the assets it leaves alone, and the ones still waiting to start.
pub struct RequestRegime {
    pub base: HashMap<String, f64>,
    pub excluded: BTreeSet<String>,
    pub waiting: Vec<WaitingAsset>,
}

/// How the active requests shape this tick (openspec `opportunistic-objective` D4 and
/// `opportunistic-deadline-dispatch` D3, D4).
///
/// A start-now request runs its asset at full effort (`cap_max_import_kw`, which the asset
/// already reports as zero once the request's target, injected into the asset, is reached) and
/// takes no surplus on top. A start-by request does the same from its latest start, the
/// deadline less the asset's own time to target and `DEADLINE_MARGIN_S`; until then it takes
/// part in the pass like a surplus-only request, and an asset that declares no time to target
/// starts at once. A surplus-only request makes its asset take surplus whatever its surplus
/// switch says. An asset with no request is served as it declares
/// (`AssetSnapshot::dispatch_without_request`, a plugged-in EV below target: start now), unless
/// it is a surplus-charging asset (an EV, by its declared lever kind) whose own switch is off
/// (`ev_surplus_switch_on`: its opportunistic-charging setting, `PUT /ev-settings`): it then
/// takes no part.
pub fn request_regime(
    sim: &crate::controller::SimSnapshot,
    mut base: HashMap<String, f64>,
    requests: &HashMap<String, NonPlanningDispatch>,
    ev_surplus_switch_on: impl Fn(&str) -> bool,
    now: chrono::DateTime<chrono::Utc>,
) -> RequestRegime {
    let mut excluded = BTreeSet::new();
    let mut waiting = Vec::new();
    for (id, snap) in sim.assets_in_roster_order() {
        let dispatch = match requests.get(id) {
            Some(dispatch) => Some(*dispatch),
            None if snap
                .arbiter_lever
                .is_some_and(|l| l.kind == ArbiterLeverKind::SurplusCharge)
                && !ev_surplus_switch_on(id) =>
            {
                excluded.insert(id.to_string());
                continue;
            }
            None => snap.dispatch_without_request,
        };
        let starts_now = match dispatch {
            Some(NonPlanningDispatch::StartNow) => true,
            Some(NonPlanningDispatch::StartBy(deadline)) => match snap.full_effort_time_to_target_s
            {
                Some(time_to_target_s) => {
                    let lead_s = (time_to_target_s + DEADLINE_MARGIN_S).ceil();
                    let start_by = deadline - chrono::Duration::seconds(lead_s as i64);
                    let start = now >= start_by;
                    if !start {
                        waiting.push(WaitingAsset {
                            asset_id: id.to_string(),
                            start_by,
                        });
                    }
                    start
                }
                None => true,
            },
            Some(NonPlanningDispatch::SurplusOnly) | None => false,
        };
        if starts_now {
            base.insert(id.to_string(), snap.cap_max_import_kw);
            excluded.insert(id.to_string());
        }
    }
    RequestRegime {
        base,
        excluded,
        waiting,
    }
}

/// One tick of dispatch without look-ahead. `base` is each asset's own command before the
/// pass (its default, a thermostat's own setpoint, a start-now request's full effort);
/// `excluded_ids` take no part this tick and keep their base command (a start-now request
/// already runs at full effort, an EV whose surplus switch is off and that has no request).
///
/// The result goes to `ArbiterOutcome::opportunistic` only, never to `active_lever` or
/// `displaced_kw_by_asset`: nothing the deviation pass or the residual tracker reads.
/// The opportunistic objective's whole decision for one step, live and in the forecast (D6):
/// requests set the regime (`request_regime`), the pass shares the site balance out in
/// declared rank order (`opportunistic_pass`), and the shared stages bound the result
/// (`dispatch_stages`). `defaults` is each asset's setpoint without a plan; `ev_surplus_switch_on`
/// is each EV's opportunistic toggle.
pub fn opportunistic_step(
    tick: &ArbiterTick,
    defaults: HashMap<String, f64>,
    requests: &OpportunisticRequests,
    ev_surplus_switch_on: impl Fn(&str) -> bool,
    now: chrono::DateTime<chrono::Utc>,
    stages: &super::DispatchStages,
) -> ArbiterOutcome {
    let regime = request_regime(
        tick.sim,
        defaults,
        &requests.dispatch_by_asset,
        ev_surplus_switch_on,
        now,
    );
    let mut outcome = opportunistic_pass(tick, &regime.base, &regime.excluded);
    if let Some(pass) = outcome.opportunistic.as_mut() {
        pass.waiting = regime.waiting;
    }
    super::dispatch_stages(outcome, tick, now, stages)
}

pub fn opportunistic_pass(
    tick: &ArbiterTick,
    base: &HashMap<String, f64>,
    excluded_ids: &BTreeSet<String>,
) -> ArbiterOutcome {
    let sim = tick.sim;
    let mut ranked: Vec<(&str, &AssetSnapshot)> = sim
        .assets_in_roster_order()
        .into_iter()
        .filter(|(id, a)| a.surplus_claim_rank.is_some() && !excluded_ids.contains(*id))
        .collect();
    // Stable: equal ranks keep roster order.
    ranked.sort_by_key(|(_, a)| a.surplus_claim_rank);
    let ranked_ids: Vec<&str> = ranked.iter().map(|(id, _)| *id).collect();

    let mut setpoints = base.clone();
    let site_kw = projected_net_kw_except(sim, &setpoints, tick.live_kw, &ranked_ids);
    let mut balance_kw = site_kw;
    let mut result = OpportunisticOutcome {
        site_kw,
        ..Default::default()
    };
    for (id, snap) in ranked {
        let base_kw = base
            .get(id)
            .copied()
            .unwrap_or_else(|| snap.default_setpoint_kw.unwrap_or(snap.power_kw));
        let (setpoint_kw, drawn_kw) = allocate(snap, base_kw, balance_kw);
        let moved = (drawn_kw - snap.power_when_command_lands_kw(base_kw)).abs() > 1e-9;
        if moved && result.leading_asset_id.is_none() {
            result.leading_asset_id = Some(id.to_string());
            result.absorbing = balance_kw < 0.0;
        }
        setpoints.insert(id.to_string(), setpoint_kw);
        balance_kw += drawn_kw;
    }
    result.unresolved_kw = if balance_kw.abs() < DEAD_BAND_KW {
        0.0
    } else {
        balance_kw
    };
    ArbiterOutcome {
        setpoints,
        opportunistic: Some(result),
        ..Default::default()
    }
}

/// One ranked asset's command and what it will draw once that lands, given the balance the
/// lower-ranked assets left (`balance_kw`, import positive). A surplus is taken up to what the
/// asset can absorb without overshooting it (a stage that would import is not taken); an
/// import is covered by storage down to its floor; otherwise the asset draws only what its
/// base command makes it draw.
fn allocate(snap: &AssetSnapshot, base_kw: f64, balance_kw: f64) -> (f64, f64) {
    let base_draw_kw = snap.power_when_command_lands_kw(base_kw);
    if balance_kw <= -DEAD_BAND_KW && snap.available_charge_kwh.is_none_or(|kwh| kwh > 0.0) {
        let want_kw = (-balance_kw).min(snap.cap_max_import_kw);
        let setpoint_kw = snap.setpoint_for_power_at_or_below_kw(want_kw);
        let drawn_kw = snap.power_when_command_lands_kw(setpoint_kw);
        if drawn_kw > base_draw_kw {
            return (setpoint_kw, drawn_kw);
        }
    }
    let is_storage = snap
        .arbiter_lever
        .is_some_and(|l| l.kind == ArbiterLeverKind::Storage);
    let has_energy = snap.available_discharge_kwh.is_none_or(|kwh| kwh > 0.0);
    if balance_kw + base_draw_kw >= DEAD_BAND_KW && is_storage && has_energy {
        // The import this asset sees includes its own base draw; cover it down to the floor.
        let import_kw = balance_kw + base_draw_kw;
        let setpoint_kw = (-import_kw).max(-snap.cap_max_export_kw.abs());
        return (setpoint_kw, snap.power_when_command_lands_kw(setpoint_kw));
    }
    (base_kw, base_draw_kw)
}

#[cfg(test)]
#[path = "tests/opportunistic_tests.rs"]
mod opportunistic_tests;
