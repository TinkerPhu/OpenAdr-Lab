//! Per-lever capacity/cost queries and `apply_*` functions for
//! `controller::arbiter` — split out to keep `arbiter.rs` under the file-size
//! cap. See that module's doc comment for the overall design.
//!
//! Every lever belongs to the asset that declared it (`AssetSnapshot::arbiter_lever`), so each
//! function here takes that asset's id; none looks an asset up by a fixed id (W5a).

use std::collections::HashMap;

use super::{
    EvOverlay, DEAD_BAND_KW, HEATER_COMFORT_OVERRIDE_EUR_PER_KWH,
    LEVER_PREEMPTION_MARGIN_EUR_PER_KWH,
};
use crate::controller::SimSnapshot;
use crate::entities::asset::{ArbiterLeverKind, EmergencyWhatIfs};
use crate::entities::plan::PlanTimeSlot;
use crate::entities::planner_params::PlannerObjective;

/// Lever kinds: a namespace of their own (a kind names a *lever*, not an asset or an asset
/// type - the battery lever happens to share a spelling with the default battery's id,
/// nothing more). One spelling each, used by the constructors here and by every place that
/// matches a lever.
pub(crate) const LEVER_BATTERY: &str = "battery";
pub(crate) const LEVER_EV: &str = "ev";
pub(crate) const LEVER_HEATER_PAUSE: &str = "heater_pause";
pub(crate) const LEVER_HEATER_EMERGENCY: &str = "heater_emergency";
pub(crate) const LEVER_PV_CURTAIL: &str = "pv_curtail";

/// The lever kind an asset's declaration offers.
pub(crate) fn lever_kind_of(kind: ArbiterLeverKind) -> &'static str {
    match kind {
        ArbiterLeverKind::Storage => LEVER_BATTERY,
        ArbiterLeverKind::SurplusCharge => LEVER_EV,
        ArbiterLeverKind::PausableLoad => LEVER_HEATER_PAUSE,
    }
}

/// A lever: its kind and the asset it acts on (`None` for a site lever, PV curtailment).
/// Reported as `<kind>:<asset_id>`, or `<kind>` for a site lever - the kind first, so the
/// diagnostics text stays greppable from `LEVER_*`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LeverId {
    pub(crate) kind: &'static str,
    pub(crate) asset_id: Option<String>,
}

impl LeverId {
    pub(crate) fn of_asset(kind: &'static str, asset_id: &str) -> Self {
        Self {
            kind,
            asset_id: Some(asset_id.to_string()),
        }
    }

    pub(crate) fn site(kind: &'static str) -> Self {
        Self {
            kind,
            asset_id: None,
        }
    }

    /// The reported form, also what a pass is handed back as its incumbent.
    pub(crate) fn key(&self) -> String {
        match &self.asset_id {
            Some(asset_id) => format!("{}:{asset_id}", self.kind),
            None => self.kind.to_string(),
        }
    }
}

/// The lever kind of a reported lever (`<kind>:<asset_id>` or `<kind>`).
pub(crate) fn kind_of_key(key: &str) -> &str {
    key.split(':').next().unwrap_or(key)
}

#[derive(Debug, Clone)]
pub(super) struct Lever {
    pub(super) id: LeverId,
    pub(super) available_capacity_kw: f64,
    pub(super) marginal_cost_eur_per_kwh: f64,
}

/// The setpoint this tick's adjustments start from: the value already in the
/// setpoint map, else the asset's last applied command. `reconcile` seeds the
/// map with the last applied command for the carried levers (their dead-beat
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

/// A storage asset's setpoint range: its own capability (it already reports 0 in
/// a direction it can't sustain near full/empty — no SoC re-interpretation
/// here), narrowed by a pass's `bounds_kw` (the comms-loss clamp).
fn storage_setpoint_range_kw(
    snap: &crate::controller::sim_snapshot::AssetSnapshot,
    bounds_kw: Option<(f64, f64)>,
) -> (f64, f64) {
    let (min_kw, max_kw) = (-snap.cap_max_export_kw.abs(), snap.cap_max_import_kw);
    match bounds_kw {
        Some((lo_kw, hi_kw)) => (min_kw.max(lo_kw).min(0.0), max_kw.min(hi_kw).max(0.0)),
        None => (min_kw, max_kw),
    }
}

