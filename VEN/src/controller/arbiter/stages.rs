//! The stages every tick's setpoints pass through after the pass that proposed them (deviation
//! correction, the opportunistic pass or the plan as it stands): a VTN dispatch window, the
//! comms-loss fail-safe and last the import-limit pass, which also holds a lagging lever where
//! its command lands (`limit::hold_where_commands_land`). One function, `dispatch_stages`, so the
//! live tick and the opportunistic forecast bound a proposal the same way. Reads the tick and
//! the windows it is given; holds no state and reads no clock.

use chrono::{DateTime, Utc};
use std::collections::HashMap;

use super::{limit, tighten_generation, ArbiterOutcome, ArbiterTick};
use crate::controller::SimSnapshot;
use crate::entities::capacity::{AlertWindow, DispatchWindow};

/// What the stages read besides the tick.
#[derive(Clone, Copy)]
pub struct DispatchStages<'a> {
    pub dispatch_windows: &'a [DispatchWindow],
    pub alert_windows: &'a [AlertWindow],
    /// `None` where comms-loss does not apply (the profile opts out, or a forecast).
    pub comms_loss: Option<crate::controller::comms_loss::CommsLossState>,
    /// The lever the limit pass held last tick.
    pub limit_incumbent_lever: Option<&'a str>,
}

/// Bound a pass's proposal: the DISPATCH_SETPOINT override, the comms-loss clamp, and last the
/// limit-enforcement pass, which therefore also bounds a dispatch setpoint above a hard import
/// limit while staying inside the comms-loss bounds. A limit-pass Curtail (alerts only)
/// overrides a deviation-pass Absorb, heater by heater.
pub fn dispatch_stages(
    mut outcome: ArbiterOutcome,
    tick: &ArbiterTick,
    now: DateTime<Utc>,
    stages: &DispatchStages,
) -> ArbiterOutcome {
    let sp = &mut outcome.setpoints;
    apply_dispatch_override(sp, tick, now, stages.dispatch_windows, stages.alert_windows);
    apply_comms_loss_clamp(sp, tick.sim, stages.comms_loss);
    let bounds_kw = comms_loss_setpoint_bounds_kw(tick.sim, stages.comms_loss);
    // A deviation-pass PV curtailment lowers generation, so the limit pass
    // projects PV after it.
    let pv_tighten_kw = outcome.pv_generation_limit_tighten_kw.unwrap_or(0.0);
    let tightened = tighten_generation(tick.live_kw, tick.sim, pv_tighten_kw);
    let limit_tick = ArbiterTick {
        live_kw: &tightened,
        ..*tick
    };
    outcome.limit = limit::enforce_import_limit(
        &limit_tick,
        &mut outcome.setpoints,
        stages.limit_incumbent_lever,
        &bounds_kw,
    );
    // A lagging lever's command lands a tick late: held where it lands, a cut charger is not
    // released into the limit a tick later (R-151), whichever pass proposed it.
    limit::hold_where_commands_land(tick, &mut outcome.setpoints, outcome.limit.as_mut());
    let limit_modes = outcome.limit.iter().flat_map(|l| &l.heater_emergency_mode);
    let limit_modes: Vec<(String, (bool, bool))> =
        limit_modes.map(|(id, m)| (id.clone(), *m)).collect();
    outcome.heater_emergency_mode.extend(limit_modes);
    outcome
}

/// While a DISPATCH_SETPOINT window is active, steer the storage levers so net site
/// power hits the commanded setpoint (`arbiter::meet_dispatch_setpoint`), overriding the
/// plan's storage allocation (other assets keep their planned setpoints; the plan keeps
/// running underneath and resumes when the window ends).
/// Precedence (recorded decision): an active alert window wins - safety over
/// instruction - so the override is skipped entirely while one is active.
fn apply_dispatch_override(
    sp: &mut HashMap<String, f64>,
    tick: &crate::controller::arbiter::ArbiterTick,
    now: DateTime<Utc>,
    dispatch_windows: &[crate::entities::capacity::DispatchWindow],
    alert_windows: &[crate::entities::capacity::AlertWindow],
) {
    use lab_core::time_window::{any_covering, covering};
    if any_covering(alert_windows, now) {
        return;
    }
    if let Some(win) = covering(dispatch_windows, now) {
        crate::controller::arbiter::meet_dispatch_setpoint(tick, sp, win.setpoint_kw);
    }
}

