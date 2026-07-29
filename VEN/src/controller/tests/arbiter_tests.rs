//! Unit tests for the deviation arbiter. Reconstructs the design's worked examples
//! (see `docs/architecture/VEN_ARCHITECTURE.md`'s Deviation Arbiter section) as table
//! tests, plus the lever-switching-chatter and zero-capacity-exclusion invariants.

use super::*;
use crate::controller::simulator_port::{AssetSnapshot, GridSnapshot};
use std::collections::HashMap as StdHashMap;

fn battery_snap(setpoint_kw: f64, soc: f64) -> AssetSnapshot {
    let mut values = StdHashMap::new();
    values.insert("soc".into(), soc);
    values.insert("min_soc".into(), 0.1);
    const CAPACITY_KWH: f64 = 8.0;
    AssetSnapshot {
        power_kw: setpoint_kw,
        asset_type: "battery".into(),
        cap_max_import_kw: 5.0,
        cap_max_export_kw: 5.0,
        available_discharge_kwh: Some((soc * CAPACITY_KWH).max(0.0)),
        available_charge_kwh: Some(((1.0 - soc) * CAPACITY_KWH).max(0.0)),
        default_setpoint_kw: 0.0,
        setpoint_kw,
        values,
    }
}

fn ev_snap(setpoint_kw: f64, soc: f64, soc_target: f64, plugged: bool) -> AssetSnapshot {
    let mut values = StdHashMap::new();
    values.insert("plugged".into(), if plugged { 1.0 } else { 0.0 });
    values.insert("soc".into(), soc);
    values.insert("soc_target".into(), soc_target);
    values.insert("max_charge_kw".into(), 7.0);
    values.insert("min_charge_kw".into(), 1.4);
    AssetSnapshot {
        power_kw: setpoint_kw,
        asset_type: "ev".into(),
        cap_max_import_kw: 7.0,
        cap_max_export_kw: 0.0,
        available_discharge_kwh: None,
        available_charge_kwh: None,
        default_setpoint_kw: 0.0,
        setpoint_kw,
        values,
    }
}

fn heater_snap(
    temp_c: f64,
    temp_min_c: f64,
    temp_max_c: f64,
    temp_safety_max_c: f64,
) -> AssetSnapshot {
    let mut values = StdHashMap::new();
    values.insert("temp_c".into(), temp_c);
    values.insert("temp_min_c".into(), temp_min_c);
    values.insert("temp_max_c".into(), temp_max_c);
    values.insert("temp_safety_max_c".into(), temp_safety_max_c);
    values.insert("max_kw".into(), 3.0);
    AssetSnapshot {
        power_kw: 0.0,
        asset_type: "heater".into(),
        cap_max_import_kw: 3.0,
        cap_max_export_kw: 0.0,
        default_setpoint_kw: 0.0,
        setpoint_kw: 0.0,
        available_discharge_kwh: None,
        available_charge_kwh: None,
        values,
    }
}

fn base_snap(power_kw: f64) -> AssetSnapshot {
    AssetSnapshot {
        power_kw,
        asset_type: "base_load".into(),
        cap_max_import_kw: power_kw,
        cap_max_export_kw: 0.0,
        available_discharge_kwh: None,
        available_charge_kwh: None,
        default_setpoint_kw: power_kw,
        setpoint_kw: power_kw,
        values: StdHashMap::new(),
    }
}

