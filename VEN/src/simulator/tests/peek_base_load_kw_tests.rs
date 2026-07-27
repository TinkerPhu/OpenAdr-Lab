//! `SimState::peek_base_load_kw` — a read-only preview of this tick's
//! base-load output, closing the base-load half of the one-tick lag
//! `peek_pv_kw` already closes for PV. The anchor test proves peek() and
//! tick() can never silently diverge.

use super::super::*;
use crate::entities::asset_params::{AssetParams, BaseLoadParams, PvCurtailmentSource};
use chrono::TimeZone;

fn base_load_state(baseline_kw: f64) -> SimState {
    SimState::from_params(&[AssetParams::BaseLoad(BaseLoadParams {
        id: crate::ids::ASSET_BASE_LOAD.to_string(),
        baseline_kw,
        spikes: Vec::new(),
    })])
}

fn noon() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 7, 12, 12, 0, 0).unwrap()
}

#[test]
fn peek_base_load_kw_returns_none_without_base_load_asset() {
    let sim = SimState::from_params(&[]);
    assert_eq!(sim.peek_base_load_kw(noon(), 30.0, None, 0.1), None);
}

#[test]
fn peek_base_load_kw_matches_tick_output_for_same_now() {
    let mut sim = base_load_state(0.5);
    // A lingering perturbation offset (as if a slider was recently released),
    // so the decay branch is exercised, not just the flat profile.
    sim.base_load_smoothing.load_offset_kw = 1.2;

    let now = noon();
    let dt_s = 30.0;
    let base_load_alpha = 0.1;

    let preview = sim
        .peek_base_load_kw(now, dt_s, None, base_load_alpha)
        .expect("base_load asset is configured");

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
        base_load_alpha,
        None,
        None,
        None,
        None,
        None,
        None,
        PvCurtailmentSource::None,
    );

    let bl_entry = sim
        .assets
        .iter()
        .find(|e| e.id == crate::ids::ASSET_BASE_LOAD)
        .expect("base_load asset entry must exist");
    assert!(
        (bl_entry.last_power_kw - preview).abs() < 1e-9,
        "peek_base_load_kw ({preview}) must equal tick()'s actual base-load output ({}) for the \
         same `now` — any divergence reintroduces the one-tick lag this method exists to prevent",
        bl_entry.last_power_kw
    );
}

#[test]
fn peek_base_load_kw_override_bypasses_decay() {
    let mut sim = base_load_state(0.5);
    // A large lingering offset that would dominate the natural profile if it
    // were applied — the override must win outright, not blend with it.
    sim.base_load_smoothing.load_offset_kw = 5.0;

    let preview = sim
        .peek_base_load_kw(noon(), 30.0, Some(2.0), 0.1)
        .expect("base_load asset is configured");
    assert!(
        (preview - 2.0).abs() < 1e-9,
        "override=2.0 must yield exactly 2.0 kW, got {preview}"
    );
}

#[test]
fn peek_base_load_kw_decays_toward_natural_profile() {
    let mut sim = base_load_state(0.5);
    sim.base_load_smoothing.load_offset_kw = 2.0;

    let preview = sim
        .peek_base_load_kw(noon(), 30.0, None, 0.1)
        .expect("base_load asset is configured");
    assert!(
        preview > 0.5 && preview < 2.5,
        "decayed offset must land strictly between the natural profile and the full \
         un-decayed offset, got {preview}"
    );
}

#[test]
fn peek_base_load_kw_matches_tick_output_with_lingering_offset_for_same_now() {
    let mut sim = base_load_state(1.0);
    sim.base_load_smoothing.load_offset_kw = -0.8; // still decaying from a released override
    let now = noon();
    let dt_s = 30.0;

    let preview = sim
        .peek_base_load_kw(now, dt_s, None, 0.1)
        .expect("base_load asset is configured");

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
        None,
        None,
        None,
        None,
        PvCurtailmentSource::None,
    );

    let bl_entry = sim
        .assets
        .iter()
        .find(|e| e.id == crate::ids::ASSET_BASE_LOAD)
        .expect("base_load asset entry must exist");
    assert!(
        (bl_entry.last_power_kw - preview).abs() < 1e-9,
        "peek_base_load_kw ({preview}) must equal tick()'s actual base-load output ({}) with a \
         lingering decay offset, got divergence",
        bl_entry.last_power_kw
    );
}
