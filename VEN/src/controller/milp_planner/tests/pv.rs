use super::*;

// ── PV forecast reflects live irradiance_offset and tau_s (decay time
// constant, `pv-competence-consolidation` D7 — was `pv_alpha`) ─────────────

/// Return midnight so natural_irradiance_at() = 0, isolating the offset term.
fn fixed_midnight() -> DateTime<Utc> {
    use chrono::TimeZone;
    Utc.with_ymd_and_hms(2026, 4, 12, 0, 0, 0).unwrap()
}

/// Set irradiance_offset and tau_s on the PV asset in an existing SimSnapshot.
fn set_pv_inject(sim: &mut SimSnapshot, offset: f64, tau_s: f64) {
    let snap = sim.assets.get_mut("pv").expect("no pv asset in sim");
    snap.values.insert("irradiance_offset".to_string(), offset);
    snap.values.insert("tau_s".to_string(), tau_s);
}

/// `pv-competence-consolidation` section 4: build p_pv_kw the way production
/// now does — a live `SimState` with irradiance_offset/tau_s injected on its
/// `PvInverter`, resolved via `resolve_pv_forecast_kw` (the same helper
/// `tasks::planning::cycle::run_plan_cycle` calls) and threaded into
/// `build_milp_inputs` as `pv_live_forecast_kw`. Supersedes `set_pv_inject`'s
/// raw-SimSnapshot mutation for tests exercising the live-PV precedence path
/// specifically (`set_pv_inject`/`bmi` still cover the no-live-PV fallback).
fn bmi_with_live_pv(profile: &Profile, now: DateTime<Utc>, offset: f64, tau_s: f64) -> MilpInputs {
    use crate::simulator::plan_context::resolve_pv_forecast_kw;
    use crate::simulator::SimState;

    let mut sim_state = SimState::from_params(&profile.assets, now);
    {
        let (_, cfg) = sim_state.find_asset_mut("pv").expect("no pv asset in sim");
        let pv = cfg
            .as_any_mut()
            .downcast_mut::<crate::assets::PvInverter>()
            .expect("expected PvInverter");
        pv.irradiance_offset = offset;
        pv.tau_s = tau_s;
    }
    let n_slots: usize = profile.planner.plan_zones.iter().map(|z| z.slots).sum();
    let mut cum_s: Vec<i64> = Vec::with_capacity(n_slots + 1);
    cum_s.push(0);
    for zone in &profile.planner.plan_zones {
        for _ in 0..zone.slots {
            cum_s.push(cum_s.last().unwrap() + zone.step_s as i64);
        }
    }
    let pv_live_forecast_kw = resolve_pv_forecast_kw(&sim_state, n_slots, &cum_s, now);

    let sim_snap = sim_state.to_sim_snapshot();
    let ctxs: Vec<Box<dyn crate::controller::milp_planner::AssetMilpContext>> = vec![];
    super::super::inputs::build_milp_inputs(
        &ctxs,
        &sim_snap,
        &TariffTimeSeries::from_snapshots(&[]),
        &no_capacity(),
        &[],
        &[],
        &profile.planner,
        profile.grid.max_import_kw,
        profile.grid.max_export_kw,
        profile.pv_config(),
        None,
        now,
        None,
        None,
        pv_live_forecast_kw.as_deref(),
        &std::collections::HashMap::new(),
        None,
        None,
        None,
    )
}

#[test]
fn pv_irradiance_offset_in_forecast() {
    // Regression: irradiance_offset must project into p_pv_kw.
    // At midnight, natural irradiance = 0. With offset=0.5 and a very large
    // tau (≈no decay over the horizon), slot 0 must be ≈ 0.5 × rated_kw.
    let now = fixed_midnight();
    let profile = make_profile(); // rated_kw=5.0
    let inp = bmi_with_live_pv(&profile, now, 0.5, 1_000_000.0); // huge tau -> offset barely decays

    // slot 0: elapsed_s=0 -> decayed_offset = 0.5 * e^0 = 0.5
    // p_pv[0] = (0.0 + 0.5).clamp(0,1) × 5.0 = 2.5 kW
    assert!(
        inp.p_pv_kw[0] > 1.0,
        "p_pv_kw[0] should reflect irradiance_offset (got {:.4})",
        inp.p_pv_kw[0]
    );
}

