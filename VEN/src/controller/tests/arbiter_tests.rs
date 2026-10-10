//! Unit tests for the deviation arbiter. Reconstructs the design's worked examples
//! (see `docs/architecture/VEN_ARCHITECTURE.md`'s Deviation Arbiter section) as table
//! tests, plus the lever-switching-chatter and zero-capacity-exclusion invariants.

use super::*;
use crate::controller::simulator_port::{AssetSnapshot, GridSnapshot};
use std::collections::HashMap as StdHashMap;

fn battery_snap(setpoint_kw: f64, soc_frac: f64) -> AssetSnapshot {
    use crate::assets::battery::{Battery, BatteryState};
    let battery = Battery {
        capacity_kwh: 8.0,
        max_charge_kw: 5.0,
        max_discharge_kw: 5.0,
        round_trip_efficiency: 1.0,
        min_soc_frac: 0.1,
    };
    let state = crate::assets::AssetState::Battery(BatteryState {
        soc_frac,
        actual_power_kw: setpoint_kw,
    });
    crate::services::test_support::asset_snapshots::snapshot_from_asset(
        &battery,
        state,
        "battery",
        setpoint_kw,
        setpoint_kw,
    )
}

fn ev_snap(setpoint_kw: f64, soc_frac: f64, soc_target_frac: f64, plugged: bool) -> AssetSnapshot {
    use crate::assets::ev::{EvCharger, EvState};
    let ev = EvCharger {
        max_charge_kw: 7.0,
        max_discharge_kw: 0.0,
        v2g_capable: false,
        battery_kwh: 60.0,
        consumption_kwh_per_km: 0.18,
        soc_target_frac,
        soc_target_profile: soc_target_frac,
        default_charge_kw: 0.0,
        min_soc_frac: 0.0,
        min_charge_kw: 1.4,
        response_delay_s: 10.0,
        departure_time: None,
        usage_sim: None,
        usage_sim_seed_tag: 0,
    };
    let state = crate::assets::AssetState::Ev(EvState {
        soc_frac,
        plugged,
        actual_power_kw: setpoint_kw.max(0.0),
        pending_command_kw: setpoint_kw.max(0.0),
        was_away_by_usage_sim: false,
    });
    crate::services::test_support::asset_snapshots::snapshot_from_asset(
        &ev,
        state,
        "ev",
        setpoint_kw,
        setpoint_kw,
    )
}

fn heater_snap(
    temp_c: f64,
    temp_min_c: f64,
    temp_max_c: f64,
    temp_safety_max_c: f64,
) -> AssetSnapshot {
    use crate::assets::heater::HeaterEmergencyMode;
    heater_snap_in(
        temp_c,
        (temp_min_c, temp_max_c, temp_safety_max_c),
        HeaterEmergencyMode::Normal,
        0.0,
        false,
    )
}

/// `band` = (temp_min_c, temp_max_c, temp_safety_max_c); `last_kw` = last tick's draw;
/// `emergency_latched` = a thermostat emergency already fired and is inside its hysteresis.
fn heater_snap_in(
    temp_c: f64,
    band: (f64, f64, f64),
    emergency_mode: crate::assets::heater::HeaterEmergencyMode,
    last_kw: f64,
    emergency_latched: bool,
) -> AssetSnapshot {
    use crate::assets::heater::{Heater, HeaterState};
    let (temp_min_c, temp_max_c, temp_safety_max_c) = band;
    let heater = Heater {
        default_target_temp_c: None,
        max_kw: 3.0,
        power_stages: 2,
        temp_min_c,
        temp_max_c,
        temp_min_c_profile: temp_min_c,
        temp_max_c_profile: temp_max_c,
        temp_safety_max_c,
        thermostat_delta_c: 3.0,
        emergency_mode,
        thermal_mass_kwh_per_c: 2.0,
        k_loss_kw_per_c: 0.1,
        draw_kw: 0.0,
        ambient_temp_c: 10.0,
    };
    let state = crate::assets::AssetState::Heater(HeaterState {
        temperature_c: temp_c,
        actual_power_kw: last_kw,
        emergency_latched,
        ceiling_latched: false,
    });
    crate::services::test_support::asset_snapshots::snapshot_from_asset(
        &heater, state, "heater", last_kw, 0.0,
    )
}

fn base_snap(power_kw: f64) -> AssetSnapshot {
    // From the real asset, like every other fixture here: a hand-built
    // capability is exactly the drift the arbiter is not allowed to have.
    use crate::assets::base_load::{BaseLoad, BaseLoadState};
    let base_load = BaseLoad::from_params(&crate::entities::asset_params::BaseLoadParams {
        baseline_kw: power_kw,
        ..Default::default()
    });
    let state = crate::assets::AssetState::BaseLoad(BaseLoadState {
        actual_power_kw: power_kw,
    });
    crate::services::test_support::asset_snapshots::snapshot_from_asset(
        &base_load,
        state,
        "base_load",
        power_kw,
        power_kw,
    )
}

fn make_sim(pairs: Vec<(&str, AssetSnapshot)>) -> SimSnapshot {
    SimSnapshot {
        ts: chrono::Utc::now(),
        grid: GridSnapshot {
            net_power_w: 0.0,
            voltage_v: 230.0,
            import_kwh: 0.0,
            export_kwh: 0.0,
            import_limit_kw: f64::MAX,
            export_limit_kw: -f64::MAX,
        },
        assets: pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect(),
    }
}

fn test_slot(
    marginal_cost_import: f64,
    marginal_cost_export: f64,
    net_import_kw: f64,
    net_export_kw: f64,
    pv_used_kw: f64,
    export_tariff: f64,
) -> PlanTimeSlot {
    let start = chrono::Utc::now();
    PlanTimeSlot {
        slot_index: 0,
        start,
        end: start + chrono::Duration::seconds(300),
        import_tariff_eur_kwh: 0.25,
        export_tariff_eur_kwh: export_tariff,
        co2_g_kwh: 300.0,
        grid_effective_cost: 0.25,
        marginal_cost_import_eur_per_kwh: marginal_cost_import,
        marginal_cost_export_eur_per_kwh: marginal_cost_export,
        rate_estimated: false,
        import_cap_kw: 25.0,
        export_cap_kw: 10.0,
        baseline_kw: 0.5,
        pv_forecast_kw: pv_used_kw,
        pv_used_kw,
        surplus_available_kw: 0.0,
        allocations: vec![],
        net_import_kw,
        net_export_kw,
        import_flexibility_kw: 0.0,
        export_flexibility_kw: 0.0,
        bat_charge_kw: 0.0,
        bat_discharge_kw: 0.0,
        planned_kw_by_asset: StdHashMap::new(),
        planned_state_by_asset: StdHashMap::new(),
    }
}

// ── deviation_kw ────────────────────────────────────────────────────────────

