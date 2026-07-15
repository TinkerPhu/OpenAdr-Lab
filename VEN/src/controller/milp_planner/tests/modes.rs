//! WP4.1-b (BL-28): UserRequestMode semantics in the EV session-intent translation.
//!
//! ASAP        — allocate at maximum feasible rate from now, cost-blind.
//! OPPORTUNISTIC — no deadline; allocate only where marginal cost ≈ 0
//!                 (PV surplus or non-positive tariff).

use super::*;
use crate::entities::design_vocabulary::UserRequestMode;

fn ev_session_with_mode(
    now: DateTime<Utc>,
    mode: UserRequestMode,
) -> crate::entities::device_session::EvSession {
    crate::entities::device_session::EvSession {
        id: uuid::Uuid::new_v4(),
        // soc 0.2 → 0.3 on 60 kWh = 6 kWh core; feasible within the 2 h horizon at 7.4 kW.
        target_soc: 0.3,
        departure_time: now + Duration::hours(2),
        soft_deadline: false,
        budget_eur: None,
        mode,
        created_at: now,
        updated_at: now,
    }
}

/// EV + base load only — no PV, no battery, so "free energy" only exists
/// where the import tariff is non-positive.
fn ev_only_profile() -> Profile {
    let mut p = make_profile_1800s();
    p.assets
        .retain(|a| matches!(a, AssetProfile::Ev(_) | AssetProfile::BaseLoad(_)));
    p
}

fn plan_ev_kw(plan: &crate::entities::plan::Plan) -> Vec<f64> {
    plan.slots
        .iter()
        .map(|s| {
            s.allocations
                .iter()
                .find(|a| a.asset_id == "ev")
                .map(|a| a.power_kw)
                .unwrap_or(0.0)
        })
        .collect()
}

fn solve_with_session(
    profile: &Profile,
    sim: &SimSnapshot,
    tariffs: &TariffTimeSeries,
    now: DateTime<Utc>,
    session: &crate::entities::device_session::EvSession,
) -> crate::entities::plan::Plan {
    run_planner(
        build_asset_contexts(profile, sim, now, Some(session), None, tariffs),
        sim,
        tariffs,
        &no_capacity(),
        profile,
        now,
        crate::entities::asset::PlanTrigger::UserRequest,
        Some(session),
        None,
        &[],
        None,
        None,
    )
}

/// BL-28 verify clause: same session parameters, distinguishably different
/// solver allocations between the two poles.
#[test]
fn test_mode_asap_vs_opportunistic_allocations_differ() {
    let now = fixed_now();
    let profile = ev_only_profile();
    let mut sim = make_snap_from_profile(&profile);
    set_ev_plugged(&mut sim, true);
    // Expensive first hour (slots 0–1), cheap afterwards (slots 2–3).
    let tariffs = make_two_zone_tariffs(0.40, 0.05);

    let asap = ev_session_with_mode(now, UserRequestMode::Asap);
    let plan_asap = solve_with_session(&profile, &sim, &tariffs, now, &asap);
    let opp = ev_session_with_mode(now, UserRequestMode::Opportunistic);
    let plan_opp = solve_with_session(&profile, &sim, &tariffs, now, &opp);

    let ev_asap = plan_ev_kw(&plan_asap);
    let ev_opp = plan_ev_kw(&plan_opp);
    assert!(
        ev_asap[0] > 1.0,
        "ASAP charges immediately despite the expensive window, got {ev_asap:?}"
    );
    assert!(
        ev_opp.iter().sum::<f64>() < 1e-3,
        "OPPORTUNISTIC finds no free energy (no PV, positive tariff) so plans nothing, got {ev_opp:?}"
    );
}