#[test]
fn pv_irradiance_offset_decays_by_elapsed_seconds() {
    // Regression guard: the decay exponent must be real elapsed seconds
    // (`e^(-elapsed_s/tau_s)`), producing a smooth, monotonic fade — not a
    // per-plan-step count that could jump discontinuously across a
    // multi-zone horizon (the shape of bug this reparametrization removes).
    let now = fixed_midnight(); // natural=0, isolates offset
    let profile = make_profile(); // rated_kw=5.0, step_s=300
    let tau = -300.0_f64 / (1.0_f64 - 0.1).ln(); // equivalent to the old "typical alpha=0.1"
    let inp = bmi_with_live_pv(&profile, now, 0.5, tau);

    // slot 0 (elapsed_s=0): 0.5 × e^0 × 5.0 = 2.5 kW
    // slot 1 (elapsed_s=300): 0.5 × e^(-300/tau) × 5.0 -- must remain clearly non-zero.
    assert!(
        inp.p_pv_kw[1] > 1.0,
        "slot 1 must retain most of the offset after one 300s step, got {:.6}",
        inp.p_pv_kw[1]
    );
    // slot 5 (elapsed_s=1500): still a meaningful fraction of the original offset.
    assert!(
        inp.p_pv_kw[5] > 0.5,
        "slot 5 must still show partial offset, got {:.6}",
        inp.p_pv_kw[5]
    );
    // Decay is monotonically decreasing (offset fades over horizon)
    assert!(
        inp.p_pv_kw[1] < inp.p_pv_kw[0],
        "slot 1 must be less than slot 0 (offset decaying)"
    );
}

#[test]
fn pv_shorter_tau_decays_faster_in_forecast() {
    // Regression: a shorter tau_s (faster blend-back) must produce lower
    // p_pv_kw at later forecast slots because the offset decays faster.
    // At midnight natural=0, so all forecast power comes from the decaying offset.
    let now = fixed_midnight();
    let profile = make_profile(); // rated_kw=5.0, step_s=300s, 24 slots

    let inp_slow = bmi_with_live_pv(&profile, now, 0.5, 1_000_000.0); // huge tau: barely decays
    let inp_fast = bmi_with_live_pv(&profile, now, 0.5, 20.0); // tiny tau: decays almost immediately

    // At slot 3 (900s ahead at midnight, natural=0):
    //   slow (tau=1e6): 0.5 × e^(-900/1e6)  ≈ 0.5 × 0.9991 ≈ 2.50 kW
    //   fast (tau=20):  0.5 × e^(-900/20)   ≈ 0.5 × ~0     ≈ 0.00 kW
    let t = 3;
    assert!(
        inp_fast.p_pv_kw[t] < inp_slow.p_pv_kw[t],
        "a shorter tau_s should produce lower p_pv_kw at later slots: \
             fast={:.4} >= slow={:.4}",
        inp_fast.p_pv_kw[t],
        inp_slow.p_pv_kw[t]
    );
}

#[test]
fn pv_zero_offset_matches_sin_model() {
    // Backward compat: when irradiance_offset=0, p_pv_kw must equal the
    // profile's pure sin model (PvConfig::forecast_kw).
    let now = fixed_now(); // 06:00 → natural = 0 at slot 0
    let profile = make_profile(); // rated_kw=5.0, step_s=300s

    // from_profile initialises irradiance_offset=0 (tau_s irrelevant when offset=0)
    let sim = make_snap_from_profile(&profile);

    let inp = bmi(
        &profile,
        &sim,
        &TariffTimeSeries::from_snapshots(&[]),
        &no_capacity(),
        now,
        None,
        None,
    );

    // Compare every slot against the profile's sin model
    let pv_cfg = profile.pv_config().unwrap();
    for t in 0..inp.n {
        let slot_t = now + Duration::seconds(t as i64 * 300);
        let expected = pv_cfg.forecast_kw(slot_t);
        assert!(
            (inp.p_pv_kw[t] - expected).abs() < 1e-9,
            "slot {t}: zero-offset p_pv_kw should match sin model \
                 (got {:.6}, expected {:.6})",
            inp.p_pv_kw[t],
            expected
        );
    }
}

// ── pv_forecast_override (022-deterministic-test-env) ─────────────────────

#[test]
fn pv_forecast_override_zeros_all_slots() {
    // US1-AC-2: build_milp_inputs with pv_forecast_override=Some(0.0) must
    // produce p_pv_kw[t]=0 for every slot regardless of time-of-day or
    // irradiance_offset. Called twice → outputs are identical (deterministic).
    let now = fixed_now(); // 06:00 → non-zero natural irradiance during day
    let profile = make_profile(); // rated_kw=5.0, plan_horizon_h=2, plan_step_s=300
    let mut sim = make_snap_from_profile(&profile);
    set_pv_inject(&mut sim, 0.5, 0.1); // non-zero offset to ensure override wins

    let ctxs: Vec<Box<dyn crate::controller::milp_planner::AssetMilpContext>> = vec![];

    let inp1 = build_milp_inputs_with_override(
        &ctxs,
        &sim,
        &TariffTimeSeries::from_snapshots(&[]),
        &no_capacity(),
        &profile,
        now,
        &[],
        None,
        Some(0.0),
    );

    // All horizon slots must be 0.0
    for (t, &pv) in inp1.p_pv_kw.iter().enumerate() {
        assert!(
            pv.abs() < 1e-9,
            "slot {t}: expected p_pv_kw=0.0 with override=Some(0.0), got {pv:.6}"
        );
    }

    // Second call with same inputs must produce identical p_pv values (US1-AC-2)
    let inp2 = build_milp_inputs_with_override(
        &ctxs,
        &sim,
        &TariffTimeSeries::from_snapshots(&[]),
        &no_capacity(),
        &profile,
        now,
        &[],
        None,
        Some(0.0),
    );
    assert_eq!(inp1.n, inp2.n, "two identical calls must produce same n");
    for t in 0..inp1.n {
        assert!(
            (inp1.p_pv_kw[t] - inp2.p_pv_kw[t]).abs() < 1e-9,
            "slot {t}: second call differs: inp1={:.6} inp2={:.6}",
            inp1.p_pv_kw[t],
            inp2.p_pv_kw[t]
        );
    }
}