#[test]
fn deviation_kw_zero_when_projection_matches_plan() {
    let slot = test_slot(0.2, 0.2, 2.0, 0.0, 0.0, 0.08);
    assert_eq!(deviation_kw(&slot, 2.0, None), 0.0);
}

#[test]
fn deviation_kw_positive_means_importing_more_than_planned() {
    let slot = test_slot(0.2, 0.2, 2.0, 0.0, 0.0, 0.08);
    assert!((deviation_kw(&slot, 3.0, None) - 1.0).abs() < 1e-9);
}

// ── §5.4 scenario A: EV picked over battery for a surplus/import mix ───────

#[test]
fn scenario_a_ev_picked_over_battery_battery_only_bridges_the_charger_lag() {
    // PV surplus deviation (export-excess): the EV has headroom at flat 0
    // cost, the battery has a real marginal cost — so the EV is the lever
    // that takes the surplus. It cannot ramp in the tick it is commanded
    // though (R-82), so the battery bridges that one tick and releases as
    // soon as the charger has picked the surplus up (second half below).
    let sim = make_sim(vec![
        ("battery", battery_snap(0.0, 0.5)),
        ("ev", ev_snap(0.0, 0.4, 0.8, true)),
        ("heater", heater_snap(20.0, 18.0, 23.0, 23.0)),
        ("base_load", base_snap(0.5)),
        ("pv", base_snap(-6.0)),
    ]);
    // Plan expected 3.3 kW net export; PV is actually exporting more (-6.0 +
    // 0.5 base = -5.5 vs. planned -3.3) — a 2.2 kW surplus deviation, above
    // the EV's 1.4 kW minimum sustained charge rate (BL-12).
    let slot = test_slot(0.25, 0.06, 0.0, 3.3, 4.5, 0.08);
    let base_setpoints: StdHashMap<String, f64> = StdHashMap::new();
    let outcome = reconcile(
        &ArbiterTick {
            sim: &sim,
            plan_slot: Some(&slot),
            objective: PlannerObjective::MinCost,
            plan_has_ev_allocation: false,
            overlay_enabled: true,
            live_pv_kw: Some(-6.0),
            live_pv_released_kw: None,
            live_base_load_kw: Some(0.5),
            alert_active: false,
            limit_target_kw: None,
            prev_battery_bridge_kw: 0.0,
        },
        &base_setpoints,
        None,
    );
    let ev_sp = outcome.setpoints.get("ev").copied().unwrap_or(0.0);
    assert!(ev_sp > 0.0, "EV must pick up the surplus, got {ev_sp}");
    assert!(
        outcome.setpoints.get("battery").copied().unwrap_or(0.0) > 0.0,
        "the battery absorbs what the charger cannot take until its command lands"
    );

    // Next tick: the charger is drawing what it was told, so the surplus is
    // gone and the battery goes back to idle — the one-tick bridge, not a
    // standing commitment. `setpoints` carries the battery's last-applied
    // value forward as its baseline (see `projected_net_kw`'s doc comment).
    let settled = make_sim(vec![
        ("battery", battery_snap(0.0, 0.5)),
        ("ev", ev_snap(ev_sp, 0.4, 0.8, true)),
        ("heater", heater_snap(20.0, 18.0, 23.0, 23.0)),
        ("base_load", base_snap(0.5)),
        ("pv", base_snap(-6.0)),
    ]);
    let outcome = reconcile(
        &ArbiterTick {
            sim: &settled,
            plan_slot: Some(&slot),
            objective: PlannerObjective::MinCost,
            plan_has_ev_allocation: false,
            overlay_enabled: true,
            live_pv_kw: Some(-6.0),
            live_pv_released_kw: None,
            live_base_load_kw: Some(0.5),
            alert_active: false,
            limit_target_kw: None,
            prev_battery_bridge_kw: 0.0,
        },
        &base_setpoints,
        outcome.active_lever,
    );
    assert_eq!(
        outcome.setpoints.get("battery").copied().unwrap_or(0.0),
        0.0,
        "battery released once the charger took the surplus"
    );
}

// ── §5.4 scenario D: battery covers a base-load step, heater pause used too ─

#[test]
fn scenario_d_battery_covers_base_load_step_when_ev_at_target() {
    // EV already at target SoC (zero remaining capacity — must be excluded,
    // not merely deprioritized); battery has headroom and a real cost.
    let sim = make_sim(vec![
        ("battery", battery_snap(0.0, 0.4)),
        ("ev", ev_snap(0.0, 0.8, 0.8, true)),
        ("heater", heater_snap(20.0, 18.0, 23.0, 23.0)),
        ("base_load", base_snap(2.5)),
    ]);
    let slot = test_slot(0.18, 0.18, 0.5, 0.0, 0.0, 0.08);
    let mut base_setpoints: StdHashMap<String, f64> = StdHashMap::new();
    base_setpoints.insert("heater".to_string(), 0.0);
    let outcome = reconcile(
        &ArbiterTick {
            sim: &sim,
            plan_slot: Some(&slot),
            objective: PlannerObjective::MinCost,
            plan_has_ev_allocation: false,
            overlay_enabled: true,
            live_pv_kw: Some(0.0),
            live_pv_released_kw: None,
            live_base_load_kw: Some(2.5),
            alert_active: false,
            limit_target_kw: None,
            prev_battery_bridge_kw: 0.0,
        },
        &base_setpoints,
        None,
    );
    assert_eq!(
        outcome.active_lever,
        Some("battery"),
        "EV at target SoC must be excluded outright, battery must cover the step"
    );
    assert!(!outcome.setpoints.contains_key("ev") || outcome.setpoints["ev"] == 0.0);
}

// ── Heater emergency Curtail: alerts only (GB-47 — the heater's own thermostat
//    is its safety; a capacity limit's penalty-inflated cost must not override it) ─

#[test]
fn heater_emergency_curtail_not_offered_outside_an_alert() {
    let sim = make_sim(vec![("heater", heater_snap(17.0, 18.0, 23.0, 23.0))]);
    let slot = test_slot(0.30, 0.30, 5.0, 0.0, 0.0, 0.08);
    let lever = heater_emergency_lever(&sim, Some(&slot), 1.0, false, false);
    assert!(
        lever.is_none(),
        "outside an alert, forced emergency heat is the heater's own safety"
    );
}

#[test]
fn heater_emergency_curtail_offered_during_an_alert() {
    let sim = make_sim(vec![("heater", heater_snap(17.0, 18.0, 23.0, 23.0))]);
    let slot = test_slot(0.90, 0.90, 5.0, 0.0, 0.0, 0.08);
    let lever = heater_emergency_lever(&sim, Some(&slot), 1.0, false, true);
    assert!(lever.is_some(), "an alert may curtail emergency heat");
}