/// Storage lever capacity + cost. Direction-dependent: absorbing an import
/// deviation (`deviation_kw > 0`) needs headroom to discharge more (or charge
/// less); absorbing a surplus (`deviation_kw < 0`) needs headroom to charge
/// more (or discharge less). Without a plan slot (startup window) the cost is
/// 0 — ranking then falls back to roster order.
pub(super) fn battery_lever(
    setpoints: &HashMap<String, f64>,
    sim: &SimSnapshot,
    asset_id: &str,
    slot: Option<&PlanTimeSlot>,
    deviation_kw: f64,
    bounds_kw: Option<(f64, f64)>,
) -> Option<Lever> {
    let snap = sim.assets.get(asset_id)?;
    let current_sp = current_setpoint_kw(setpoints, sim, asset_id);
    let (min_kw, max_kw) = storage_setpoint_range_kw(snap, bounds_kw);
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
        id: LeverId::of_asset(LEVER_BATTERY, asset_id),
        available_capacity_kw,
        marginal_cost_eur_per_kwh,
    })
}

/// Dead-beat storage correction, metered against `assigned_kw` (this lever's
/// share of the deviation, from the shared `remaining_kw` pool) rather than
/// the full deviation. Starts from `current_setpoint_kw`: for the deviation pass
/// that is the previously applied setpoint (`reconcile` seeds it), not the plan
/// allocation, to avoid a limit cycle (see §3a's stability re-verification tests).
/// Returns the achieved change (kW, magnitude).
pub(super) fn apply_battery_lever(
    setpoints: &mut HashMap<String, f64>,
    sim: &SimSnapshot,
    asset_id: &str,
    assigned_kw: f64,
    objective: PlannerObjective,
    bounds_kw: Option<(f64, f64)>,
) -> f64 {
    let Some(snap) = sim.assets.get(asset_id) else {
        return 0.0;
    };
    if objective == PlannerObjective::MaxRevenue && assigned_kw > 0.0 {
        return 0.0;
    }
    let (min_kw, max_kw) = storage_setpoint_range_kw(snap, bounds_kw);
    let current_sp = current_setpoint_kw(setpoints, sim, asset_id);

    let raw_target = current_sp - assigned_kw;
    let clamped = raw_target.clamp(min_kw, max_kw);

    let delta = clamped - current_sp;
    if delta.abs() < 1e-6 {
        return 0.0;
    }
    setpoints.insert(asset_id.to_string(), clamped);
    delta.abs()
}

/// EV lever: flat zero cost, only offered when the plan has no allocation for this EV
/// (opportunistic regime — the plan's own EV commitment is never second-guessed) and its
/// own settings allow it. Capacity is direction-dependent: absorbing a surplus can
/// increase charging up to `max_charge_kw`; absorbing an import deviation can
/// only claw back whatever opportunistic charge is already flowing (BL-12's
/// discrete relay floor makes finer-grained reduction physically meaningless).
pub(super) fn ev_lever(
    setpoints: &HashMap<String, f64>,
    sim: &SimSnapshot,
    asset_id: &str,
    deviation_kw: f64,
    overlay: EvOverlay,
) -> Option<Lever> {
    if !overlay.offers_surplus() {
        return None;
    }
    let snap = sim.assets.get(asset_id)?;
    // The EV's own capability is 0 while unplugged or at/above its target.
    let charge_ceiling_kw = snap.cap_max_import_kw;
    if charge_ceiling_kw <= 0.0 {
        return None;
    }
    let current_sp = current_setpoint_kw(setpoints, sim, asset_id).max(0.0);

    let available_capacity_kw = if deviation_kw < 0.0 {
        (charge_ceiling_kw - current_sp).max(0.0)
    } else {
        current_sp
    };
    if available_capacity_kw <= 0.0 {
        return None;
    }
    Some(Lever {
        id: LeverId::of_asset(LEVER_EV, asset_id),
        available_capacity_kw,
        marginal_cost_eur_per_kwh: 0.0,
    })
}

/// What a lever's command changes this tick and once it has landed (kW, magnitudes).
/// They differ only for an asset with a response lag: the charger applies a command a
/// tick late (BL-12's `response_delay_s`, R-82).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(crate) struct LeverEffect {
    pub now_kw: f64,
    pub landed_kw: f64,
}

