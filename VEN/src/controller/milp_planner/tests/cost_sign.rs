use super::*;
use chrono::TimeZone;

// ── BL-40: AssetAllocation.cost_eur must price PV-surplus consumption as
// forgone export revenue (opportunity cost, `+`), matching
// envelopes.rs::solved_session_cost()'s convention — not as a credit (`-`).

fn noon() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 4, 11, 12, 0, 0).unwrap()
}

/// Boosts PV rated/inverter capacity so noon surplus (~rated_kw at noon under
/// the sin model, minus the small base load) comfortably exceeds any single
/// asset's max power — guaranteeing a fully-surplus-covered slot to test against.
fn boost_pv(profile: &mut Profile, rated_kw: f64) {
    profile.assets = std::mem::take(&mut profile.assets)
        .into_iter()
        .map(|a| match a {
            AssetProfile::Pv(mut pv) => {
                pv.rated_kw = rated_kw;
                pv.inverter_max_kw = rated_kw;
                AssetProfile::Pv(pv)
            }
            other => other,
        })
        .collect();
}

/// Asserts every allocation for `asset_id` matches the opportunity-cost formula,
/// and that at least one slot fully covered by PV surplus (grid_power_kw ≈ 0)
/// reports a strictly positive `cost_eur` (forgone export revenue), not a credit.
fn assert_opportunity_cost_convention(plan: &crate::entities::plan::Plan, asset_id: &str) {
    let mut found_fully_surplus_covered = false;
    for slot in &plan.slots {
        let dt_h = (slot.end - slot.start).num_seconds() as f64 / 3600.0;
        for a in slot.allocations.iter().filter(|a| a.asset_id == asset_id) {
            let expected = a.grid_power_kw * slot.import_tariff_eur_kwh * dt_h
                + a.surplus_power_kw * slot.export_tariff_eur_kwh * dt_h;
            assert!(
                (a.cost_eur - expected).abs() < 1e-6,
                "{asset_id} cost_eur {:.6} must equal opportunity-cost formula {:.6} \
                 (grid_power_kw={:.4}, surplus_power_kw={:.4})",
                a.cost_eur,
                expected,
                a.grid_power_kw,
                a.surplus_power_kw
            );
            if a.surplus_power_kw > 0.01 && a.grid_power_kw < 0.01 {
                assert!(
                    a.cost_eur > 0.0,
                    "{asset_id} slot fully covered by PV surplus must report a positive \
                     forgone-export cost, got {:.6}",
                    a.cost_eur
                );
                found_fully_surplus_covered = true;
            }
        }
    }
    assert!(
        found_fully_surplus_covered,
        "expected at least one {asset_id} slot fully covered by PV surplus"
    );
}

#[test]
fn ev_allocation_cost_eur_prices_pv_surplus_as_opportunity_cost() {
    let now = noon();
    let mut profile = make_profile_1800s();
    profile.assets.retain(|a| {
        matches!(
            a,
            AssetProfile::Ev(_) | AssetProfile::Pv(_) | AssetProfile::BaseLoad(_)
        )
    });
    boost_pv(&mut profile, 20.0);
    profile.assets = profile
        .assets
        .into_iter()
        .map(|a| match a {
            AssetProfile::Ev(mut ev) => {
                // Small pack so the (soc_target - soc) energy need is achievable
                // within the 2h departure window at max_charge_kw.
                ev.battery_kwh = 10.0;
                AssetProfile::Ev(ev)
            }
            other => other,
        })
        .collect();

    let mut sim = make_snap_from_profile(&profile);
    set_ev_plugged(&mut sim, true);
    if let Some(ev) = sim.assets.get_mut("ev") {
        let bat_kwh = ev.val("battery_kwh").unwrap_or(60.0);
        let soc_target = ev.val("soc_target").unwrap_or(0.8);
        let max_ch = ev.val("max_charge_kw").unwrap_or(7.4);
        ev.values.insert("soc".into(), 0.1);
        ev.cap_max_import_kw = if 0.1_f64 >= soc_target { 0.0 } else { max_ch };
        ev.available_discharge_kwh = Some(0.1 * bat_kwh);
        ev.available_charge_kwh = Some(0.9 * bat_kwh);
    }
    let session = crate::entities::device_session::EvSession {
        mode: Default::default(),
        id: uuid::Uuid::new_v4(),
        target_soc: 0.8,
        departure_time: now + Duration::hours(2),
        soft_deadline: false,
        budget_eur: None,
        comfort_rates: vec![],
        created_at: now,
        updated_at: now,
    };
    let tariffs = make_tariffs(0.25, 0.08, 300.0);
    let plan = run_planner(
        build_asset_contexts(&profile, &sim, now, Some(&session), None, &tariffs),
        &sim,
        &tariffs,
        &no_capacity(),
        &profile,
        now,
        crate::entities::asset::PlanTrigger::Periodic,
        Some(&session),
        None,
        &[],
        None,
        None,
    );
    assert_opportunity_cost_convention(&plan, "ev");
}

