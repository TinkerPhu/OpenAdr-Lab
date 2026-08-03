//! `SimState::peek_pv_kw` — a read-only preview of this tick's PV output,
//! added to fix the one-tick PV lag in `apply_surplus_ev_overlay` (found via
//! the phase 3+4 review's EV grid-residual toggle, 2026-07-12). The anchor
//! test proves peek() and tick() can never silently diverge.

use super::super::*;
use crate::entities::asset_params::{AssetParams, PvCurtailmentSource, PvParams};
use chrono::TimeZone;

fn pv_state(rated_kw: f64) -> SimState {
    SimState::from_params(
        &[AssetParams::Pv(PvParams {
            id: crate::ids::ASSET_PV.to_string(),
            rated_kw,
            inverter_max_kw: rated_kw,
        })],
        noon(),
    )
}

fn noon() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 7, 12, 12, 0, 0).unwrap()
}

#[test]
fn peek_pv_kw_returns_none_without_pv_asset() {
    let sim = SimState::from_params(&[], noon());
    assert_eq!(sim.peek_pv_kw(noon(), 30.0, None, 0.1, None, None), None);
}

#[test]
fn peek_pv_kw_matches_tick_output_for_same_now() {
    let mut sim = pv_state(5.0);
    // A lingering perturbation offset (as if a slider was recently released),
    // so the decay branch — not just the pure sin model — is exercised.
    sim.pv_smoothing.irradiance_offset = 0.15;

    let now = noon();
    let dt_s = 30.0;
    let pv_alpha = 0.1;

    let preview = sim
        .peek_pv_kw(now, dt_s, None, pv_alpha, None, None)
        .expect("PV asset is configured");

    sim.tick(
        dt_s,
        HashMap::new(),
        now,
        None,
        pv_alpha,
        None,
        None,
        None,
        None,
        0.1,
        None,
        None,
        None,
        None,
        None,
        None,
        PvCurtailmentSource::None,
        None, // pv_measured_kw
        None, // base_load_measured_kw
    );

    let pv_entry = sim
        .assets
        .iter()
        .find(|e| e.id == crate::ids::ASSET_PV)
        .expect("PV asset entry must exist");
    assert!(
        (pv_entry.last_power_kw - preview).abs() < 1e-9,
        "peek_pv_kw ({preview}) must equal tick()'s actual PV output ({}) for the same `now` — \
         any divergence reintroduces the one-tick lag this method exists to prevent",
        pv_entry.last_power_kw
    );
}

#[test]
fn peek_pv_kw_override_bypasses_decay() {
    let mut sim = pv_state(10.0);
    // A large lingering offset that would dominate the natural model if it
    // were applied — the override must win outright, not blend with it.
    sim.pv_smoothing.irradiance_offset = 0.9;

    let preview = sim
        .peek_pv_kw(noon(), 30.0, Some(0.5), 0.1, None, None)
        .expect("PV asset is configured");
    assert!(
        (preview + 5.0).abs() < 1e-9,
        "override=0.5 on a 10 kW array must yield -5.0 kW export, got {preview}"
    );
}

#[test]
fn peek_pv_kw_respects_generation_limit_kw() {
    let mut sim = pv_state(10.0);
    if let Some(AssetConfig::Pv(pv)) = sim.asset_configs.first_mut() {
        pv.generation_limit_kw = Some(-2.0);
    } else {
        panic!("expected a PV asset config");
    }

    let preview = sim
        .peek_pv_kw(noon(), 30.0, Some(1.0), 0.1, None, None)
        .expect("PV asset is configured");
    assert!(
        (preview + 2.0).abs() < 1e-9,
        "generation limit of -2.0 kW must clamp full-irradiance output, got {preview}"
    );
}

#[test]
fn peek_pv_kw_uses_weather_when_no_manual_override() {
    let sim = pv_state(10.0); // sin model at noon would be near-full irradiance
    let preview = sim
        .peek_pv_kw(noon(), 30.0, None, 0.1, Some(4.2), None)
        .expect("PV asset is configured");
    assert!(
        (preview + 4.2).abs() < 1e-9,
        "weather value must override the sin model when no manual inject is active, got {preview}"
    );
}