/// The change achieved **this tick** is zero while the charger is still applying the
/// command it accepted last tick: the change is commanded all the same and lands next
/// tick, and the greedy loop meanwhile passes the excess to the next lever instead of
/// believing an actuator that has not moved yet. `landed_kw` is what the command will
/// have changed then, so the loop knows how much of that pass-on is a one-tick bridge.
pub(super) fn apply_ev_lever(
    setpoints: &mut HashMap<String, f64>,
    sim: &SimSnapshot,
    asset_id: &str,
    assigned_kw: f64,
) -> LeverEffect {
    let Some(snap) = sim.assets.get(asset_id) else {
        return LeverEffect::default();
    };
    let current_sp = current_setpoint_kw(setpoints, sim, asset_id).max(0.0);
    // The charger's own answer for what the reduced command will draw — it
    // cannot sustain a trickle below its minimum charge rate and says so, so
    // no sub-minimum setpoint is ever commanded (which would yield 0 kW
    // physically while corrupting the arbiter's next-tick accounting).
    let new_sp = snap.power_when_command_lands_kw((current_sp - assigned_kw).max(0.0));
    setpoints.insert(asset_id.to_string(), new_sp);
    LeverEffect {
        now_kw: (snap.power_drawn_for_setpoint_kw(current_sp)
            - snap.power_drawn_for_setpoint_kw(new_sp))
        .abs(),
        landed_kw: (snap.power_when_command_lands_kw(current_sp) - new_sp).abs(),
    }
}

/// Pause-within-comfort-band lever: flat zero cost, available
/// whenever the load draws a stage this tick (§5.4 scenario D — "not because
/// a static rule ranked it third but because its marginal cost is genuinely
/// zero whenever available"). Nothing to pause while the thermostat forces the
/// load's power (`forced_power_kw`) — the setpoint is ignored then.
pub(super) fn heater_pause_lever(
    setpoints: &HashMap<String, f64>,
    sim: &SimSnapshot,
    asset_id: &str,
    deviation_kw: f64,
) -> Option<Lever> {
    if deviation_kw < 0.0 {
        return None; // pausing a load can't absorb a surplus
    }
    let snap = sim.assets.get(asset_id)?;
    if snap.forced_power_kw.is_some() {
        return None;
    }
    let drawn_kw = drawn_kw(setpoints, sim, asset_id);
    if drawn_kw <= 0.0 {
        return None;
    }
    Some(Lever {
        id: LeverId::of_asset(LEVER_HEATER_PAUSE, asset_id),
        available_capacity_kw: drawn_kw,
        marginal_cost_eur_per_kwh: 0.0,
    })
}

/// Commands whatever setpoint makes the load draw at most
/// `drawn − assigned` — the asset's own answer, so an in-between value that
/// would round back up to the stage it came from is never sent. Returns the
/// achieved change (kW, magnitude): a whole stage, which may exceed
/// `assigned_kw`.
pub(super) fn apply_heater_pause_lever(
    setpoints: &mut HashMap<String, f64>,
    sim: &SimSnapshot,
    asset_id: &str,
    assigned_kw: f64,
) -> f64 {
    let Some(snap) = sim.assets.get(asset_id) else {
        return 0.0;
    };
    let drawn_kw = drawn_kw(setpoints, sim, asset_id);
    let new_sp = snap.setpoint_for_power_at_or_below_kw((drawn_kw - assigned_kw).max(0.0));
    setpoints.insert(asset_id.to_string(), new_sp);
    drawn_kw - snap.power_drawn_for_setpoint_kw(new_sp)
}

/// What `asset_id` draws for the setpoint currently commanded — its own
/// answer, whatever kind of asset it is (R-81).
pub(super) fn drawn_kw(setpoints: &HashMap<String, f64>, sim: &SimSnapshot, asset_id: &str) -> f64 {
    sim.assets.get(asset_id).map_or(0.0, |snap| {
        snap.power_drawn_for_setpoint_kw(current_setpoint_kw(setpoints, sim, asset_id))
    })
}