/// ASAP front-loads even when waiting would be much cheaper; BY_DEADLINE defers.
#[test]
fn test_mode_asap_charges_immediately_despite_cheaper_later() {
    let now = fixed_now();
    let profile = ev_only_profile();
    let mut sim = make_snap_from_profile(&profile);
    set_ev_plugged(&mut sim, true);
    let tariffs = make_two_zone_tariffs(0.40, 0.05);

    let bd = ev_session_with_mode(now, UserRequestMode::ByDeadline);
    let plan_bd = solve_with_session(&profile, &sim, &tariffs, now, &bd);
    let asap = ev_session_with_mode(now, UserRequestMode::Asap);
    let plan_asap = solve_with_session(&profile, &sim, &tariffs, now, &asap);

    let ev_bd = plan_ev_kw(&plan_bd);
    let ev_asap = plan_ev_kw(&plan_asap);
    // BY_DEADLINE waits for the cheap window for its core energy.
    assert!(
        ev_bd[0] < 1e-3,
        "BY_DEADLINE defers out of the 0.40 window, got {ev_bd:?}"
    );
    assert!(
        ev_bd[2] + ev_bd[3] > 1.0,
        "BY_DEADLINE charges in the cheap window, got {ev_bd:?}"
    );
    // ASAP is cost-blind: max feasible rate from slot 0.
    assert!(
        ev_asap[0] > 7.0,
        "ASAP charges at ~max rate immediately, got {ev_asap:?}"
    );
    // Both must still deliver the 6 kWh core by the deadline.
    let core_kwh = |ev: &[f64]| ev.iter().map(|p| p * 0.5).sum::<f64>();
    assert!(
        core_kwh(&ev_asap) >= 6.0 - 1e-6,
        "ASAP delivers the core energy, got {ev_asap:?}"
    );
    assert!(
        core_kwh(&ev_bd) >= 6.0 - 1e-6,
        "BY_DEADLINE delivers the core energy, got {ev_bd:?}"
    );
}

/// OPPORTUNISTIC charges in non-positive-tariff slots and nowhere else.
#[test]
fn test_mode_opportunistic_charges_only_in_free_slots() {
    let now = fixed_now();
    let profile = ev_only_profile();
    let mut sim = make_snap_from_profile(&profile);
    set_ev_plugged(&mut sim, true);
    // Positive first hour, negative afterwards (grid pays you to consume).
    let tariffs = make_two_zone_tariffs(0.30, -0.05);

    // Departure inside the positive window: a deadline-driven mode would be
    // forced to charge at 0.30 now; OPPORTUNISTIC ignores the deadline and
    // waits for the negative-tariff window instead.
    let mut opp = ev_session_with_mode(now, UserRequestMode::Opportunistic);
    opp.departure_time = now + Duration::hours(1);
    let plan = solve_with_session(&profile, &sim, &tariffs, now, &opp);
    let ev = plan_ev_kw(&plan);
    assert!(
        ev[0] < 1e-3 && ev[1] < 1e-3,
        "no charging while the tariff is positive, got {ev:?}"
    );
    assert!(
        ev[2] + ev[3] > 1.0,
        "charging happens in the negative-tariff window, got {ev:?}; warnings: {:?}",
        plan.warnings
    );
}

/// OPPORTUNISTIC charges from forecast PV surplus, capped by it.
#[test]
fn test_mode_opportunistic_charges_from_pv_surplus() {
    let now = fixed_now();
    let profile = ev_only_profile();
    let mut sim = make_snap_from_profile(&profile);
    set_ev_plugged(&mut sim, true);
    let tariffs = make_tariffs(0.30, 0.02, 300.0); // flat positive import, low feed-in

    let opp = ev_session_with_mode(now, UserRequestMode::Opportunistic);
    let plan = super::super::run_planner(
        build_asset_contexts(&profile, &sim, now, Some(&opp), None, &tariffs),
        &sim,
        &tariffs,
        &no_capacity(),
        &[],
        &[],
        &profile.planner,
        profile.grid.max_import_kw,
        profile.grid.max_export_kw,
        &profile.assets,
        now,
        crate::entities::asset::PlanTrigger::UserRequest,
        Some(&opp),
        None,
        &[],
        None,
        None,
        Some(2.5), // pv_forecast_override: 2.5 kW PV, 0.5 kW base → 2.0 kW surplus cap
        &std::collections::HashMap::new(),
    );
    let ev = plan_ev_kw(&plan);
    let total_kwh: f64 = ev.iter().map(|p| p * 0.5).sum();
    assert!(
        total_kwh > 1.0,
        "surplus PV is free energy — expect charging, got {ev:?}; warnings: {:?}",
        plan.warnings
    );
    // The 2.0 kW surplus cap can only deliver 4 kWh over the horizon — less
    // than the 6 kWh core a deadline-driven mode would force through the grid.
    for (t, &p) in ev.iter().enumerate() {
        assert!(
            p <= 2.0 + 1e-6,
            "slot {t}: charging must stay within the PV surplus cap, got {p}"
        );
    }
}