#[test]
fn pv_forecast_override_none_is_non_zero_during_day() {
    // Sanity: without override, p_pv at noon must be non-zero (natural irradiance).
    use chrono::TimeZone;
    let noon = Utc.with_ymd_and_hms(2026, 4, 11, 12, 0, 0).unwrap();
    let profile = make_profile(); // rated_kw=5.0
    let sim = make_snap_from_profile(&profile);
    let ctxs: Vec<Box<dyn crate::controller::milp_planner::AssetMilpContext>> = vec![];

    let inp = build_milp_inputs_with_override(
        &ctxs,
        &sim,
        &TariffTimeSeries::from_snapshots(&[]),
        &no_capacity(),
        &profile,
        noon,
        &[],
        None,
        None,
    );
    assert!(
        inp.p_pv_kw[0] > 0.0,
        "at noon with override=None, p_pv_kw[0] must be > 0 (got {:.4})",
        inp.p_pv_kw[0]
    );
}

// ── R-50: weather_pv_kw precedence ────────────────────────────────────────

/// Direct call to the real `inputs::build_milp_inputs` (not the
/// `build_milp_inputs_with_override` test wrapper, which hardcodes
/// `weather_pv_kw: None`) — needed to exercise the weather precedence branch.
#[allow(clippy::too_many_arguments)]
fn bmi_with_weather(
    ctxs: &[Box<dyn crate::controller::milp_planner::AssetMilpContext>],
    sim: &SimSnapshot,
    tariffs: &TariffTimeSeries,
    cap: &OadrCapacityState,
    profile: &Profile,
    now: DateTime<Utc>,
    pv_forecast_override: Option<f64>,
    weather_pv_kw: Option<&[f64]>,
) -> MilpInputs {
    super::super::inputs::build_milp_inputs(
        ctxs,
        sim,
        tariffs,
        cap,
        &[],
        &[],
        &profile.planner,
        profile.grid.max_import_kw,
        profile.grid.max_export_kw,
        profile.pv_config(),
        None,
        now,
        None,
        pv_forecast_override,
        None,
        &std::collections::HashMap::new(),
        weather_pv_kw,
        None,
        None,
    )
}

#[test]
fn weather_pv_kw_overrides_sin_model_fallback() {
    let now = fixed_midnight(); // natural sin-model irradiance = 0 at midnight
    let profile = make_profile(); // rated_kw=5.0
    let sim = make_snap_from_profile(&profile); // no live "pv" asset injected offset
    let ctxs: Vec<Box<dyn crate::controller::milp_planner::AssetMilpContext>> = vec![];

    let weather_kw = vec![3.5; 100]; // clearly non-zero, distinct from the midnight sin model
    let inp = bmi_with_weather(
        &ctxs,
        &sim,
        &TariffTimeSeries::from_snapshots(&[]),
        &no_capacity(),
        &profile,
        now,
        None,
        Some(&weather_kw),
    );
    assert!(
        (inp.p_pv_kw[0] - 3.5).abs() < 1e-9,
        "weather_pv_kw must override the midnight sin-model fallback (0.0), got {:.4}",
        inp.p_pv_kw[0]
    );
}

#[test]
fn weather_pv_kw_none_falls_back_to_sin_model() {
    let noon = {
        use chrono::TimeZone;
        Utc.with_ymd_and_hms(2026, 4, 11, 12, 0, 0).unwrap()
    };
    let profile = make_profile();
    let sim = make_snap_from_profile(&profile);
    let ctxs: Vec<Box<dyn crate::controller::milp_planner::AssetMilpContext>> = vec![];

    let inp = bmi_with_weather(
        &ctxs,
        &sim,
        &TariffTimeSeries::from_snapshots(&[]),
        &no_capacity(),
        &profile,
        noon,
        None,
        None,
    );
    assert!(
        inp.p_pv_kw[0] > 0.0,
        "weather_pv_kw=None must fall back to the sin model (non-zero at noon)"
    );
}