/// R-59 fail-safe: once VTN comms-loss has been confirmed for the profile's
/// configured debounce window, cap every controllable asset's setpoint to
/// `max_power_pct` of its own ceiling — symmetric for the battery (both
/// charge and discharge capped, no special-casing), one generic knob for all
/// assets rather than one fail-safe per asset type. Runs last in the tick
/// pipeline (after `apply_dispatch_override`) because a comms-loss fail-safe
/// outranks a VTN-instructed dispatch window: if the VTN can't be reached,
/// any window it set is stale/unverifiable. No-op when `comms_loss` is `None`
/// (profile opt-out) or not yet `active` (debounce not elapsed).
fn apply_comms_loss_clamp(
    sp: &mut HashMap<String, f64>,
    sim_snap: &SimSnapshot,
    comms_loss: Option<crate::controller::comms_loss::CommsLossState>,
) {
    for (asset_id, (min_kw, max_kw)) in comms_loss_setpoint_bounds_kw(sim_snap, comms_loss) {
        if let Some(sp_kw) = sp.get_mut(&asset_id) {
            *sp_kw = sp_kw.clamp(min_kw, max_kw);
        }
    }
}

/// The comms-loss clamp's per-asset `(min, max)` setpoint bounds — applied by
/// `apply_comms_loss_clamp` and handed to the arbiter's limit pass, which runs
/// after it and must stay inside them. Empty while comms-loss is not active.
fn comms_loss_setpoint_bounds_kw(
    sim_snap: &SimSnapshot,
    comms_loss: Option<crate::controller::comms_loss::CommsLossState>,
) -> crate::controller::arbiter::SetpointBoundsKw {
    let Some(cl) = comms_loss.filter(|c| c.active) else {
        return Default::default();
    };
    let pct = cl.max_power_pct;
    // Every asset that offers a lever is controllable; each is capped at `pct` of its own
    // rated range, both ways (positive = charge/import, negative = discharge/export).
    sim_snap
        .assets
        .iter()
        .filter_map(|(asset_id, snap)| {
            let lever = snap.arbiter_lever?;
            let bounds_kw = (-(pct * lever.rated_export_kw), pct * lever.rated_import_kw);
            Some((asset_id.clone(), bounds_kw))
        })
        .collect()
}

#[cfg(test)]
mod dispatch_override_tests {
    use super::*;
    use crate::controller::sim_snapshot::{AssetSnapshot, GridSnapshot};
    use crate::entities::asset::{ArbiterLever, ArbiterLeverKind, SetpointResponse};
    use crate::entities::capacity::{AlertWindow, DispatchWindow};
    use chrono::TimeZone;

    fn ts(secs: i64) -> DateTime<Utc> {
        Utc.timestamp_opt(1_700_000_000 + secs, 0).unwrap()
    }

    fn snap_asset(power_kw: f64, imp: f64, exp: f64) -> AssetSnapshot {
        AssetSnapshot {
            power_kw,
            asset_type: crate::entities::asset::AssetKind::Battery,
            cap_max_import_kw: imp,
            cap_max_export_kw: exp,
            available_discharge_kwh: None,
            available_charge_kwh: None,
            forced_power_kw: None,
            response: SetpointResponse::continuous(),
            default_setpoint_kw: power_kw,
            setpoint_kw: power_kw,
            values: std::collections::HashMap::new(),
            history: Default::default(),
            embodied_co2_g_kwh: None,
            emergency_what_ifs: None,
            ac_ceiling_kw: None,
            arbiter_lever: None,
            surplus_claim_rank: None,
            full_effort_time_to_target_s: None,
            dispatch_without_request: None,
            unserved_energy_kwh: None,
            stored_energy_kwh: None,
            roster_index: 0,
        }
    }