fn make_sim(pairs: Vec<(&str, AssetSnapshot)>) -> SimSnapshot {
    SimSnapshot {
        ts: chrono::Utc::now(),
        grid: GridSnapshot {
            net_power_w: 0.0,
            voltage_v: 230.0,
            import_kwh: 0.0,
            export_kwh: 0.0,
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
    assert_eq!(deviation_kw(&slot, 2.0), 0.0);
}

#[test]
fn deviation_kw_positive_means_importing_more_than_planned() {
    let slot = test_slot(0.2, 0.2, 2.0, 0.0, 0.0, 0.08);
    assert!((deviation_kw(&slot, 3.0) - 1.0).abs() < 1e-9);
}

// ── §5.4 scenario A: EV picked over battery for a surplus/import mix ───────

#[test]
fn scenario_a_ev_picked_over_battery_no_battery_movement() {
    // PV surplus deviation (export-excess): EV has headroom at flat 0 cost,
    // battery has a real marginal cost — EV must absorb first, battery must
    // not move at all while EV alone can cover the deviation.
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
        &sim,
        &base_setpoints,
        Some(&slot),
        PlannerObjective::MinCost,
        false,
        true,
        Some(-6.0),
        Some(0.5),
        None,
    );
    assert_eq!(outcome.active_lever, Some("ev"));
    assert!(
        !outcome.setpoints.contains_key("battery"),
        "battery must not move while EV alone covers the deviation"
    );
    let ev_sp = outcome.setpoints.get("ev").copied().unwrap_or(0.0);
    assert!(ev_sp > 0.0, "EV must pick up the surplus, got {ev_sp}");
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
        &sim,
        &base_setpoints,
        Some(&slot),
        PlannerObjective::MinCost,
        false,
        true,
        Some(0.0),
        Some(2.5),
        None,
    );
    assert_eq!(
        outcome.active_lever,
        Some("battery"),
        "EV at target SoC must be excluded outright, battery must cover the step"
    );
    assert!(!outcome.setpoints.contains_key("ev") || outcome.setpoints["ev"] == 0.0);
}

// ── §5.4 scenario H: heater emergency only above the comfort-override threshold ─

#[test]
fn heater_emergency_not_offered_below_comfort_override_threshold() {
    let sim = make_sim(vec![("heater", heater_snap(17.0, 18.0, 23.0, 23.0))]);
    // Routine tariff-level marginal cost, well below the override threshold.
    let slot = test_slot(0.30, 0.30, 5.0, 0.0, 0.0, 0.08);
    let lever = heater_emergency_lever(&sim, &slot, 1.0, false);
    assert!(
        lever.is_none(),
        "routine marginal cost must not invade the safety envelope"
    );
}

#[test]
fn heater_emergency_offered_once_obligation_penalty_exceeds_threshold() {
    let sim = make_sim(vec![("heater", heater_snap(17.0, 18.0, 23.0, 23.0))]);
    // Obligation-penalty-inflated marginal cost, above the override threshold.
    let slot = test_slot(0.90, 0.90, 5.0, 0.0, 0.0, 0.08);
    let lever = heater_emergency_lever(&sim, &slot, 1.0, false);
    assert!(
        lever.is_some(),
        "an obligation breach penalty must cross the threshold and offer the lever"
    );
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
        &sim,
        &base_setpoints,
        Some(&slot),
        PlannerObjective::MinCost,
        false,
        true,
        Some(-6.0),
        None,
        None,
    );
    assert_eq!(outcome.active_lever, Some("pv_curtail"));
    assert!(outcome.pv_export_limit_tighten_kw.unwrap_or(0.0) > 0.0);
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

// ── Ported regression tests: apply_battery_lever (adapted from the former
//    dispatcher::apply_battery_correction_overlay, metered via assigned_kw) ─

#[test]
fn battery_lever_discharges_on_shortfall() {
    let sim = make_sim(vec![("battery", battery_snap(0.0, 0.5))]);
    let mut sp: StdHashMap<String, f64> = StdHashMap::new();
    sp.insert("battery".to_string(), 0.0);
    let delta = apply_battery_lever(&mut sp, &sim, 3.0, PlannerObjective::MinCost);
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
    let delta = apply_battery_lever(&mut sp, &sim, 3.0, PlannerObjective::MinCost);
    assert_eq!(delta, 0.0, "discharge must be suppressed near min_soc");
}

#[test]
fn battery_lever_suppressed_for_maxrevenue_discharge() {
    let sim = make_sim(vec![("battery", battery_snap(0.0, 0.5))]);
    let mut sp: StdHashMap<String, f64> = StdHashMap::new();
    sp.insert("battery".to_string(), 0.0);
    let delta = apply_battery_lever(&mut sp, &sim, 3.0, PlannerObjective::MaxRevenue);
    assert_eq!(delta, 0.0, "MaxRevenue must suppress discharge corrections");
}