#[test]
fn heater_emergency_offered_while_the_heater_is_held_on_by_its_own_hysteresis() {
    // 19 °C is above temp_min_c (18) but an emergency that already fired is still
    // inside its hysteresis, so the thermostat keeps it forced on until 21 °C:
    // Curtail would free 3 kW.
    use crate::assets::heater::HeaterEmergencyMode;
    let heater = heater_snap_in(
        19.0,
        (18.0, 23.0, 23.0),
        HeaterEmergencyMode::Normal,
        3.0,
        true,
    );
    let sim = make_sim(vec![("heater", heater)]);
    let slot = test_slot(0.90, 0.90, 5.0, 0.0, 0.0, 0.08);
    let lever = heater_emergency_lever(&sim, Some(&slot), 1.0, false, true);
    assert_eq!(lever.map(|l| l.available_capacity_kw), Some(3.0));
}

#[test]
fn heater_emergency_stays_offered_while_its_own_curtail_is_active() {
    // Curtail suppresses the forced heat; the lever must still see the heat it
    // is holding back, or it would drop out and re-fire the next tick.
    use crate::assets::heater::HeaterEmergencyMode;
    let heater = heater_snap_in(
        17.0,
        (18.0, 23.0, 23.0),
        HeaterEmergencyMode::Curtail,
        0.0,
        false,
    );
    let sim = make_sim(vec![("heater", heater)]);
    let slot = test_slot(0.90, 0.90, 5.0, 0.0, 0.0, 0.08);
    assert!(heater_emergency_lever(&sim, Some(&slot), 1.0, true, true).is_some());
}

// ── PV curtailment backstop ─────────────────────────────────────────────────

#[test]
fn pv_curtailment_used_only_as_backstop_when_other_levers_exhausted() {
    // Battery full, EV unplugged, heater already at ceiling — only PV curtailment
    // left. PV suddenly exports 1 kW more than the plan expected (live -6.0 vs.
    // the plan's -5.0 net_export target), creating a genuine surplus deviation.
    let sim = make_sim(vec![
        ("battery", battery_snap(0.0, 1.0)),
        ("ev", ev_snap(0.0, 0.5, 0.8, false)),
        ("heater", heater_snap(23.0, 18.0, 23.0, 23.0)),
        ("pv", base_snap(-6.0)),
    ]);
    let slot = test_slot(0.2, 0.06, 0.0, 5.0, 5.0, 0.08);
    let base_setpoints: StdHashMap<String, f64> = StdHashMap::new();
    let outcome = reconcile(
        &ArbiterTick {
            sim: &sim,
            plan_slot: Some(&slot),
            objective: PlannerObjective::MinCost,
            plan_has_ev_allocation: false,
            overlay_enabled: true,
            live_pv_kw: Some(-6.0),
            live_pv_released_kw: None,
            live_base_load_kw: None,
            alert_active: false,
            limit_target_kw: None,
            prev_battery_bridge_kw: 0.0,
        },
        &base_setpoints,
        None,
    );
    assert_eq!(outcome.active_lever, Some("pv_curtail"));
    assert!(outcome.pv_generation_limit_tighten_kw.unwrap_or(0.0) > 0.0);
}

// ── §4a lever-switching hysteresis ──────────────────────────────────────────

#[test]
fn near_equal_cost_levers_do_not_switch_every_tick() {
    let levers = vec![
        Lever {
            id: "battery",
            available_capacity_kw: 5.0,
            marginal_cost_eur_per_kwh: 0.200,
        },
        Lever {
            id: "heater_emergency",
            available_capacity_kw: 3.0,
            marginal_cost_eur_per_kwh: 0.205, // within the preemption margin (0.02)
        },
    ];
    // battery is cheapest and starts as incumbent — heater_emergency must not
    // preempt it despite being nominally close in cost.
    let ranked = rank_levers(levers.clone(), Some("battery"));
    assert_eq!(ranked[0].id, "battery");

    // Now flip incumbency to heater_emergency, with battery nominally cheaper
    // by less than the margin — heater_emergency (incumbent) must stay first.
    let mut levers2 = levers;
    levers2[1].marginal_cost_eur_per_kwh = 0.195; // heater_emergency now cheapest, by < margin
    let ranked2 = rank_levers(levers2, Some("heater_emergency"));
    assert_eq!(
        ranked2[0].id, "heater_emergency",
        "incumbent must not be preempted by a challenger within the margin"
    );
}

#[test]
fn challenger_beyond_margin_does_preempt_incumbent() {
    let levers = vec![
        Lever {
            id: "battery",
            available_capacity_kw: 5.0,
            marginal_cost_eur_per_kwh: 0.200,
        },
        Lever {
            id: "ev",
            available_capacity_kw: 3.0,
            marginal_cost_eur_per_kwh: 0.0, // far cheaper, beyond the margin
        },
    ];
    let ranked = rank_levers(levers, Some("battery"));
    assert_eq!(
        ranked[0].id, "ev",
        "a challenger cheaper by more than the margin must preempt the incumbent"
    );
}

// ── Ported regression tests: apply_ev_lever_opportunistic (moved verbatim
//    from the former dispatcher::apply_surplus_ev_overlay, no-plan path) ───

#[test]
fn opportunistic_ev_charges_when_pv_exceeds_base() {
    let sim = make_sim(vec![
        ("pv", base_snap(-3.0)),
        ("base_load", base_snap(1.0)),
        ("ev", ev_snap(0.0, 0.4, 0.8, true)),
    ]);
    let mut sp: StdHashMap<String, f64> = StdHashMap::new();
    apply_ev_lever_opportunistic(&mut sp, &sim, None, None, false, true);
    let ev_sp = sp.get("ev").copied().unwrap_or(0.0);
    assert!((ev_sp - 2.0).abs() < 1e-6, "expected 2.0 kW, got {ev_sp}");
}

#[test]
fn opportunistic_ev_prefers_live_pv_kw_over_stale_snapshot() {
    let sim = make_sim(vec![
        ("pv", base_snap(-0.5)), // stale: last tick's power_kw
        ("base_load", base_snap(1.0)),
        ("ev", ev_snap(0.0, 0.4, 0.8, true)),
    ]);
    let mut sp: StdHashMap<String, f64> = StdHashMap::new();
    apply_ev_lever_opportunistic(&mut sp, &sim, Some(-5.0), None, false, true);
    let ev_sp = sp.get("ev").copied().unwrap_or(0.0);
    assert!((ev_sp - 4.0).abs() < 1e-6, "expected 4.0 kW, got {ev_sp}");
}

#[test]
fn opportunistic_ev_falls_back_to_stale_snapshot_without_live_pv_kw() {
    let sim = make_sim(vec![
        ("pv", base_snap(-0.5)),
        ("base_load", base_snap(1.0)),
        ("ev", ev_snap(0.0, 0.4, 0.8, true)),
    ]);
    let mut sp: StdHashMap<String, f64> = StdHashMap::new();
    apply_ev_lever_opportunistic(&mut sp, &sim, None, None, false, true);
    assert!(!sp.contains_key("ev"), "0.5 kW deficit must not charge EV");
}