    /// The real inverter: its power never follows the setpoint map, which is
    /// what makes the `f64::MAX` "no generation limit" entry harmless.
    fn pv_snap(last_power_kw: f64) -> AssetSnapshot {
        use crate::assets::pv::PvInverter;
        let params = crate::entities::asset_params::PvParams {
            id: crate::ids::ASSET_PV.to_string(),
            rated_kw: 10.0,
            inverter_max_kw: 10.0,
            co2_g_kwh: 41.0,
        };
        let pv = PvInverter::from_params(&params);
        let mut state = PvInverter::initial_state(&params);
        state.actual_power_kw = last_power_kw;
        crate::services::test_support::asset_snapshots::snapshot_from_asset(
            &pv,
            crate::assets::AssetState::Pv(state),
            last_power_kw,
            f64::MAX,
        )
    }

    /// The real heater, below `temp_min_c`: its thermostat forces emergency
    /// heat, which its capability declares as the power it will draw whatever
    /// the dispatcher committed.
    fn forced_heater_snap() -> AssetSnapshot {
        use crate::assets::heater::{Heater, HeaterEmergencyMode, HeaterState};
        let heater = Heater {
            default_target_temp_c: None,
            max_kw: 3.0,
            power_stages: 1,
            temp_min_c: 20.0,
            temp_max_c: 23.0,
            temp_min_c_profile: 20.0,
            temp_max_c_profile: 23.0,
            temp_safety_max_c: 23.0,
            thermostat_delta_c: 3.0,
            emergency_mode: HeaterEmergencyMode::Normal,
            thermal_mass_kwh_per_c: 2.0,
            k_loss_kw_per_c: 0.1,
            draw_kw: 0.0,
            switching_penalty_eur: 0.0,
            ambient_temp_c: 10.0,
        };
        let state = crate::assets::AssetState::Heater(HeaterState {
            temperature_c: 19.0, // below temp_min_c: emergency heat is forced on
            actual_power_kw: 3.0,
            emergency_latched: true,
            ceiling_latched: false,
        });
        crate::services::test_support::asset_snapshots::snapshot_from_asset(
            &heater, state, 3.0, 0.0,
        )
    }

    fn make_sim() -> SimSnapshot {
        let mut assets = std::collections::HashMap::new();
        assets.insert("base_load".to_string(), snap_asset(0.5, 0.5, 0.5));
        let mut battery = snap_asset(0.0, 5.0, -5.0);
        battery.arbiter_lever = lever_snap(0.0, ArbiterLeverKind::Storage, 5.0, 5.0).arbiter_lever;
        assets.insert("battery".to_string(), battery);
        SimSnapshot {
            ts: ts(0),
            grid: GridSnapshot {
                net_power_kw: 0.5,
                voltage_v: 230.0,
                import_kwh: 0.0,
                export_kwh: 0.0,
                import_limit_kw: f64::MAX,
                export_limit_kw: -f64::MAX,
            },
            assets,
        }
    }

    /// The override as the tick runs it, without a plan.
    fn dispatch(
        sp: &mut HashMap<String, f64>,
        sim: &SimSnapshot,
        now: DateTime<Utc>,
        windows: &[DispatchWindow],
        alerts: &[AlertWindow],
        live_pv_kw: Option<f64>,
        live_base_load_kw: Option<f64>,
    ) {
        use crate::controller::arbiter::test_ticks;
        let tick = crate::controller::arbiter::ArbiterTick {
            sim,
            plan_slot: None,
            objective: crate::entities::planner_params::PlannerObjective::MinCost,
            ev_overlay: test_ticks::ev_on(),
            live_kw: &test_ticks::live(live_pv_kw, live_base_load_kw),
            live_released_kw: None,
            alert_active: false,
            limit_target_kw: None,
            prev_bridge_kw_by_asset: test_ticks::no_bridge(),
        };
        apply_dispatch_override(sp, &tick, now, windows, alerts);
    }

    fn win(setpoint_kw: f64) -> DispatchWindow {
        DispatchWindow {
            setpoint_kw,
            start: ts(0),
            end: ts(600),
            event_id: "disp-1".into(),
        }
    }

    #[test]
    fn test_apply_dispatch_override_steers_battery_to_site_setpoint() {
        let sim = make_sim();
        let mut sp = HashMap::from([("base_load".to_string(), 0.5)]);
        dispatch(&mut sp, &sim, ts(60), &[win(2.0)], &[], None, None);
        // site = base 0.5 + battery -> battery must charge 1.5 kW to hit 2.0.
        assert!((sp["battery"] - 1.5).abs() < 1e-9);
    }