/// Emergency-mode lever (`HeaterEmergencyMode::Curtail`/`Absorb`), offered by every asset that
/// answers the emergency what-ifs (`AssetSnapshot::emergency_what_ifs`).
///
/// Curtail (import direction) is offered only during an alert window: the
/// heater's own thermostat is its safety, and nothing short of a grid alert
/// overrides it — not a capacity limit, whose violation penalty inflates the
/// slot's marginal cost just like an alert's would (GB-47). Priced at
/// `HEATER_COMFORT_OVERRIDE_EUR_PER_KWH`, so cheaper levers go first.
///
/// Absorb (surplus direction) is offered when the export marginal cost exceeds
/// `HEATER_COMFORT_OVERRIDE_EUR_PER_KWH` (§5.4 scenario H — routine tariff
/// swings must never invade the safety band). `is_incumbent` (§4a.2):
/// when this asset's mode was already active last tick, that entry threshold is
/// lowered by the preemption margin, making the mode "stickier" to exit than
/// to enter — a marginal cost hovering right at the threshold cannot flip the
/// mode on and off every tick.
pub(super) fn heater_emergency_lever(
    sim: &SimSnapshot,
    asset_id: &str,
    slot: Option<&PlanTimeSlot>,
    deviation_kw: f64,
    is_incumbent: bool,
    alert_active: bool,
) -> Option<Lever> {
    let snap = sim.assets.get(asset_id)?;
    // The heater's own answers (its thermostat rule under Normal/Absorb),
    // not a re-derivation from temperatures here.
    let EmergencyWhatIfs {
        emergency_heat_kw,
        absorb_headroom_kw,
    } = snap.emergency_what_ifs?;
    let threshold = if is_incumbent {
        HEATER_COMFORT_OVERRIDE_EUR_PER_KWH - LEVER_PREEMPTION_MARGIN_EUR_PER_KWH
    } else {
        HEATER_COMFORT_OVERRIDE_EUR_PER_KWH
    };
    let id = LeverId::of_asset(LEVER_HEATER_EMERGENCY, asset_id);

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
            id,
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
            id,
            available_capacity_kw: absorb_headroom_kw,
            marginal_cost_eur_per_kwh: slot.marginal_cost_export_eur_per_kwh,
        })
    }
}

/// PV curtailment: backstop only, export-excess direction, priced at the
/// forgone export tariff — naturally ranks last. A site lever: it tightens the site's
/// generation limit, not one asset's setpoint.
pub(super) fn pv_curtailment_lever(
    slot: Option<&PlanTimeSlot>,
    deviation_kw: f64,
) -> Option<Lever> {
    let slot = slot?;
    if deviation_kw >= 0.0 || slot.pv_used_kw <= 0.0 {
        return None;
    }
    Some(Lever {
        id: LeverId::site(LEVER_PV_CURTAIL),
        available_capacity_kw: slot.pv_used_kw,
        marginal_cost_eur_per_kwh: slot.export_tariff_eur_kwh,
    })
}

/// Opportunistic surplus EV charging: offer whatever generation exceeds every
/// other active load to the EVs, in roster order, each up to what its charger says it can
/// take and only under its own overlay (`ev_overlay`). The one implementation of that
/// decision — the deviation pass's no-plan-yet fallback and the pre-arbiter overlay path
/// (`dispatcher::apply_surplus_ev_overlay`) both call this. Independent of any plan
/// target, since none exists during the startup window before the first plan.
pub(crate) fn apply_ev_lever_opportunistic(
    setpoints: &mut HashMap<String, f64>,
    sim: &SimSnapshot,
    live_pv_kw: Option<f64>,
    live_base_load_kw: Option<f64>,
    ev_overlay: &super::EvOverlays,
) {
    use ArbiterLeverKind::{Storage, SurplusCharge};
    // Only the EVs this overlay may steer leave the projection; one the plan allocates, or
    // whose own settings keep it out, still draws what it draws.
    let evs: Vec<&str> = sim
        .lever_asset_ids(&[SurplusCharge])
        .into_iter()
        .filter(|id| ev_overlay.get(*id).is_some_and(EvOverlay::offers_surplus))
        .collect();
    let storage = sim.lever_asset_ids(&[Storage]);
    let except: Vec<&str> = evs.iter().chain(&storage).copied().collect();
    let net_other_kw =
        super::projected_net_kw_except(sim, setpoints, live_pv_kw, live_base_load_kw, &except);
    let storage_charge_kw: f64 = storage
        .iter()
        .map(|id| setpoints.get(*id).copied().unwrap_or(0.0).max(0.0))
        .sum();
    let mut surplus_kw = (-net_other_kw - storage_charge_kw).max(0.0);
    for ev_id in evs {
        if surplus_kw < DEAD_BAND_KW {
            return;
        }
        // The charger's own answer: 0 while unplugged, at/above its target, or
        // when the surplus is below the rate it can sustain (R-81).
        let charge_kw = sim.assets[ev_id].power_when_command_lands_kw(surplus_kw);
        if charge_kw > 0.0 {
            setpoints.insert(ev_id.to_string(), charge_kw);
            surplus_kw -= charge_kw;
        }
    }
}