#[test]
fn opportunistic_ev_capped_at_max_charge_kw() {
    let sim = make_sim(vec![
        ("pv", base_snap(-10.0)),
        ("base_load", base_snap(0.0)),
        ("ev", ev_snap(0.0, 0.4, 0.8, true)),
    ]);
    let mut sp: StdHashMap<String, f64> = StdHashMap::new();
    apply_ev_lever_opportunistic(&mut sp, &sim, None, None, false, true);
    let ev_sp = sp.get("ev").copied().unwrap_or(0.0);
    assert!(
        (ev_sp - 7.0).abs() < 1e-6,
        "expected cap at 7.0, got {ev_sp}"
    );
}

#[test]
fn opportunistic_ev_disabled_suppresses_charging() {
    let sim = make_sim(vec![
        ("pv", base_snap(-3.0)),
        ("base_load", base_snap(1.0)),
        ("ev", ev_snap(0.0, 0.4, 0.8, true)),
    ]);
    let mut sp: StdHashMap<String, f64> = StdHashMap::new();
    apply_ev_lever_opportunistic(&mut sp, &sim, None, None, false, false);
    assert!(
        !sp.contains_key("ev"),
        "overlay_enabled=false must suppress"
    );
}

// ── GB-47: stage-aware heater pause ─────────────────────────────────────────

#[test]
fn heater_pause_commands_a_reachable_step_not_one_the_heater_rounds_back_up() {
    // heater_snap: max 3.0 kW, 2 stages → steps 0 / 1.5 / 3.0; 20 °C is inside the band.
    let sim = make_sim(vec![("heater", heater_snap(20.0, 18.0, 23.0, 23.0))]);
    let mut sp = StdHashMap::from([("heater".to_string(), 1.5)]);
    // 1.5 − 0.6 = 0.9 kW: the heater would round 0.9 up to 1.5, so the lever
    // must command the highest step at or below it — 0.
    let achieved = apply_heater_pause_lever(&mut sp, &sim, 0.6);
    assert_eq!(sp["heater"], 0.0);
    assert_eq!(achieved, 1.5);
}

#[test]
fn projected_net_kw_counts_the_heater_stage_its_setpoint_rounds_to() {
    let sim = make_sim(vec![("heater", heater_snap(20.0, 18.0, 23.0, 23.0))]);
    let sp = StdHashMap::from([("heater".to_string(), 1.03)]);
    assert!((projected_net_kw(&sim, &sp, None, None) - 1.5).abs() < 1e-9);
}

#[test]
fn projected_net_kw_asks_every_stepped_asset_not_just_the_heater() {
    // R-81: a shiftable load starts at full power for any setpoint above 0 —
    // a rule the projection used to apply to the heater alone, by asset id,
    // and so got wrong for every other stepped asset.
    use crate::assets::shiftable_load::{ShiftableLoadAsset, ShiftableLoadState};
    let now = chrono::Utc::now();
    let load = ShiftableLoadAsset {
        power_kw: 2.0,
        duration_min: 90,
        earliest_start: now,
        latest_end: now + chrono::Duration::hours(6),
    };
    let snap = crate::services::test_support::asset_snapshots::snapshot_from_asset(
        &load,
        crate::assets::AssetState::ShiftableLoad(ShiftableLoadState {
            started: false,
            elapsed_min: 0.0,
            actual_power_kw: 0.0,
        }),
        "shiftable_load",
        0.0,
        0.0,
    );
    let sim = make_sim(vec![("shiftable_load", snap)]);

    let sp = StdHashMap::from([("shiftable_load".to_string(), 0.5)]);
    assert!(
        (projected_net_kw(&sim, &sp, None, None) - 2.0).abs() < 1e-9,
        "a part-load command still starts it at full power"
    );
    let off = StdHashMap::from([("shiftable_load".to_string(), 0.0)]);
    assert!((projected_net_kw(&sim, &off, None, None)).abs() < 1e-9);
}

#[test]
fn heater_pause_offers_no_capacity_while_the_thermostat_forces_power() {
    // 17 °C ≤ temp_min_c 18: the thermostat forces emergency heat, so pausing
    // the setpoint frees nothing and must not be claimed.
    let sim = make_sim(vec![("heater", heater_snap(17.0, 18.0, 23.0, 23.0))]);
    let sp = StdHashMap::from([("heater".to_string(), 1.5)]);
    assert!(heater_pause_lever(&sp, &sim, 1.0).is_none());
}

// ── Ported regression tests: apply_battery_lever (adapted from the former
//    dispatcher::apply_battery_correction_overlay, metered via assigned_kw) ─

#[test]
fn battery_lever_discharges_on_shortfall() {
    let sim = make_sim(vec![("battery", battery_snap(0.0, 0.5))]);
    let mut sp: StdHashMap<String, f64> = StdHashMap::new();
    sp.insert("battery".to_string(), 0.0);
    let delta = apply_battery_lever(&mut sp, &sim, 3.0, PlannerObjective::MinCost, None);
    assert!(
        delta > 0.0,
        "expected non-zero correction magnitude, got {delta}"
    );
    let bat_sp = sp.get("battery").copied().unwrap();
    assert!(
        bat_sp < 0.0,
        "battery must discharge (negative), got {bat_sp}"
    );
}

#[test]
fn battery_lever_suppressed_when_at_min_soc() {
    let sim = make_sim(vec![("battery", battery_snap(0.0, 0.105))]);
    let mut sp: StdHashMap<String, f64> = StdHashMap::new();
    sp.insert("battery".to_string(), 0.0);
    let delta = apply_battery_lever(&mut sp, &sim, 3.0, PlannerObjective::MinCost, None);
    assert_eq!(delta, 0.0, "discharge must be suppressed near min_soc");
}

#[test]
fn battery_lever_suppressed_for_maxrevenue_discharge() {
    let sim = make_sim(vec![("battery", battery_snap(0.0, 0.5))]);
    let mut sp: StdHashMap<String, f64> = StdHashMap::new();
    sp.insert("battery".to_string(), 0.0);
    let delta = apply_battery_lever(&mut sp, &sim, 3.0, PlannerObjective::MaxRevenue, None);
    assert_eq!(delta, 0.0, "MaxRevenue must suppress discharge corrections");
}

#[test]
fn battery_lever_allows_maxrevenue_on_export_excess() {
    let sim = make_sim(vec![("battery", battery_snap(0.0, 0.5))]);
    let mut sp: StdHashMap<String, f64> = StdHashMap::new();
    sp.insert("battery".to_string(), 0.0);
    let delta = apply_battery_lever(&mut sp, &sim, -3.0, PlannerObjective::MaxRevenue, None);
    assert!(
        delta > 0.0,
        "MaxRevenue must allow charge corrections, got {delta}"
    );
    let bat_sp = sp.get("battery").copied().unwrap();
    assert!(bat_sp > 0.0);
}