    #[test]
    fn apply_dispatch_override_uses_this_ticks_live_base_load() {
        let sim = make_sim();
        let mut sp = HashMap::from([("base_load".to_string(), 0.5)]);
        dispatch(&mut sp, &sim, ts(60), &[win(2.0)], &[], None, Some(1.2));
        // net w/o battery = live base load 1.2 → battery charges 0.8, not 1.5.
        assert!((sp["battery"] - 0.8).abs() < 1e-9, "got {}", sp["battery"]);
    }

    #[test]
    fn apply_dispatch_override_prefers_live_pv_kw_over_stale_snapshot() {
        let mut sim = make_sim();
        // Stale snapshot: PV read 0.0 last tick; this tick it exports 3.0 kW.
        sim.assets
            .insert("pv".to_string(), snap_asset(0.0, 0.0, 8.0));
        let mut sp = HashMap::from([
            ("base_load".to_string(), 0.5),
            ("pv".to_string(), f64::MAX), // uncontrollable sentinel
        ]);
        dispatch(&mut sp, &sim, ts(60), &[win(2.0)], &[], Some(-3.0), None);
        // net w/o battery = base 0.5 + live PV −3.0 = −2.5 → battery charges 4.5.
        assert!(
            (sp["battery"] - 4.5).abs() < 1e-9,
            "expected battery 4.5 kW using live PV, got {}",
            sp["battery"]
        );
    }

    #[test]
    fn apply_dispatch_override_falls_back_to_snapshot_without_live_pv_kw() {
        let mut sim = make_sim();
        sim.assets
            .insert("pv".to_string(), snap_asset(0.0, 0.0, 8.0));
        let mut sp = HashMap::from([("base_load".to_string(), 0.5), ("pv".to_string(), f64::MAX)]);
        dispatch(&mut sp, &sim, ts(60), &[win(2.0)], &[], None, None);
        // net w/o battery = base 0.5 + stale PV 0.0 → battery charges 1.5.
        assert!((sp["battery"] - 1.5).abs() < 1e-9);
    }

    #[test]
    fn test_apply_dispatch_override_clamps_to_battery_capability() {
        let sim = make_sim();
        let mut sp = HashMap::from([("base_load".to_string(), 0.5)]);
        dispatch(&mut sp, &sim, ts(60), &[win(20.0)], &[], None, None);
        assert!((sp["battery"] - 5.0).abs() < 1e-9, "clamped at max charge");
        dispatch(&mut sp, &sim, ts(60), &[win(-20.0)], &[], None, None);
        assert!(
            (sp["battery"] - (-5.0)).abs() < 1e-9,
            "clamped at max discharge"
        );
    }

    #[test]
    fn test_apply_dispatch_override_alert_wins() {
        let sim = make_sim();
        let mut sp = HashMap::from([("base_load".to_string(), 0.5)]);
        let alert = AlertWindow {
            alert_type: "ALERT_GRID_EMERGENCY".into(),
            start: ts(0),
            end: ts(600),
            event_id: "a1".into(),
            message: String::new(),
        };
        dispatch(&mut sp, &sim, ts(60), &[win(2.0)], &[alert], None, None);
        assert!(
            !sp.contains_key("battery"),
            "override skipped while alert active"
        );
    }

    #[test]
    fn test_apply_dispatch_override_ignores_pv_sentinel_setpoint() {
        // Uncontrollable assets carry an f64::MAX default_setpoint_kw that
        // lands in the setpoint map — the override must fall back to live
        // power for them instead of summing the sentinel (regression: the
        // battery got clamped to full discharge because the wanted power
        // came out -inf).
        let mut sim = make_sim();
        sim.assets.insert("pv".to_string(), pv_snap(-2.0));
        let mut sp = HashMap::from([("base_load".to_string(), 0.5), ("pv".to_string(), f64::MAX)]);
        dispatch(&mut sp, &sim, ts(60), &[win(2.0)], &[], None, None);
        // net without battery = 0.5 + (-2.0 live PV) = -1.5 -> battery 3.5.
        assert!((sp["battery"] - 3.5).abs() < 1e-9);
    }