#[test]
fn battery_lever_allows_maxrevenue_on_export_excess() {
    let sim = make_sim(vec![("battery", battery_snap(0.0, 0.5))]);
    let mut sp: StdHashMap<String, f64> = StdHashMap::new();
    sp.insert("battery".to_string(), 0.0);
    let delta = apply_battery_lever(&mut sp, &sim, -3.0, PlannerObjective::MaxRevenue);
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
    let _delta = apply_battery_lever(&mut sp, &sim, 20.0, PlannerObjective::MinCost);
    let bat_sp = sp.get("battery").copied().unwrap();
    assert!(
        bat_sp >= -5.0,
        "must not exceed -max_discharge_kw, got {bat_sp}"
    );
}

#[test]
fn battery_lever_converges_not_oscillates_using_prev_setpoint() {
    // Regression (moved from dispatcher.rs): previous tick applied +4.17 kW
    // correction; using setpoint_kw (not the plan's sp_map entry) as the
    // integrator state must push the correction harder, not oscillate to
    // discharge.
    let sim = make_sim(vec![("battery", battery_snap(4.17, 0.5))]);
    let mut sp: StdHashMap<String, f64> = StdHashMap::new();
    sp.insert("battery".to_string(), -0.5); // plan allocation, must be ignored
    let _delta = apply_battery_lever(&mut sp, &sim, -4.5, PlannerObjective::MinCost);
    let bat_sp = sp.get("battery").copied().unwrap();
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
    let mut soc = 0.5_f64;
    const STATIONARY_DEVIATION_KW: f64 = 2.0; // constant unplanned import step
    const CAPACITY_KWH: f64 = 10.0;
    const DT_H: f64 = 300.0 / 3600.0;

    let mut history = Vec::new();
    for _ in 0..6 {
        let sim = make_sim(vec![("battery", battery_snap(setpoint_kw, soc))]);
        let mut sp: StdHashMap<String, f64> = StdHashMap::new();
        // Fresh per-tick deviation: the stationary external disturbance plus
        // whatever the battery's own last correction is already contributing
        // to net import (mirrors how a real `deviation_kw` recompute would
        // already reflect the battery's prior setpoint via `projected_net_kw`)
        // — NOT a constant reapplied every tick, which would double-count the
        // battery's own correction and never converge.
        let assigned_kw = STATIONARY_DEVIATION_KW + setpoint_kw;
        let _delta = apply_battery_lever(&mut sp, &sim, assigned_kw, PlannerObjective::MinCost);
        setpoint_kw = sp.get("battery").copied().unwrap_or(setpoint_kw);
        // Physics: setpoint_kw negative = discharge, drains SoC.
        soc = (soc - (-setpoint_kw).max(0.0) * DT_H / CAPACITY_KWH).clamp(0.0, 1.0);
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
fn heater_emergency_mode_hysteresis_stays_active_within_margin_of_threshold() {
    let sim = make_sim(vec![("heater", heater_snap(17.0, 18.0, 23.0, 23.0))]);
    // Marginal cost just below the plain threshold but within the margin of
    // it — as incumbent, the lever must still be offered (sticky exit).
    let cost = HEATER_COMFORT_OVERRIDE_EUR_PER_KWH - (LEVER_PREEMPTION_MARGIN_EUR_PER_KWH / 2.0);
    let slot = test_slot(cost, cost, 5.0, 0.0, 0.0, 0.08);
    let as_incumbent = heater_emergency_lever(&sim, &slot, 1.0, true);
    let as_challenger = heater_emergency_lever(&sim, &slot, 1.0, false);
    assert!(
        as_incumbent.is_some(),
        "incumbent heater emergency mode must not exit within the margin band"
    );
    assert!(
        as_challenger.is_none(),
        "a non-incumbent must still require the full threshold to enter"
    );
}