#[test]
fn battery_lever_clamped_to_max_discharge_kw() {
    let sim = make_sim(vec![("battery", battery_snap(0.0, 0.5))]);
    let mut sp: StdHashMap<String, f64> = StdHashMap::new();
    sp.insert("battery".to_string(), 0.0);
    let _delta = apply_battery_lever(&mut sp, &sim, 20.0, PlannerObjective::MinCost, None);
    let bat_sp = sp.get("battery").copied().unwrap();
    assert!(
        bat_sp >= -5.0,
        "must not exceed -max_discharge_kw, got {bat_sp}"
    );
}

#[test]
fn reconcile_battery_integrates_from_prev_setpoint_not_plan_allocation() {
    // Regression (moved from dispatcher.rs): previous tick applied +4.17 kW
    // correction; the integrator state must be that applied setpoint, not the
    // plan's allocation, so a surplus pushes the correction harder instead of
    // oscillating to discharge. The levers act on the setpoint map (shared by
    // every arbiter pass), so this rule lives in `reconcile`, which seeds the
    // map from `AssetSnapshot.setpoint_kw` — tested there, not on the lever.
    let sim = make_sim(vec![
        ("battery", battery_snap(4.17, 0.5)),
        ("base_load", base_snap(0.0)),
    ]);
    let mut base_setpoints: StdHashMap<String, f64> = StdHashMap::new();
    base_setpoints.insert("battery".to_string(), -0.5); // plan allocation, must be ignored
    let slot = test_slot(0.20, 0.20, 0.0, 0.0, 0.0, 0.08);
    // Projected net = 4.17 (battery) − 8.67 (live base load as a surplus) = −4.5.
    let outcome = reconcile(
        &ArbiterTick {
            sim: &sim,
            plan_slot: Some(&slot),
            objective: PlannerObjective::MinCost,
            plan_has_ev_allocation: false,
            overlay_enabled: true,
            live_pv_kw: None,
            live_pv_released_kw: None,
            live_base_load_kw: Some(-8.67),
            alert_active: false,
            limit_target_kw: None,
            prev_battery_bridge_kw: 0.0,
        },
        &base_setpoints,
        None,
    );
    let bat_sp = outcome.setpoints["battery"];
    assert!(
        bat_sp > 4.17,
        "correction must increase charging above prev setpoint (4.17), not oscillate to discharge; got {bat_sp}"
    );
}

// ── §3a battery corrector multi-tick stability re-verification ─────────────

#[test]
fn battery_lever_converges_under_stationary_disturbance_across_multiple_ticks() {
    // §3a.2: drive the moved battery lever for several consecutive ticks
    // under a *stationary* deviation and confirm it settles rather than
    // rings. `loops.rs`/`prev_correction_kw` — the holding mechanism the
    // original function's doc comment said its caller had to provide — is
    // confirmed absent from the codebase (grep). Since the arbiter now runs
    // unconditionally every tick and always reads `AssetSnapshot.setpoint_kw`
    // (the actually-applied value) as the integrator state, this test proves
    // that alone is sufficient: no external holding state is needed.
    let mut setpoint_kw = 0.0_f64;
    let mut soc_frac = 0.5_f64;
    const STATIONARY_DEVIATION_KW: f64 = 2.0; // constant unplanned import step
    const CAPACITY_KWH: f64 = 10.0;
    const DT_H: f64 = 300.0 / 3600.0;

    let mut history = Vec::new();
    for _ in 0..6 {
        let sim = make_sim(vec![("battery", battery_snap(setpoint_kw, soc_frac))]);
        let mut sp: StdHashMap<String, f64> = StdHashMap::new();
        // Fresh per-tick deviation: the stationary external disturbance plus
        // whatever the battery's own last correction is already contributing
        // to net import (mirrors how a real `deviation_kw` recompute would
        // already reflect the battery's prior setpoint via `projected_net_kw`)
        // — NOT a constant reapplied every tick, which would double-count the
        // battery's own correction and never converge.
        let assigned_kw = STATIONARY_DEVIATION_KW + setpoint_kw;
        let _delta =
            apply_battery_lever(&mut sp, &sim, assigned_kw, PlannerObjective::MinCost, None);
        setpoint_kw = sp.get("battery").copied().unwrap_or(setpoint_kw);
        // Physics: setpoint_kw negative = discharge, drains SoC.
        soc_frac = (soc_frac - (-setpoint_kw).max(0.0) * DT_H / CAPACITY_KWH).clamp(0.0, 1.0);
        history.push(setpoint_kw);
    }

    // Converges in one tick for a stationary disturbance (dead-beat, P=1.0)
    // and stays converged — no ringing, no sign reversal once converged.
    let converged = history[1];
    assert!(
        (converged - (-STATIONARY_DEVIATION_KW)).abs() < 1e-6,
        "expected convergence to -{STATIONARY_DEVIATION_KW} kW after one tick, got {converged}"
    );
    for (i, &sp) in history.iter().enumerate().skip(1) {
        assert!(
            (sp - converged).abs() < 1e-6,
            "setpoint must stay converged at tick {i}, got {sp} vs converged {converged} — \
             any divergence is ringing/oscillation, exactly what the missing holding \
             mechanism was meant to prevent"
        );
    }
}