#[test]
fn peek_pv_kw_manual_override_wins_over_weather() {
    let sim = pv_state(10.0);
    let preview = sim
        .peek_pv_kw(noon(), 30.0, Some(0.5), 0.1, Some(4.2), None)
        .expect("PV asset is configured");
    assert!(
        (preview + 5.0).abs() < 1e-9,
        "manual sim inject must win over the weather value, got {preview}"
    );
}

#[test]
fn peek_pv_kw_blends_decaying_offset_onto_weather_when_override_released() {
    // Regression for the production bug found live on ven-1: a released manual
    // override's decaying offset must NOT suppress weather entirely — it blends
    // additively on top of it instead (see PvInverter::step_inner).
    let mut sim = pv_state(10.0);
    sim.pv_smoothing.irradiance_offset = -0.1; // still decaying from a released override
    let preview = sim
        .peek_pv_kw(noon(), 30.0, None, 0.0, Some(4.2), None) // pv_alpha=0.0: no decay, offset stays exact
        .expect("PV asset is configured");
    assert!(
        (preview + 3.2).abs() < 1e-9,
        "expected weather(4.2) + offset(-0.1)*rated_kw(10.0) = -3.2 kW, got {preview}"
    );
}

#[test]
fn peek_pv_kw_matches_tick_output_with_weather_for_same_now() {
    let mut sim = pv_state(10.0);
    let now = noon();
    let dt_s = 30.0;

    let preview = sim
        .peek_pv_kw(now, dt_s, None, 0.1, Some(7.0), None)
        .expect("PV asset is configured");

    sim.tick(
        dt_s,
        HashMap::new(),
        now,
        None,
        0.1,
        None,
        None,
        None,
        None,
        0.1,
        None,
        None,
        Some(7.0),
        None,
        None,
        None,
        PvCurtailmentSource::None,
        None, // pv_measured_kw
        None, // base_load_measured_kw
    );

    let pv_entry = sim
        .assets
        .iter()
        .find(|e| e.id == crate::ids::ASSET_PV)
        .expect("PV asset entry must exist");
    assert!(
        (pv_entry.last_power_kw - preview).abs() < 1e-9,
        "peek_pv_kw ({preview}) must equal tick()'s actual PV output ({}) when a weather \
         value is supplied, same as the sin-model case",
        pv_entry.last_power_kw
    );
}