#[test]
fn heater_allocation_cost_eur_prices_pv_surplus_as_opportunity_cost() {
    let now = noon();
    let mut profile = make_profile_1800s();
    profile.assets.retain(|a| {
        matches!(
            a,
            AssetProfile::Heater(_) | AssetProfile::Pv(_) | AssetProfile::BaseLoad(_)
        )
    });
    boost_pv(&mut profile, 20.0);
    if let Some(AssetProfile::Heater(ref mut h)) = profile
        .assets
        .iter_mut()
        .find(|a| matches!(a, AssetProfile::Heater(_)))
    {
        h.temp_initial_c = 19.0;
        h.k_loss_kw_per_c = 0.0;
        h.draw_kw = 0.0;
    }
    let mut sim = make_snap_from_profile(&profile);
    set_heater_temp(&mut sim, 19.0);
    let target = crate::entities::device_session::HeaterTarget {
        mode: Default::default(),
        id: uuid::Uuid::new_v4(),
        target_temp_c: 20.0,
        ready_by: now + Duration::hours(1),
        comfort_rates: vec![],
        created_at: now,
        updated_at: now,
    };
    let tariffs = make_tariffs(0.25, 0.08, 300.0);
    let plan = run_planner(
        build_asset_contexts(&profile, &sim, now, None, Some(&target), &tariffs),
        &sim,
        &tariffs,
        &no_capacity(),
        &profile,
        now,
        crate::entities::asset::PlanTrigger::UserRequest,
        None,
        Some(&target),
        &[],
        None,
        None,
    );
    assert_opportunity_cost_convention(&plan, "heater");
}

#[test]
fn shiftable_allocation_cost_eur_prices_pv_surplus_as_opportunity_cost() {
    let now = noon();
    let mut profile = make_profile_1800s();
    profile
        .assets
        .retain(|a| matches!(a, AssetProfile::Pv(_) | AssetProfile::BaseLoad(_)));
    boost_pv(&mut profile, 20.0);
    let sim = make_snap_from_profile(&profile);
    let tariffs = make_tariffs(0.25, 0.08, 300.0);
    let load = crate::entities::device_session::ShiftableLoad {
        id: uuid::Uuid::new_v4(),
        asset_id: "wm".to_string(),
        power_kw: 2.0,
        duration_min: 30,
        earliest_start: now,
        latest_end: now + Duration::minutes(60),
        mode: Default::default(),
        created_at: now,
        updated_at: now,
    };
    let mut ctxs = build_asset_contexts(&profile, &sim, now, None, None, &tariffs);
    push_shiftable_load_contexts(&mut ctxs, std::slice::from_ref(&load), &profile, now);
    let plan = run_planner(
        ctxs,
        &sim,
        &tariffs,
        &no_capacity(),
        &profile,
        now,
        crate::entities::asset::PlanTrigger::UserRequest,
        None,
        None,
        &[load],
        None,
        None,
    );
    assert_opportunity_cost_convention(&plan, "wm");
}

#[test]
fn battery_charging_allocation_cost_eur_prices_pv_surplus_as_opportunity_cost() {
    let now = noon();
    let mut profile = make_profile_1800s();
    profile.assets.retain(|a| {
        matches!(
            a,
            AssetProfile::Battery(_) | AssetProfile::Pv(_) | AssetProfile::BaseLoad(_)
        )
    });
    boost_pv(&mut profile, 20.0);
    let sim = make_snap_from_profile(&profile);
    let tariffs = make_tariffs(0.25, 0.08, 300.0);
    let plan = run_planner(
        build_asset_contexts(&profile, &sim, now, None, None, &tariffs),
        &sim,
        &tariffs,
        &no_capacity(),
        &profile,
        now,
        crate::entities::asset::PlanTrigger::Periodic,
        None,
        None,
        &[],
        None,
        None,
    );
    assert_opportunity_cost_convention(&plan, "battery");
}