#[test]
fn reconcile_battery_converges_under_stationary_disturbance_not_runaway_to_clamp() {
    // Drives the *real* production entry point (`reconcile`, not
    // `apply_battery_lever` directly) across multiple ticks, mirroring
    // exactly how `tasks::sim_tick::helpers::build_tick_setpoints` calls it:
    // `base_setpoints` is rebuilt fresh from the *static* plan allocation
    // every tick (battery's plan target never changes tick to tick, exactly
    // like `dispatcher::build_setpoints` would produce for an unchanging
    // plan slot), while `sim`'s battery snapshot carries forward whatever
    // `reconcile` actually applied last tick.
    //
    // `battery_lever_converges_under_stationary_disturbance_across_multiple_ticks`
    // above asserts the same convergence property but hand-simulates
    // `assigned_kw` directly, assuming `projected_net_kw` already folds in
    // the battery's prior setpoint. It does not: `projected_net_kw` falls
    // back to `base_setpoints.get("battery")` (the static plan target), so
    // through the real `reconcile` path the deviation never shrinks and the
    // battery is driven to its physical clamp instead of converging to the
    // exact correction.
    let mut setpoint_kw = 0.0_f64;
    let mut soc_frac = 0.5_f64;
    const STATIONARY_DEVIATION_KW: f64 = 2.0; // constant unplanned base-load step
    const CAPACITY_KWH: f64 = 10.0;
    const DT_H: f64 = 300.0 / 3600.0;
    // Plan target: 2.0 kW net import, exactly matching the pre-disturbance
    // baseline, so the entire STATIONARY_DEVIATION_KW step is unplanned. A
    // `base_load` asset must be present for `live_base_load_kw` to feed into
    // `projected_net_kw` at all (it only substitutes for an id already in
    // `sim.assets`).
    let slot = test_slot(0.20, 0.20, 2.0, 0.0, 0.0, 0.08);
    let mut base_setpoints: StdHashMap<String, f64> = StdHashMap::new();
    base_setpoints.insert("battery".to_string(), 0.0); // static plan target, never changes

    let mut history = Vec::new();
    let mut incumbent: Option<&'static str> = None;
    for _ in 0..6 {
        let sim = make_sim(vec![
            ("battery", battery_snap(setpoint_kw, soc_frac)),
            ("base_load", base_snap(2.0)),
        ]);
        let outcome = reconcile(
            &ArbiterTick {
                sim: &sim,
                plan_slot: Some(&slot),
                objective: PlannerObjective::MinCost,
                plan_has_ev_allocation: false,
                overlay_enabled: true,
                live_pv_kw: None,
                live_pv_released_kw: None,
                live_base_load_kw: Some(2.0 + STATIONARY_DEVIATION_KW),
                alert_active: false,
                limit_target_kw: None,
                prev_battery_bridge_kw: 0.0,
            },
            &base_setpoints,
            // live base load: 2.0 planned + 2.0 kW step
            incumbent,
        );
        incumbent = outcome.active_lever;
        setpoint_kw = outcome
            .setpoints
            .get("battery")
            .copied()
            .unwrap_or(setpoint_kw);
        // Physics: setpoint_kw negative = discharge, drains SoC.
        soc_frac = (soc_frac - (-setpoint_kw).max(0.0) * DT_H / CAPACITY_KWH).clamp(0.0, 1.0);
        history.push(setpoint_kw);
    }

    let converged = history[0];
    assert!(
        (converged - (-STATIONARY_DEVIATION_KW)).abs() < 1e-6,
        "expected convergence to -{STATIONARY_DEVIATION_KW} kW after one tick \
         (exactly canceling the deviation), got {converged} — a value pinned at \
         the physical clamp (-5.0), or drifting further each tick, indicates the \
         battery term of projected_net_kw is reading the static plan allocation, \
         not the arbiter's own prior setpoint"
    );
    for (i, &sp) in history.iter().enumerate().skip(1) {
        assert!(
            (sp - converged).abs() < 1e-6,
            "setpoint must stay converged at tick {i}, got {sp} vs converged {converged} — \
             any divergence means a tick where the lever didn't fire reverted the \
             correction back toward the static plan allocation instead of holding \
             the last-applied setpoint"
        );
    }
}

#[test]
fn heater_emergency_absorb_hysteresis_stays_active_within_margin_of_threshold() {
    // Absorb (surplus direction) is still entered on marginal cost.
    let sim = make_sim(vec![("heater", heater_snap(20.0, 18.0, 23.0, 23.0))]);
    // Marginal cost just below the plain threshold but within the margin of
    // it — as incumbent, the lever must still be offered (sticky exit).
    let cost = HEATER_COMFORT_OVERRIDE_EUR_PER_KWH - (LEVER_PREEMPTION_MARGIN_EUR_PER_KWH / 2.0);
    let slot = test_slot(cost, cost, 0.0, 5.0, 0.0, 0.08);
    let as_incumbent = heater_emergency_lever(&sim, Some(&slot), -1.0, true, false);
    let as_challenger = heater_emergency_lever(&sim, Some(&slot), -1.0, false, false);
    assert!(
        as_incumbent.is_some(),
        "incumbent heater emergency mode must not exit within the margin band"
    );
    assert!(
        as_challenger.is_none(),
        "a non-incumbent must still require the full threshold to enter"
    );
}

// ── R-88: a correction releases when its cause is gone ──────────────────────
//
// The plan wants 2.0 kW net import with the battery idle; base load is 2.0 kW when nothing
// is disturbed. `release_tick` runs one real `reconcile` with the battery (and optionally
// the EV) at last tick's applied setpoint, exactly as `build_tick_setpoints` does.

const PLAN_NET_KW: f64 = 2.0;

fn release_tick(
    battery_sp_kw: f64,
    ev_sp_kw: Option<f64>,
    live_base_load_kw: f64,
    incumbent: Option<&'static str>,
) -> ArbiterOutcome {
    let mut pairs = vec![
        ("battery", battery_snap(battery_sp_kw, 0.5)),
        ("base_load", base_snap(PLAN_NET_KW)),
    ];
    let mut base_setpoints: StdHashMap<String, f64> = StdHashMap::new();
    base_setpoints.insert("battery".to_string(), 0.0); // the plan's allocation
    if let Some(ev_kw) = ev_sp_kw {
        pairs.push(("ev", ev_snap(ev_kw, 0.5, 0.9, true)));
        base_setpoints.insert("ev".to_string(), 0.0);
    }
    let sim = make_sim(pairs);
    let slot = test_slot(0.20, 0.20, PLAN_NET_KW, 0.0, 0.0, 0.08);
    reconcile(
        &ArbiterTick {
            sim: &sim,
            plan_slot: Some(&slot),
            objective: PlannerObjective::MinCost,
            plan_has_ev_allocation: false,
            overlay_enabled: true,
            live_pv_kw: None,
            live_pv_released_kw: None,
            live_base_load_kw: Some(live_base_load_kw),
            alert_active: false,
            limit_target_kw: None,
            prev_battery_bridge_kw: 0.0,
        },
        &base_setpoints,
        incumbent,
    )
}

#[test]
fn reconcile_releases_the_battery_to_plan_once_the_disturbance_is_gone() {
    // Tick 0: a +2 kW base-load step. The battery discharges 2 kW to cancel it.
    let engaged = release_tick(0.0, None, PLAN_NET_KW + 2.0, None);
    assert!((engaged.setpoints["battery"] - -2.0).abs() < 1e-6);
    assert_eq!(engaged.active_lever, Some("battery"));

    // Tick 1: the step is gone. With the battery back at plan there would be no deviation,
    // so the correction is released to plan in one tick instead of being undone stepwise.
    let released = release_tick(-2.0, None, PLAN_NET_KW, Some("battery"));
    assert!(
        released.setpoints["battery"].abs() < 1e-6,
        "the battery must return to its plan allocation, got {}",
        released.setpoints["battery"]
    );
    assert_eq!(
        released.active_lever, None,
        "release is the one true 'cleared'"
    );
}

#[test]
fn reconcile_keeps_reporting_the_lever_while_its_correction_holds() {
    // The step persists and the correction has settled: nothing moves this tick, but the
    // battery is still held off-plan, so the correction is active — not "cleared".
    let held = release_tick(-2.0, None, PLAN_NET_KW + 2.0, Some("battery"));
    assert!((held.setpoints["battery"] - -2.0).abs() < 1e-6);
    assert_eq!(held.active_lever, Some("battery"));
}

