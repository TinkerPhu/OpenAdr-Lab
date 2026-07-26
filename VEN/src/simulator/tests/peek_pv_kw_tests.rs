//! `SimState::peek_pv_kw` — a read-only preview of this tick's PV output,
//! added to fix the one-tick PV lag in `apply_surplus_ev_overlay` (found via
//! the phase 3+4 review's EV grid-residual toggle, 2026-07-12). The anchor
//! test proves peek() and tick() can never silently diverge.

use super::super::*;
use crate::entities::asset_params::{AssetParams, PvCurtailmentSource, PvParams};
use chrono::TimeZone;

fn pv_state(rated_kw: f64) -> SimState {
    SimState::from_params(&[AssetParams::Pv(PvParams {
        id: crate::ids::ASSET_PV.to_string(),
        rated_kw,
        inverter_max_kw: rated_kw,
    })])
}

fn noon() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 7, 12, 12, 0, 0).unwrap()
}

#[test]
fn peek_pv_kw_returns_none_without_pv_asset() {
    let sim = SimState::from_params(&[]);
    assert_eq!(sim.peek_pv_kw(noon(), 30.0, None, 0.1, None), None);
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
        .peek_pv_kw(now, dt_s, None, pv_alpha, None)
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
        .peek_pv_kw(noon(), 30.0, Some(0.5), 0.1, None)
        .expect("PV asset is configured");
    assert!(
        (preview + 5.0).abs() < 1e-9,
        "override=0.5 on a 10 kW array must yield -5.0 kW export, got {preview}"
    );
}

#[test]
fn peek_pv_kw_respects_export_limit_kw() {
    let mut sim = pv_state(10.0);
    if let Some(AssetConfig::Pv(pv)) = sim.asset_configs.first_mut() {
        pv.export_limit_kw = Some(-2.0);
    } else {
        panic!("expected a PV asset config");
    }

    let preview = sim
        .peek_pv_kw(noon(), 30.0, Some(1.0), 0.1, None)
        .expect("PV asset is configured");
    assert!(
        (preview + 2.0).abs() < 1e-9,
        "export limit of -2.0 kW must clamp full-irradiance output, got {preview}"
    );
}

#[test]
fn peek_pv_kw_uses_weather_when_no_manual_override() {
    let sim = pv_state(10.0); // sin model at noon would be near-full irradiance
    let preview = sim
        .peek_pv_kw(noon(), 30.0, None, 0.1, Some(4.2))
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
        .peek_pv_kw(noon(), 30.0, Some(0.5), 0.1, Some(4.2))
        .expect("PV asset is configured");
    assert!(
        (preview + 5.0).abs() < 1e-9,
        "manual sim inject must win over the weather value, got {preview}"
    );
}

#[test]
fn peek_pv_kw_manual_override_still_wins_while_offset_decaying() {
    // Regression: pv_irradiance is one-shot (auto-clears one tick after being
    // posted, tasks::sim_tick::tick.rs). Weather must stay suppressed for as
    // long as the resulting offset is still decaying, not just the tick the
    // override was posted on — else weather snaps back in on tick 2 while the
    // sin-model irradiance is still actively blending back from the override.
    let mut sim = pv_state(10.0);
    sim.pv_smoothing.irradiance_offset = -0.999; // still decaying from a released override
    let preview = sim
        .peek_pv_kw(noon(), 30.0, None, 0.1, Some(4.2))
        .expect("PV asset is configured");
    assert!(
        preview.abs() < 1.0,
        "weather (-4.2) must not be used while the override's offset is still decaying, got {preview}"
    );
}

#[test]
fn peek_pv_kw_matches_tick_output_with_weather_for_same_now() {
    let mut sim = pv_state(10.0);
    let now = noon();
    let dt_s = 30.0;

    let preview = sim
        .peek_pv_kw(now, dt_s, None, 0.1, Some(7.0))
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
fn tick_weather_stays_suppressed_one_tick_after_override_auto_clears() {
    // Regression, found live on Pi4 (2026-07-25): pv_irradiance is one-shot —
    // the caller (tasks::sim_tick::tick.rs) auto-clears it from SimInjectState
    // one tick after posting. Tick 1 correctly silences PV via the override.
    // Tick 2 passes `pv_irradiance_override: None` (already auto-cleared) while
    // the offset from tick 1 is still ≈ -1.0, actively decaying — weather must
    // stay suppressed through that decay, not snap back in immediately.
    let mut sim = pv_state(10.0);
    let now = noon();
    let dt_s = 1.0;

    sim.tick(
        dt_s,
        HashMap::new(),
        now,
        Some(0.0), // tick 1: override posted
        0.1,
        None,
        None,
        None,
        None,
        0.1,
        None,
        None,
        Some(7.0), // a large live weather value, must be ignored
        None,
        None,
        None,
        PvCurtailmentSource::None,
    );
    let pv_after_tick1 = sim
        .assets
        .iter()
        .find(|e| e.id == crate::ids::ASSET_PV)
        .unwrap()
        .last_power_kw;
    assert!(
        pv_after_tick1.abs() < 1e-6,
        "tick 1 must silence PV via the override, got {pv_after_tick1}"
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
        Some(7.0),
        None,
        None,
        None,
        PvCurtailmentSource::None,
    );
    let pv_after_tick2 = sim
        .assets
        .iter()
        .find(|e| e.id == crate::ids::ASSET_PV)
        .unwrap()
        .last_power_kw;
    assert!(
        pv_after_tick2.abs() < 1.0,
        "weather (-7.0) must not snap back in on tick 2 while the offset is still \
         decaying, got {pv_after_tick2}"
    );
}

// ── pv_export_limit_override (pv-export-curtailment) ────────────────────

#[test]
fn tick_applies_pv_export_limit_override_to_asset() {
    // Regression: PvInverter.export_limit_kw was never written by any live code
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
        Some(-3.0), // export limit: at most 3 kW export
        PvCurtailmentSource::None,
    );
    let pv_power = sim
        .assets
        .iter()
        .find(|e| e.id == crate::ids::ASSET_PV)
        .unwrap()
        .last_power_kw;
    assert!(
        (pv_power + 3.0).abs() < 1e-6,
        "export limit override must clamp PV output to -3.0 kW, got {pv_power}"
    );
}

#[test]
fn tick_clears_pv_export_limit_when_override_is_none() {
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