/// Cross-check per BL-40's own Verify note: the decision matrix's summed
/// `cost_eur` and the envelope's `solved_session_cost()`-style total (recomputed
/// directly from grid/surplus power, the reference convention) must agree in sign
/// for a plan spanning multiple asset types with PV-surplus-covered slots.
#[test]
fn decision_matrix_and_envelope_totals_agree_in_sign_across_asset_types() {
    let now = noon();
    let mut profile = make_profile_1800s();
    profile.assets.retain(|a| {
        matches!(
            a,
            AssetProfile::Ev(_)
                | AssetProfile::Heater(_)
                | AssetProfile::Battery(_)
                | AssetProfile::Pv(_)
                | AssetProfile::BaseLoad(_)
        )
    });
    boost_pv(&mut profile, 20.0);
    profile.assets = profile
        .assets
        .into_iter()
        .map(|a| match a {
            AssetProfile::Ev(mut ev) => {
                ev.battery_kwh = 10.0;
                AssetProfile::Ev(ev)
            }
            other => other,
        })
        .collect();
    if let Some(AssetProfile::Heater(ref mut h)) = profile
        .assets
        .iter_mut()
        .find(|a| matches!(a, AssetProfile::Heater(_)))
    {
        h.temp_initial_c = 19.0;
        h.k_loss_kw_per_c = 0.0;
        h.draw_kw = 0.0;
    }
    let mut sim = make_snap_from_profile(&profile);
    set_heater_temp(&mut sim, 19.0);
    set_ev_plugged(&mut sim, true);
    if let Some(ev) = sim.assets.get_mut("ev") {
        let bat_kwh = ev.val("battery_kwh").unwrap_or(60.0);
        let soc_target = ev.val("soc_target").unwrap_or(0.8);
        let max_ch = ev.val("max_charge_kw").unwrap_or(7.4);
        ev.values.insert("soc".into(), 0.1);
        ev.cap_max_import_kw = if 0.1_f64 >= soc_target { 0.0 } else { max_ch };
        ev.available_discharge_kwh = Some(0.1 * bat_kwh);
        ev.available_charge_kwh = Some(0.9 * bat_kwh);
    }
    let ev_session = crate::entities::device_session::EvSession {
        mode: Default::default(),
        id: uuid::Uuid::new_v4(),
        target_soc: 0.8,
        departure_time: now + Duration::hours(2),
        soft_deadline: false,
        budget_eur: None,
        comfort_rates: vec![],
        created_at: now,
        updated_at: now,
    };
    let heater_target = crate::entities::device_session::HeaterTarget {
        mode: Default::default(),
        id: uuid::Uuid::new_v4(),
        target_temp_c: 20.0,
        ready_by: now + Duration::hours(1),
        comfort_rates: vec![],
        created_at: now,
        updated_at: now,
    };
    let tariffs = make_tariffs(0.25, 0.08, 300.0);
    let plan = run_planner(
        build_asset_contexts(
            &profile,
            &sim,
            now,
            Some(&ev_session),
            Some(&heater_target),
            &tariffs,
        ),
        &sim,
        &tariffs,
        &no_capacity(),
        &profile,
        now,
        crate::entities::asset::PlanTrigger::UserRequest,
        Some(&ev_session),
        Some(&heater_target),
        &[],
        None,
        None,
    );

    // Decision-matrix total: sum of AssetAllocation.cost_eur as displayed on the Planner tab.
    let matrix_total: f64 = plan
        .slots
        .iter()
        .flat_map(|s| s.allocations.iter())
        .map(|a| a.cost_eur)
        .sum();

    // Reference total: recomputed directly from grid/surplus power using the
    // opportunity-cost convention (mirrors envelopes.rs::solved_session_cost()).
    let reference_total: f64 = plan
        .slots
        .iter()
        .flat_map(|s| {
            let dt_h = (s.end - s.start).num_seconds() as f64 / 3600.0;
            s.allocations.iter().map(move |a| {
                (a.grid_power_kw * s.import_tariff_eur_kwh
                    + a.surplus_power_kw * s.export_tariff_eur_kwh)
                    * dt_h
            })
        })
        .sum();

    assert!(
        matrix_total > 0.0 && reference_total > 0.0,
        "expected both totals positive (net grid+surplus cost) for this scenario: \
         matrix_total={matrix_total:.6}, reference_total={reference_total:.6}"
    );
    assert!(
        (matrix_total - reference_total).abs() < 1e-6,
        "decision matrix total {matrix_total:.6} must agree with the envelope-style \
         opportunity-cost total {reference_total:.6}"
    );
}