#[test]
fn reconcile_keeps_correcting_while_a_deviation_without_its_correction_remains() {
    // A 3 kW step shrinks to 1 kW: the cause is not gone, so no release — the dead-beat
    // corrector tracks it down from its carried value instead.
    let tracked = release_tick(-3.0, None, PLAN_NET_KW + 1.0, Some("battery"));
    assert!(
        (tracked.setpoints["battery"] - -1.0).abs() < 1e-6,
        "expected the correction to shrink to -1 kW, got {}",
        tracked.setpoints["battery"]
    );
    assert_eq!(tracked.active_lever, Some("battery"));
}

#[test]
fn reconcile_releases_battery_and_ev_together() {
    // Both carried levers are off-plan and the disturbance is gone: both return to plan.
    let released = release_tick(-1.0, Some(1.4), PLAN_NET_KW, Some("battery"));
    assert!(released.setpoints["battery"].abs() < 1e-6);
    assert!(released.setpoints["ev"].abs() < 1e-6);
    assert_eq!(released.active_lever, None);
}

#[test]
fn reconcile_keeps_reporting_a_held_lever_when_no_lever_can_act_this_tick() {
    // The battery has been discharging 2 kW to hold a correction and has now reached its
    // minimum SoC, so it cannot take the next 1 kW of disturbance: that 1 kW is unresolved.
    // The correction is still held — the battery is still 2 kW off-plan — so this is not
    // a "cleared".
    let sim = make_sim(vec![
        ("battery", battery_snap(-2.0, 0.1)),
        ("base_load", base_snap(PLAN_NET_KW)),
    ]);
    let mut base_setpoints: StdHashMap<String, f64> = StdHashMap::new();
    base_setpoints.insert("battery".to_string(), 0.0);
    let slot = test_slot(0.20, 0.20, PLAN_NET_KW, 0.0, 0.0, 0.08);
    let outcome = reconcile(
        &ArbiterTick {
            sim: &sim,
            plan_slot: Some(&slot),
            objective: PlannerObjective::MinCost,
            plan_has_ev_allocation: false,
            overlay_enabled: true,
            live_pv_kw: None,
            live_pv_released_kw: None,
            live_base_load_kw: Some(PLAN_NET_KW + 3.0),
            alert_active: false,
            limit_target_kw: None,
            prev_battery_bridge_kw: 0.0,
        },
        &base_setpoints,
        Some("battery"),
    );
    assert!(
        outcome.unresolved_kw > 0.9,
        "the extra 1 kW has no lever: {}",
        outcome.unresolved_kw
    );
    assert_eq!(outcome.active_lever, Some("battery"));
}

#[test]
fn deviation_if_released_judges_pv_without_the_arbiters_own_curtailment() {
    // The site exports 1.17 kW more than planned (live PV 4.03 kW against a plan built on
    // 2.86 kW), and the arbiter answered last tick by curtailing PV: the inverter now holds
    // PV at the planned 2.86 kW, so `live_pv_kw` shows no deviation at all. Released, that
    // curtailment ends and the over-export is back — so the cause is NOT gone. Judged on the
    // curtailed PV, the correction would be released, re-engaged a tick later, released
    // again: the cycle that made "cleared" a coin toss in the E2E run of 2026-10-05.
    let sim = make_sim(vec![
        ("battery", battery_snap(0.0, 0.5)),
        ("base_load", base_snap(0.5)),
        ("pv", base_snap(-2.86)),
    ]);
    let mut base_setpoints: StdHashMap<String, f64> = StdHashMap::new();
    base_setpoints.insert("battery".to_string(), 0.0);
    let plan_net_kw = 0.5 - 2.86;
    let slot = test_slot(0.20, 0.20, 0.0, -plan_net_kw, 2.86, 0.08);
    let outcome = reconcile(
        &ArbiterTick {
            sim: &sim,
            plan_slot: Some(&slot),
            objective: PlannerObjective::MinCost,
            plan_has_ev_allocation: false,
            overlay_enabled: true,
            live_pv_kw: Some(-2.86),
            live_pv_released_kw: Some(-4.03),
            live_base_load_kw: Some(0.5),
            alert_active: false,
            limit_target_kw: None,
            prev_battery_bridge_kw: 0.0,
        },
        &base_setpoints,
        Some("pv_curtail"),
    );
    let without = outcome
        .dev_without_correction_kw
        .expect("plan slot present");
    assert!(
        (without - -1.17).abs() < 1e-6,
        "released, the 1.17 kW over-export returns; got {without}"
    );
}

#[test]
fn reconcile_reports_the_deviation_without_its_correction() {
    // What decides the release is visible: with the battery at plan the step would show.
    let held = release_tick(-2.0, None, PLAN_NET_KW + 2.0, Some("battery"));
    let without = held
        .dev_without_correction_kw
        .expect("reported whenever there is a plan slot");
    assert!((without - 2.0).abs() < 1e-6, "got {without}");
}

#[test]
fn a_held_correction_feeds_the_replan_backstop_as_energy() {
    // The backstop must hear a correction that is held, not only one that is moving: a
    // settled -2 kW battery is 2 kW off-plan for the whole tick.
    let held = release_tick(-2.0, None, PLAN_NET_KW + 2.0, Some("battery"));
    let dt_h = 1.0 / 3600.0;
    let residual =
        held.residual_kwh_by_asset(1.0, &make_sim(vec![("battery", battery_snap(-2.0, 0.5))]));
    let battery_kwh = residual.get("battery").copied().unwrap_or(0.0);
    assert!(
        (battery_kwh - 2.0 * dt_h).abs() < 1e-12,
        "expected 2 kW x 1 s of displaced energy, got {battery_kwh} kWh"
    );
}

#[test]
fn a_released_correction_feeds_the_backstop_nothing() {
    let released = release_tick(-2.0, None, PLAN_NET_KW, Some("battery"));
    let residual =
        released.residual_kwh_by_asset(1.0, &make_sim(vec![("battery", battery_snap(-2.0, 0.5))]));
    assert!(
        residual.values().all(|&kwh| kwh.abs() < 1e-12),
        "{residual:?}"
    );
}

#[path = "arbiter_limit_tests.rs"]
mod limit_tests;

// ── R-104: the real charger's response lag, ticked ───────────────────────────

/// A site the R-104 tests tick: the real `EvCharger` stepped between ticks so its
/// one-tick command lag is physics, not a fixture assumption; the battery follows
/// its command at once; both arbiter passes run as `build_tick_setpoints` runs them,
/// and each tick's battery bridge is fed back as the next tick's.
struct LaggedSite {
    ev: crate::assets::ev::EvCharger,
    ev_state: crate::assets::ev::EvState,
    ev_cmd_kw: f64,
    battery_kw: f64,
    incumbent: Option<&'static str>,
    limit_incumbent: Option<&'static str>,
    /// Last tick's battery bridge of both passes, fed back as production's tick does.
    bridge_kw: f64,
    pv_kw: f64,
    base_kw: f64,
    /// Planned site net (kW, + = import).
    plan_net_kw: f64,
    hard_limit_kw: Option<f64>,
}

