//! Simulated appliance noise on BaseLoad (coffee/cooking/TV bumps) — verifies
//! `SimState::tick` actually applies `BaseLoad::appliance_noise_kw`, not just
//! that the pure function itself behaves correctly in isolation (covered in
//! `assets::base_load::tests`). Also covers BL-40's 3-tier fallback
//! (measured → learned heuristic → synthetic) for `SimState::tick`'s
//! `BaseLoad` arm. Moved out of `tests.rs` (own file, exempt from the
//! file-size cap like `peek_pv_kw_tests.rs`/`peek_base_load_kw_tests.rs`)
//! once `tests.rs` crossed the cap after adding the BL-40 fallback tests.

use super::super::*;
use crate::entities::asset_params::{
    ApplianceSpikeParams, AssetParams, BaseLoadParams, PvCurtailmentSource,
};
use chrono::TimeZone;

/// A single coffee-time spike (matches the values this session's earlier
/// hardcoded `APPLIANCE_PATTERNS[0]` used, now supplied explicitly since
/// spikes are profile-configured rather than a built-in const).
fn coffee_spike() -> ApplianceSpikeParams {
    ApplianceSpikeParams {
        center_hour: 8.0,
        jitter_h: 0.05,
        amplitude_kw: 1.2,
        duration_h: 0.25,
        ramp_h: 0.03,
        probability: 1.0,
        weekdays: vec![],
    }
}

fn base_load_state(baseline_kw: f64) -> SimState {
    SimState::from_params(
        &[AssetParams::BaseLoad(BaseLoadParams {
            id: crate::ids::ASSET_BASE_LOAD.to_string(),
            baseline_kw,
            spikes: vec![coffee_spike()],
        })],
        Utc::now(),
    )
}

#[test]
fn tick_applies_appliance_noise_to_base_load_power() {
    let mut sim = base_load_state(0.3);
    let coffee_time = Utc.with_ymd_and_hms(2026, 7, 13, 8, 0, 0).unwrap();

    sim.tick(
        30.0,
        HashMap::new(),
        coffee_time,
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
        None, // base_load_heuristic_kw
    );

    let entry = sim
        .assets
        .iter()
        .find(|e| e.id == crate::ids::ASSET_BASE_LOAD)
        .expect("base_load asset entry must exist");
    assert!(
        entry.last_power_kw > 0.3,
        "8am tick should include a coffee-time appliance bump on top of the \
         0.3 kW static baseline, got {}",
        entry.last_power_kw
    );
}

#[test]
fn tick_base_load_kw_override_lands_exactly_regardless_of_appliance_noise() {
    // A forced override (e.g. the UI slider) must produce EXACTLY the
    // requested value, even during a coffee/cooking/TV bump window —
    // appliance noise must fold into the offset, not add on top of it.
    let mut sim = base_load_state(0.3);
    let coffee_time = Utc.with_ymd_and_hms(2026, 7, 13, 8, 0, 0).unwrap();

    sim.tick(
        30.0,
        HashMap::new(),
        coffee_time,
        None,
        0.1,
        None,
        None,
        None,
        Some(1.0), // base_load_kw_override
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
        None, // base_load_heuristic_kw
    );

    let entry = sim
        .assets
        .iter()
        .find(|e| e.id == crate::ids::ASSET_BASE_LOAD)
        .expect("base_load asset entry must exist");
    assert!(
        (entry.last_power_kw - 1.0).abs() < 1e-9,
        "override=1.0 must land exactly, even with appliance noise active, got {}",
        entry.last_power_kw
    );
}

#[test]
fn tick_at_quiet_hour_stays_close_to_static_baseline() {
    let mut sim = base_load_state(0.3);
    let quiet_time = Utc.with_ymd_and_hms(2026, 7, 13, 3, 0, 0).unwrap();

    sim.tick(
        30.0,
        HashMap::new(),
        quiet_time,
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
        None, // base_load_heuristic_kw
    );

    let entry = sim
        .assets
        .iter()
        .find(|e| e.id == crate::ids::ASSET_BASE_LOAD)
        .expect("base_load asset entry must exist");
    assert!(
        entry.last_power_kw < 0.35,
        "3am tick should be close to the 0.3 kW static baseline (no appliance \
         bump active), got {}",
        entry.last_power_kw
    );
}

// BL-40: 3-tier fallback (measured → learned heuristic → synthetic).
#[test]
fn tick_uses_heuristic_tier_when_measurement_absent_but_heuristic_present() {
    let mut sim = base_load_state(0.3);
    let coffee_time = Utc.with_ymd_and_hms(2026, 7, 13, 8, 0, 0).unwrap();
    let heuristic_kw = 9.9; // deliberately far from both synthetic and 0

    sim.tick(
        30.0,
        HashMap::new(),
        coffee_time,
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
        None,               // pv_measured_kw
        None,               // base_load_measured_kw
        Some(heuristic_kw), // base_load_heuristic_kw
    );

    let entry = sim
        .assets
        .iter()
        .find(|e| e.id == crate::ids::ASSET_BASE_LOAD)
        .expect("base_load asset entry must exist");
    assert!(
        (entry.last_power_kw - heuristic_kw).abs() < 1e-9,
        "with no measurement but a heuristic present, tick must use the heuristic \
         value ({heuristic_kw}), not the synthetic spike model, got {}",
        entry.last_power_kw
    );
}

#[test]
fn tick_falls_back_to_synthetic_when_neither_measurement_nor_heuristic_present() {
    let mut sim = base_load_state(0.3);
    let coffee_time = Utc.with_ymd_and_hms(2026, 7, 13, 8, 0, 0).unwrap();

    sim.tick(
        30.0,
        HashMap::new(),
        coffee_time,
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
        None, // base_load_heuristic_kw
    );

    let entry = sim
        .assets
        .iter()
        .find(|e| e.id == crate::ids::ASSET_BASE_LOAD)
        .expect("base_load asset entry must exist");
    assert!(
        entry.last_power_kw > 0.3,
        "cold start (no measurement, no heuristic) must still apply the synthetic \
         coffee-time appliance bump, got {}",
        entry.last_power_kw
    );
}