#[test]
fn tick_weather_visible_immediately_after_override_auto_clears() {
    // Regression, originally found live on Node1 (2026-07-25) and again on ven-1
    // in production (2026-08-02): pv_irradiance is one-shot — the caller
    // (tasks::sim_tick::tick.rs) auto-clears it from SimInjectState one tick
    // after posting. Tick 1 correctly silences PV via the override. Tick 2
    // passes `pv_irradiance_override: None` (already auto-cleared) with the
    // offset from tick 1 still actively decaying — weather must be visible on
    // tick 2 (blended with the residual offset), not suppressed entirely for
    // as long as the offset takes to fully decay (the bug: with a large enough
    // starting offset and the default pv_alpha≈0.1 EMA, that could take hours).
    // forced=0.9 (close to noon's natural≈1.0) keeps the resulting offset small,
    // so the tick-2 blend stays well under inverter_max_kw (=rated_kw=10.0 in
    // `pv_state`) rather than saturating at the hardware cap.
    let mut sim = pv_state(10.0);
    let now = noon();
    let dt_s = 1.0;

    sim.tick(
        dt_s,
        HashMap::new(),
        now,
        Some(0.9), // tick 1: override posted
        0.1,
        None,
        None,
        None,
        None,
        0.1,
        None,
        None,
        Some(5.0), // a live weather value, ignored this tick (forced override wins)
        None,
        None,
        None,
        PvCurtailmentSource::None,
        None, // pv_measured_kw
        None, // base_load_measured_kw
    );
    let pv_after_tick1 = sim
        .assets
        .iter()
        .find(|e| e.id == crate::ids::ASSET_PV)
        .unwrap()
        .last_power_kw;
    assert!(
        (pv_after_tick1 + 9.0).abs() < 1e-6,
        "tick 1: forced override=0.9 on a 10 kW array must yield -9.0 kW, got {pv_after_tick1}"
    );

    sim.tick(
        dt_s,
        HashMap::new(),
        now + chrono::Duration::seconds(1),
        None, // tick 2: caller already auto-cleared the one-shot override
        0.1,
        None,
        None,
        None,
        None,
        0.1,
        None,
        None,
        Some(5.0),
        None,
        None,
        None,
        PvCurtailmentSource::None,
        None, // pv_measured_kw
        None, // base_load_measured_kw
    );
    let pv_after_tick2 = sim
        .assets
        .iter()
        .find(|e| e.id == crate::ids::ASSET_PV)
        .unwrap()
        .last_power_kw;
    // Natural irradiance at noon ≈ 1.0, so offset after tick 1 ≈ 0.9 − 1.0 = −0.1,
    // barely decayed by tick 2 (dt_s=1s). Weather(5.0) + offset(≈−0.1)×rated_kw(10.0)
    // ≈ −4.0 kW — clearly visible (nowhere near the old ≈0 suppressed value, and
    // distinct from a raw, un-blended −5.0 kW weather value too).
    assert!(
        (pv_after_tick2 + 4.0).abs() < 0.01,
        "weather must be visible (blended with the residual offset) on the tick right \
         after release, not suppressed, got {pv_after_tick2}"
    );
}

// ── pv_generation_limit_override (pv-export-curtailment) ─────────────────

#[test]
fn tick_applies_pv_generation_limit_override_to_asset() {
    // Regression: PvInverter.generation_limit_kw was never written by any live code
    // path — only by unit tests directly — so VTN/plan-driven curtailment had no
    // physical effect. `tick()`'s new parameter must set it every tick.
    let mut sim = pv_state(10.0);
    let now = noon(); // full irradiance, would be -10.0 kW unclamped

    sim.tick(
        1.0,
        HashMap::new(),
        now,
        None,
        0.1,
        None,
        None,
        None,
        None,
        0.1,
        None,
        None,
        None,
        None,
        None,
        Some(-3.0), // generation limit: at most 3 kW export
        PvCurtailmentSource::None,
        None, // pv_measured_kw
        None, // base_load_measured_kw
    );
    let pv_power = sim
        .assets
        .iter()
        .find(|e| e.id == crate::ids::ASSET_PV)
        .unwrap()
        .last_power_kw;
    assert!(
        (pv_power + 3.0).abs() < 1e-6,
        "generation limit override must clamp PV output to -3.0 kW, got {pv_power}"
    );
}

#[test]
fn tick_clears_pv_generation_limit_when_override_is_none() {
    let mut sim = pv_state(10.0);
    let now = noon();

    // Tick 1: limit active.
    sim.tick(
        1.0,
        HashMap::new(),
        now,
        None,
        0.1,
        None,
        None,
        None,
        None,
        0.1,
        None,
        None,
        None,
        None,
        None,
        Some(-2.0),
        PvCurtailmentSource::None,
        None, // pv_measured_kw
        None, // base_load_measured_kw
    );
    // Tick 2: no active limit — PV must return to unclamped output.
    sim.tick(
        1.0,
        HashMap::new(),
        now + chrono::Duration::seconds(1),
        None,
        0.1,
        None,
        None,
        None,
        None,
        0.1,
        None,
        None,
        None,
        None,
        None,
        None,
        PvCurtailmentSource::None,
        None, // pv_measured_kw
        None, // base_load_measured_kw
    );
    let pv_power = sim
        .assets
        .iter()
        .find(|e| e.id == crate::ids::ASSET_PV)
        .unwrap()
        .last_power_kw;
    assert!(
        pv_power < -9.0,
        "with no active limit, PV must be unclamped (~-10.0 kW at noon), got {pv_power}"
    );
}