    #[test]
    fn test_apply_dispatch_override_accounts_for_heater_forced_on() {
        // Regression: WP3.4 DISPATCH_SETPOINT E2E scenario failed because the
        // heater was mid-emergency-hysteresis (drawing max_kw physically) while
        // its commanded setpoint read 0 — the override's net_without_battery
        // calc trusted the commanded 0 and undershot the battery correction by
        // exactly the heater's forced power. The heater itself reports that
        // override (`Asset::forced_power_kw`, covered in heater.rs); this test
        // pins that the override consumes it.
        let mut sim = make_sim();
        sim.assets
            .insert("heater".to_string(), forced_heater_snap());
        let mut sp = HashMap::from([
            ("base_load".to_string(), 0.5),
            ("heater".to_string(), 0.0), // dispatcher committed 0; hysteresis overrides it
        ]);
        dispatch(&mut sp, &sim, ts(60), &[win(2.0)], &[], None, None);
        // net w/o battery = base 0.5 + forced heater 3.0 = 3.5 -> battery must
        // discharge 1.5 kW (charge = -1.5) to still hit the 2.0 kW site target.
        assert!(
            (sp["battery"] - (-1.5)).abs() < 1e-9,
            "expected battery -1.5 kW accounting for the heater's forced 3.0 kW, got {}",
            sp["battery"]
        );
    }

    #[test]
    fn test_apply_dispatch_override_inactive_outside_window() {
        let sim = make_sim();
        let mut sp = HashMap::from([("base_load".to_string(), 0.5)]);
        dispatch(&mut sp, &sim, ts(700), &[win(2.0)], &[], None, None);
        assert!(!sp.contains_key("battery"), "window ended - no override");
    }

    // ── apply_comms_loss_clamp (R-59) ────────────────────────────────────────

    /// An asset declaring `kind` with its rated range — what the comms-loss clamp reads.
    fn lever_snap(
        power_kw: f64,
        kind: ArbiterLeverKind,
        rated_import_kw: f64,
        rated_export_kw: f64,
    ) -> AssetSnapshot {
        let mut snap = snap_asset(power_kw, f64::MAX, -f64::MAX);
        snap.arbiter_lever = Some(ArbiterLever {
            kind,
            rated_import_kw,
            rated_export_kw,
        });
        snap
    }

    fn comms_loss(
        active: bool,
        max_power_pct: f64,
    ) -> Option<crate::controller::comms_loss::CommsLossState> {
        Some(crate::controller::comms_loss::CommsLossState {
            active,
            max_power_pct,
        })
    }

    #[test]
    fn apply_comms_loss_clamp_noop_when_comms_loss_is_none() {
        let mut sim = make_sim();
        sim.assets.insert(
            "ev".to_string(),
            lever_snap(10.0, ArbiterLeverKind::SurplusCharge, 7.4, 0.0),
        );
        let mut sp = HashMap::from([("ev".to_string(), 7.4)]);
        apply_comms_loss_clamp(&mut sp, &sim, None);
        assert_eq!(sp["ev"], 7.4, "no comms_loss config -> untouched");
    }

    #[test]
    fn apply_comms_loss_clamp_noop_when_not_yet_active() {
        let mut sim = make_sim();
        sim.assets.insert(
            "ev".to_string(),
            lever_snap(10.0, ArbiterLeverKind::SurplusCharge, 7.4, 0.0),
        );
        let mut sp = HashMap::from([("ev".to_string(), 7.4)]);
        apply_comms_loss_clamp(&mut sp, &sim, comms_loss(false, 0.7));
        assert_eq!(sp["ev"], 7.4, "debounce not elapsed -> untouched");
    }

    #[test]
    fn apply_comms_loss_clamp_caps_ev_charge_to_pct_of_max_charge_kw() {
        let mut sim = make_sim();
        sim.assets.insert(
            "ev".to_string(),
            lever_snap(10.0, ArbiterLeverKind::SurplusCharge, 7.4, 0.0),
        );
        let mut sp = HashMap::from([("ev".to_string(), 7.4)]);
        apply_comms_loss_clamp(&mut sp, &sim, comms_loss(true, 0.5));
        assert!((sp["ev"] - 3.7).abs() < 1e-9, "got {}", sp["ev"]);
    }

