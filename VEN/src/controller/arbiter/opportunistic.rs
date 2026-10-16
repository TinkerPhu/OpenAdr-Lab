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

/// What one opportunistic tick did (diagnostics, `GET /arbiter-diagnostics`).
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize)]
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
}

/// How the active requests shape this tick (openspec `opportunistic-objective` D4): the base
/// commands the pass starts from, and the assets it leaves alone.
///
/// A start-now request runs its asset at full effort (`cap_max_import_kw`, which the asset
/// already reports as zero once the request's target, injected into the asset, is reached) and
/// takes no surplus on top. A surplus-only request makes its asset take surplus whatever its
/// surplus switch says. An asset with no request takes surplus only if its own switch is on
/// (`surplus_switch_on`: an EV's opportunistic-charging setting; true for every other kind).
pub fn request_regime(
    sim: &crate::controller::SimSnapshot,
    mut base: HashMap<String, f64>,
    requests: &HashMap<String, NonPlanningDispatch>,
    surplus_switch_on: impl Fn(&str) -> bool,
) -> (HashMap<String, f64>, BTreeSet<String>) {
    let mut excluded = BTreeSet::new();
    for (id, snap) in &sim.assets {
        match requests.get(id) {
            Some(NonPlanningDispatch::StartNow) => {
                base.insert(id.clone(), snap.cap_max_import_kw);
                excluded.insert(id.clone());
            }
            Some(NonPlanningDispatch::SurplusOnly) => {}
            None if !surplus_switch_on(id) => {
                excluded.insert(id.clone());
            }
            None => {}
        }
    }
    (base, excluded)
}

/// One tick of dispatch without look-ahead. `base` is each asset's own command before the
/// pass (its default, a thermostat's own setpoint, a start-now request's full effort);
/// `excluded_ids` take no part this tick and keep their base command (a start-now request
/// already runs at full effort, an EV whose surplus switch is off and that has no request).
///
/// The result goes to `ArbiterOutcome::opportunistic` only, never to `active_lever` or
/// `displaced_kw_by_asset`: nothing the deviation pass or the residual tracker reads.
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
        let base_kw = base.get(id).copied().unwrap_or(snap.default_setpoint_kw);
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