/// OPPORTUNISTIC has no deadline: mask stays open past the departure time.
#[test]
fn test_mode_opportunistic_has_no_deadline_constraint() {
    let now = fixed_now();
    let profile = ev_only_profile();
    let mut sim = make_snap_from_profile(&profile);
    set_ev_plugged(&mut sim, true);
    let mut session = ev_session_with_mode(now, UserRequestMode::Opportunistic);
    session.departure_time = now + Duration::minutes(30); // half the horizon

    let tariffs = make_tariffs(0.30, 0.08, 300.0);
    let ctxs = build_asset_contexts(&profile, &sim, now, Some(&session), None, &tariffs);
    let inp = build_milp_inputs(
        &ctxs,
        &sim,
        &tariffs,
        &no_capacity(),
        &profile,
        now,
        &[],
        None,
    );
    assert!(
        inp.a_ev.iter().all(|&v| v),
        "OPPORTUNISTIC ignores the departure deadline, mask {:?}",
        inp.a_ev
    );
    assert_eq!(inp.t_ev_dead_step, None);
    assert!(
        inp.e_ev_core_kwh < 1e-9,
        "OPPORTUNISTIC has no core obligation, got {}",
        inp.e_ev_core_kwh
    );
}

// ── WP4.1-c (BL-28 PR-c): MAX_COST + *_FREE variants ─────────────────────────

fn ev_session_with_budget(
    now: DateTime<Utc>,
    budget_eur: Option<f64>,
) -> crate::entities::device_session::EvSession {
    let mut s = ev_session_with_mode(now, UserRequestMode::MaxCost);
    s.budget_eur = budget_eur;
    s
}

/// MAX_COST with an insufficient budget charges only what the budget buys
/// and the plan carries the budget warning (→ WP4.3 notification).
#[test]
fn test_mode_max_cost_caps_spending_and_warns() {
    let now = fixed_now();
    let profile = ev_only_profile();
    let mut sim = make_snap_from_profile(&profile);
    set_ev_plugged(&mut sim, true);
    let tariffs = make_tariffs(0.30, 0.08, 300.0); // flat 0.30 → 6 kWh costs 1.80

    let session = ev_session_with_budget(now, Some(0.90)); // buys 3 kWh at 0.30
    let plan = solve_with_session(&profile, &sim, &tariffs, now, &session);
    let ev = plan_ev_kw(&plan);
    let charged_kwh: f64 = ev.iter().map(|p| p * 0.5).sum();
    assert!(
        charged_kwh > 2.5 && charged_kwh < 3.2,
        "budget 0.90 at 0.30/kWh buys ~3 kWh, got {charged_kwh} ({ev:?})"
    );
    assert!(
        plan.warnings.iter().any(|w| w.message.contains("budget")),
        "insufficient budget must surface a warning, got {:?}",
        plan.warnings
    );
}

/// MAX_COST with a sufficient budget completes the target without a warning.
#[test]
fn test_mode_max_cost_sufficient_budget_completes() {
    let now = fixed_now();
    let profile = ev_only_profile();
    let mut sim = make_snap_from_profile(&profile);
    set_ev_plugged(&mut sim, true);
    let tariffs = make_tariffs(0.30, 0.08, 300.0);

    let session = ev_session_with_budget(now, Some(5.0)); // 6 kWh costs 1.80 < 5
    let plan = solve_with_session(&profile, &sim, &tariffs, now, &session);
    let charged_kwh: f64 = plan_ev_kw(&plan).iter().map(|p| p * 0.5).sum();
    assert!(
        charged_kwh > 5.9,
        "sufficient budget reaches the 6 kWh target, got {charged_kwh}"
    );
    assert!(
        !plan.warnings.iter().any(|w| w.message.contains("budget")),
        "no budget warning when the target is affordable, got {:?}",
        plan.warnings
    );
}