    #[test]
    fn apply_comms_loss_clamp_caps_heater_to_pct_of_max_kw() {
        let mut sim = make_sim();
        sim.assets.insert(
            "heater".to_string(),
            lever_snap(6.0, ArbiterLeverKind::PausableLoad, 6.0, 0.0),
        );
        let mut sp = HashMap::from([("heater".to_string(), 6.0)]);
        apply_comms_loss_clamp(&mut sp, &sim, comms_loss(true, 0.7));
        assert!((sp["heater"] - 4.2).abs() < 1e-9, "got {}", sp["heater"]);
    }

    #[test]
    fn apply_comms_loss_clamp_caps_battery_charge_and_discharge_symmetrically() {
        let mut sim = make_sim(); // already has "battery" with cap_max_import/export, override values
        sim.assets.insert(
            "battery".to_string(),
            lever_snap(0.0, ArbiterLeverKind::Storage, 5.0, 5.0),
        );
        let mut sp = HashMap::from([("battery".to_string(), 5.0)]);
        apply_comms_loss_clamp(&mut sp, &sim, comms_loss(true, 0.6));
        assert!(
            (sp["battery"] - 3.0).abs() < 1e-9,
            "charge got {}",
            sp["battery"]
        );

        sp.insert("battery".to_string(), -5.0);
        apply_comms_loss_clamp(&mut sp, &sim, comms_loss(true, 0.6));
        assert!(
            (sp["battery"] - (-3.0)).abs() < 1e-9,
            "discharge got {}",
            sp["battery"]
        );
    }

    #[test]
    fn apply_comms_loss_clamp_leaves_setpoint_untouched_when_already_under_cap() {
        let mut sim = make_sim();
        sim.assets.insert(
            "ev".to_string(),
            lever_snap(10.0, ArbiterLeverKind::SurplusCharge, 7.4, 0.0),
        );
        let mut sp = HashMap::from([("ev".to_string(), 1.0)]);
        apply_comms_loss_clamp(&mut sp, &sim, comms_loss(true, 0.7));
        assert!((sp["ev"] - 1.0).abs() < 1e-9, "got {}", sp["ev"]);
    }

    #[test]
    fn apply_comms_loss_clamp_ignores_assets_missing_from_setpoints_map() {
        let mut sim = make_sim();
        sim.assets.insert(
            "ev".to_string(),
            lever_snap(10.0, ArbiterLeverKind::SurplusCharge, 7.4, 0.0),
        );
        let mut sp = HashMap::new(); // ev has no setpoint entry this tick
        apply_comms_loss_clamp(&mut sp, &sim, comms_loss(true, 0.5));
        assert!(!sp.contains_key("ev"));
    }

    /// W5a: the window is met by whichever asset declares storage, whatever its id.
    #[test]
    fn dispatch_steers_a_battery_named_otherwise() {
        let mut sim = make_sim();
        let battery = sim.assets.remove("battery").unwrap();
        sim.assets.insert("home-battery".to_string(), battery);
        let mut sp = HashMap::from([("base_load".to_string(), 0.5)]);
        dispatch(&mut sp, &sim, ts(60), &[win(2.0)], &[], None, None);
        assert!((sp["home-battery"] - 1.5).abs() < 1e-9, "got {sp:?}");
    }

    /// W5a: every asset declaring a lever is clamped, a second EV included.
    #[test]
    fn apply_comms_loss_clamp_caps_every_ev() {
        let mut sim = make_sim();
        for id in ["ev-a", "ev-b"] {
            let ev = lever_snap(10.0, ArbiterLeverKind::SurplusCharge, 7.4, 0.0);
            sim.assets.insert(id.to_string(), ev);
        }
        let mut sp = HashMap::from([("ev-a".to_string(), 7.4), ("ev-b".to_string(), 7.4)]);
        apply_comms_loss_clamp(&mut sp, &sim, comms_loss(true, 0.5));
        assert!((sp["ev-a"] - 3.7).abs() < 1e-9, "got {sp:?}");
        assert!((sp["ev-b"] - 3.7).abs() < 1e-9, "got {sp:?}");
    }
}