impl LaggedSite {
    fn new(
        pv_kw: f64,
        base_kw: f64,
        plan_net_kw: f64,
        ev_kw: f64,
        hard_limit_kw: Option<f64>,
    ) -> Self {
        use crate::assets::ev::{EvCharger, EvState};
        Self {
            ev: EvCharger {
                max_charge_kw: 7.0,
                max_discharge_kw: 0.0,
                v2g_capable: false,
                battery_kwh: 60.0,
                consumption_kwh_per_km: 0.18,
                soc_target_frac: 0.8,
                soc_target_profile: 0.8,
                default_charge_kw: 0.0,
                min_soc_frac: 0.0,
                min_charge_kw: 1.4,
                response_delay_s: 10.0,
                departure_time: None,
                usage_sim: None,
                usage_sim_seed_tag: 0,
            },
            ev_state: EvState {
                soc_frac: 0.4,
                plugged: true,
                actual_power_kw: ev_kw,
                pending_command_kw: ev_kw,
                was_away_by_usage_sim: false,
            },
            ev_cmd_kw: ev_kw,
            battery_kw: 0.0,
            incumbent: None,
            limit_incumbent: None,
            bridge_kw: 0.0,
            pv_kw,
            base_kw,
            plan_net_kw,
            hard_limit_kw,
        }
    }

    /// Both passes, then the physics. Returns (EV command, measured site net kW).
    fn tick(&mut self) -> (f64, f64) {
        use crate::controller::arbiter::limit;
        let ev_snapshot = crate::services::test_support::asset_snapshots::snapshot_from_asset(
            &self.ev,
            crate::assets::AssetState::Ev(self.ev_state.clone()),
            "ev",
            self.ev_state.actual_power_kw,
            self.ev_cmd_kw,
        );
        let sim = make_sim(vec![
            ("battery", battery_snap(self.battery_kw, 0.5)),
            ("ev", ev_snapshot),
            ("base_load", base_snap(self.base_kw)),
            ("pv", base_snap(self.pv_kw)),
        ]);
        let (import_kw, export_kw) = (self.plan_net_kw.max(0.0), (-self.plan_net_kw).max(0.0));
        let slot = test_slot(0.25, 0.06, import_kw, export_kw, -self.pv_kw, 0.08);
        let tick = ArbiterTick {
            sim: &sim,
            plan_slot: Some(&slot),
            objective: PlannerObjective::MinCost,
            plan_has_ev_allocation: false,
            overlay_enabled: true,
            live_pv_kw: Some(self.pv_kw),
            live_pv_released_kw: None,
            live_base_load_kw: Some(self.base_kw),
            alert_active: false,
            limit_target_kw: limit::limit_target_kw(self.hard_limit_kw, self.limit_incumbent),
            prev_battery_bridge_kw: self.bridge_kw,
        };
        let mut outcome = reconcile(&tick, &StdHashMap::new(), self.incumbent);
        let limit_pass = limit::enforce_import_limit(
            &tick,
            &mut outcome.setpoints,
            self.limit_incumbent,
            &limit::SetpointBoundsKw::new(),
        );
        self.incumbent = outcome.active_lever;
        self.limit_incumbent = limit_pass.as_ref().and_then(|l| l.active_lever);
        self.bridge_kw =
            outcome.battery_bridge_kw + limit_pass.as_ref().map_or(0.0, |l| l.battery_bridge_kw);
        if let Some(&kw) = outcome.setpoints.get("ev") {
            self.ev_cmd_kw = kw;
        }
        if let Some(&kw) = outcome.setpoints.get("battery") {
            self.battery_kw = kw;
        }
        let (state, ev_kw) =
            self.ev
                .step_inner(&self.ev_state, self.ev_cmd_kw, chrono::Duration::seconds(1));
        self.ev_state = state;
        (
            self.ev_cmd_kw,
            self.base_kw + self.pv_kw + ev_kw + self.battery_kw,
        )
    }
}

/// Changes of the EV command over `ticks` (a settled lever has none).
fn ev_command_changes(ticks: &[(f64, f64)]) -> usize {
    ticks
        .windows(2)
        .filter(|w| (w[0].0 - w[1].0).abs() > 1e-6)
        .count()
}

#[test]
fn reconcile_settles_on_the_ev_under_the_real_charger_lag() {
    // R-104: logged in a full E2E run, the arbiter toggled the EV between 1.5 kW and
    // off every ~20 s and swung the battery against it, while the site itself sat on
    // plan. The battery's one-tick bridge for the charger's lag was carried forward as
    // a correction; once the EV had landed, the carried bridge read as a deviation and
    // the arbiter cut the zero-cost EV to remove it. The fixtures above set the
    // charger's actual power to its command, so the lag never showed; here the real
    // charger is stepped. Scenario A's geometry: a steady 2.2 kW PV surplus over plan.
    let mut site = LaggedSite::new(-6.0, 0.5, -3.3, 0.0, None);
    let ticks: Vec<(f64, f64)> = (0..60).map(|_| site.tick()).collect();
    let settled = &ticks[5..];
    assert_eq!(
        ev_command_changes(settled),
        0,
        "the EV command must settle, not hunt: {:?}",
        settled.iter().map(|t| t.0).collect::<Vec<_>>()
    );
    for (i, &(_, net_kw)) in ticks.iter().enumerate() {
        assert!(
            (net_kw - -3.3).abs() < 0.1,
            "tick {i}: the site should sit on plan, net {net_kw:.2} kW"
        );
    }
    assert!(
        site.battery_kw.abs() < 0.1,
        "the zero-cost EV holds the surplus; the battery only bridged the lag, got {:.2} kW",
        site.battery_kw
    );
}

#[test]
fn both_passes_settle_under_a_hard_limit_with_the_real_charger_lag() {
    // R-82's case, ticked: the EV charging 7 kW under a 3 kW import limit. The limit
    // pass cuts the charger and the battery bridges the tick until the cut lands, so
    // import never crosses the limit; once it has landed, the bridge must not be
    // carried into a correction, or the passes hunt the EV against the battery (R-104).
    let mut site = LaggedSite::new(0.0, 0.5, 7.5, 7.0, Some(3.0));
    let ticks: Vec<(f64, f64)> = (0..60).map(|_| site.tick()).collect();
    for (i, &(_, net_kw)) in ticks.iter().enumerate() {
        assert!(
            net_kw <= 3.0 + 1e-6,
            "tick {i}: import {net_kw:.2} kW above the 3 kW limit"
        );
    }
    assert_eq!(
        ev_command_changes(&ticks[5..]),
        0,
        "the EV command must settle under the limit: {:?}",
        ticks.iter().map(|t| t.0).collect::<Vec<_>>()
    );
    assert!(
        site.battery_kw.abs() < 0.1,
        "the charger holds the cut; the battery only bridged its lag, got {:.2} kW",
        site.battery_kw
    );
}