#[test]
fn pv_forecast_override_wins_over_weather_pv_kw() {
    let now = fixed_midnight();
    let profile = make_profile();
    let sim = make_snap_from_profile(&profile);
    let ctxs: Vec<Box<dyn crate::controller::milp_planner::AssetMilpContext>> = vec![];

    let weather_kw = vec![3.5; 100];
    let inp = bmi_with_weather(
        &ctxs,
        &sim,
        &TariffTimeSeries::from_snapshots(&[]),
        &no_capacity(),
        &profile,
        now,
        Some(0.0), // deterministic-testing pin
        Some(&weather_kw),
    );
    assert!(
        inp.p_pv_kw[0].abs() < 1e-9,
        "pv_forecast_override must win over weather_pv_kw, got {:.4}",
        inp.p_pv_kw[0]
    );
}

// ── PV-export decision variable (pv-export-curtailment) ────────────────────

fn capacity_with_export_limit_kw(limit_kw: f64) -> OadrCapacityState {
    OadrCapacityState {
        export_limit_kw: Some(limit_kw),
        ..no_capacity()
    }
}

/// PV + base load only — no battery/EV/heater to absorb surplus, so curtailment
/// is the only zero-cost way to relieve an export-capacity constraint.
fn make_pv_only_profile() -> Profile {
    let mut profile = make_profile_1800s();
    profile
        .assets
        .retain(|a| matches!(a, AssetProfile::Pv(_) | AssetProfile::BaseLoad(_)));
    profile
}

#[test]
fn pv_used_equals_forecast_when_no_export_constraint_binds() {
    use chrono::TimeZone;
    let noon = Utc.with_ymd_and_hms(2026, 4, 11, 12, 0, 0).unwrap();
    let profile = make_pv_only_profile();
    let sim = make_snap_from_profile(&profile);
    let tariffs = make_tariffs(0.25, 0.08, 300.0);
    let plan = run_planner(
        build_asset_contexts(&profile, &sim, noon, None, None, &tariffs),
        &sim,
        &tariffs,
        &no_capacity(),
        &profile,
        noon,
        crate::entities::asset::PlanTrigger::Periodic,
        None,
        None,
        &[],
        None,
        None,
    );
    for slot in &plan.slots {
        assert!(
            (slot.pv_used_kw - slot.pv_forecast_kw).abs() < 1e-6,
            "slot {}: expected no curtailment, pv_used_kw={:.4} pv_forecast_kw={:.4}",
            slot.slot_index,
            slot.pv_used_kw,
            slot.pv_forecast_kw
        );
    }
}

#[test]
fn export_cap_forces_pv_curtailment_without_soft_violation() {
    use chrono::TimeZone;
    let noon = Utc.with_ymd_and_hms(2026, 4, 11, 12, 0, 0).unwrap();
    let profile = make_pv_only_profile();
    let sim = make_snap_from_profile(&profile);
    let tariffs = make_tariffs(0.25, 0.08, 300.0);
    // Rated 5.0 kW PV, 0.5 kW base load → up to ~4.5 kW export at noon.
    // Cap far below that so curtailment is the only zero-cost relief.
    let capacity = capacity_with_export_limit_kw(1.0);
    let plan = run_planner(
        build_asset_contexts(&profile, &sim, noon, None, None, &tariffs),
        &sim,
        &tariffs,
        &capacity,
        &profile,
        noon,
        crate::entities::asset::PlanTrigger::Periodic,
        None,
        None,
        &[],
        None,
        None,
    );
    let noon_slot = plan
        .slots
        .iter()
        .find(|s| s.pv_forecast_kw > 2.0)
        .expect("at least one slot near noon must have a strong PV forecast");
    assert!(
        noon_slot.pv_used_kw < noon_slot.pv_forecast_kw - 0.1,
        "expected curtailment: pv_used_kw={:.4} pv_forecast_kw={:.4}",
        noon_slot.pv_used_kw,
        noon_slot.pv_forecast_kw
    );
    assert!(
        noon_slot.net_export_kw <= 1.05,
        "export must respect the 1.0 kW cap via curtailment, not soft violation, got {:.4}",
        noon_slot.net_export_kw
    );
}

#[test]
fn run_planner_pv_and_base_load_only_declares_pv_used_without_panic() {
    let now = fixed_now();
    let profile = make_pv_only_profile();
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
    assert_eq!(plan.slots.len(), 4, "plan must have 4 slots");
    for slot in &plan.slots {
        assert!(
            slot.pv_used_kw <= slot.pv_forecast_kw + 1e-9,
            "pv_used_kw must never exceed the forecast"
        );
    }
}