/// BY_DEADLINE_FREE only charges free energy *inside* the deadline window.
#[test]
fn test_mode_by_deadline_free_respects_deadline_and_gate() {
    let now = fixed_now();
    let profile = ev_only_profile();
    let mut sim = make_snap_from_profile(&profile);
    set_ev_plugged(&mut sim, true);
    // Positive first hour, negative afterwards — but the deadline is +1 h,
    // so the free window lies OUTSIDE the deadline: nothing may charge.
    let tariffs = make_two_zone_tariffs(0.30, -0.05);
    let mut session = ev_session_with_mode(now, UserRequestMode::ByDeadlineFree);
    // 55 min, not 60: a departure exactly on a slot boundary includes the
    // boundary slot (established BY_DEADLINE semantic) - keep the deadline
    // strictly inside the positive window.
    session.departure_time = now + Duration::minutes(55);
    let plan = solve_with_session(&profile, &sim, &tariffs, now, &session);
    let ev = plan_ev_kw(&plan);
    assert!(
        ev.iter().sum::<f64>() < 1e-3,
        "no free energy inside the deadline → no charging, got {ev:?}"
    );

    // Deadline extended to +2 h: the negative window is now inside → charges there.
    session.departure_time = now + Duration::hours(2);
    let plan = solve_with_session(&profile, &sim, &tariffs, now, &session);
    let ev = plan_ev_kw(&plan);
    assert!(
        ev[0] < 1e-3 && ev[1] < 1e-3,
        "positive slots stay empty, got {ev:?}"
    );
    assert!(
        ev[2] + ev[3] > 1.0,
        "free slots inside the deadline are used, got {ev:?}"
    );
}

/// ASAP_FREE prefers the *earliest* free slots when the target fits in fewer.
#[test]
fn test_mode_asap_free_prefers_earliest_free_slots() {
    let now = fixed_now();
    let profile = ev_only_profile();
    let mut sim = make_snap_from_profile(&profile);
    set_ev_plugged(&mut sim, true);
    // All slots free (negative import) — the mode must still front-load.
    let tariffs = make_tariffs(-0.05, 0.08, 300.0);
    let mut session = ev_session_with_mode(now, UserRequestMode::AsapFree);
    // Target fits into a single slot: 0.2 → 0.2617 on 60 kWh ≈ 3.7 kWh = 7.4 kW × 0.5 h.
    session.target_soc = 0.2 + 3.7 / 60.0;
    let plan = solve_with_session(&profile, &sim, &tariffs, now, &session);
    let ev = plan_ev_kw(&plan);
    // Phase 2 may spend its friction budget (phase2_epsilon_eur) on ramp
    // smoothing, so the earliest slot is not necessarily saturated — the
    // mode invariant is: front-loaded (non-increasing), the earliest slot
    // carries the majority, and the full target lands in the early half.
    assert!(
        ev[0] > ev[1] && ev[1] >= ev[2] && ev[2] >= ev[3],
        "ASAP_FREE front-loads free energy, got {ev:?}"
    );
    assert!(
        ev[0] * 0.5 > 3.7 / 2.0,
        "the earliest slot carries the majority of the target, got {ev:?}"
    );
    let total_kwh: f64 = ev.iter().map(|p| p * 0.5).sum();
    assert!(
        (total_kwh - 3.7).abs() < 0.1,
        "the full target is delivered, got {total_kwh} kWh ({ev:?})"
    );
    assert!(
        ev[2] + ev[3] < 1e-3,
        "nothing left for the late slots, got {ev:?}"
    );
}

/// ASAP_FREE never buys non-free energy, however early.
#[test]
fn test_mode_asap_free_still_gated_to_free_energy() {
    let now = fixed_now();
    let profile = ev_only_profile();
    let mut sim = make_snap_from_profile(&profile);
    set_ev_plugged(&mut sim, true);
    let tariffs = make_tariffs(0.30, 0.08, 300.0); // flat positive, no PV
    let session = ev_session_with_mode(now, UserRequestMode::AsapFree);
    let plan = solve_with_session(&profile, &sim, &tariffs, now, &session);
    let ev = plan_ev_kw(&plan);
    assert!(
        ev.iter().sum::<f64>() < 1e-3,
        "no free energy anywhere → ASAP_FREE stays idle, got {ev:?}"
    );
}
