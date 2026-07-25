mod port_tests {
    use super::super::*;

    fn _assert_send_sync<T: Send + Sync>() {}

    #[test]
    fn sim_state_is_send_sync() {
        _assert_send_sync::<SimState>();
    }

    #[test]
    fn snapshot_returns_ok_for_empty_state() {
        let sim = SimState::from_params(&[]);
        let result = SimulatorPort::snapshot(&sim);
        assert!(
            result.is_ok(),
            "snapshot() must succeed for a valid SimState"
        );
        let snap = result.unwrap();
        // Grid defaults are zero
        assert_eq!(snap.grid.net_power_w, 0.0);
    }
}

// `peek_pv_kw` tests — moved to tests/peek_pv_kw_tests.rs (own file, exempt from
// the file-size cap like other `tests/` subdirectory content) once this file
// approached the cap after adding the weather-suppression-decay regression tests.
mod peek_pv_kw_tests;

/// Simulated appliance noise on BaseLoad (coffee/cooking/TV bumps) — verifies
/// `SimState::tick` actually applies `BaseLoad::appliance_noise_kw`, not just
/// that the pure function itself behaves correctly in isolation (covered in
/// `assets::base_load::tests`).
mod base_load_noise_tests {
    use super::super::*;
    use crate::entities::asset_params::{ApplianceSpikeParams, AssetParams, BaseLoadParams};
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
        SimState::from_params(&[AssetParams::BaseLoad(BaseLoadParams {
            id: crate::ids::ASSET_BASE_LOAD.to_string(),
            baseline_kw,
            spikes: vec![coffee_spike()],
        })])
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
}

/// SC-002: Verify `GET /sim/schema` response is identical before and after the
/// pre-computation refactor.
///
/// Golden-file test: if `VEN/tests/fixtures/schema_snapshot.json` does not yet
/// exist the test creates it (first run = fixture generation) and passes.
/// On every subsequent run the test asserts byte-equality against the fixture.
mod schema_snapshot_tests {
    use super::super::schema_from_params;
    use std::path::PathBuf;

    fn fixture_path() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join("schema_snapshot.json")
    }

    fn profile_path() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("profiles")
            .join("ven-1.yaml")
    }

    #[test]
    fn schema_snapshot_matches_fixture() {
        let profile_yaml = std::fs::read_to_string(profile_path())
            .expect("ven-1.yaml must be readable for schema snapshot test");
        let profile: crate::profile::Profile =
            serde_yaml::from_str(&profile_yaml).expect("ven-1.yaml must parse as a valid Profile");

        let params = profile.asset_params();
        let schema = schema_from_params(&params);
        // Sort keys for deterministic JSON output
        let mut keys: Vec<_> = schema.keys().cloned().collect();
        keys.sort();
        let ordered: std::collections::BTreeMap<_, _> = keys
            .iter()
            .map(|k| (k.clone(), schema[k].clone()))
            .collect();
        let actual_json =
            serde_json::to_string_pretty(&ordered).expect("schema must be JSON-serialisable");

        let fixture = fixture_path();
        if !fixture.exists() {
            // First run: write the golden file and pass
            std::fs::create_dir_all(fixture.parent().unwrap())
                .expect("fixtures dir must be creatable");
            std::fs::write(&fixture, &actual_json).expect("fixture file must be writable");
            println!("schema_snapshot: fixture created at {}", fixture.display());
            return;
        }

        let expected_json = std::fs::read_to_string(&fixture)
            .expect("fixture file must be readable")
            .replace("\r\n", "\n");
        assert_eq!(
            actual_json, expected_json,
            "GET /sim/schema JSON has changed — update the fixture if the change is intentional"
        );
    }
}

/// R-20: unmodelled diurnal load on the derived grid meter — gives
/// `site-residual` a non-zero, learnable signal in simulation.
mod unmodelled_load_tests {
    use super::super::*;
    use crate::entities::asset_params::{AssetParams, BaseLoadParams};
    use chrono::TimeZone;

    fn base_only(baseline_kw: f64) -> SimState {
        SimState::from_params(&[AssetParams::BaseLoad(BaseLoadParams {
            id: crate::ids::ASSET_BASE_LOAD.to_string(),
            baseline_kw,
            spikes: vec![],
        })])
    }

    fn at(h: u32, m: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 7, 16, h, m, 0).unwrap()
    }

    fn run_tick(sim: &mut SimState, now: DateTime<Utc>) {
        sim.tick(
            30.0,
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
        );
    }

    #[test]
    fn unmodelled_load_at_is_zero_at_6h_and_peak_at_18h() {
        assert!(unmodelled_load_at(at(6, 0), 1.2).abs() < 1e-9);
        assert!((unmodelled_load_at(at(18, 0), 1.2) - 1.2).abs() < 1e-9);
        assert_eq!(unmodelled_load_at(at(18, 0), 0.0), 0.0, "0 peak disables");
        let noon = unmodelled_load_at(at(12, 0), 1.2);
        assert!(noon > 0.0 && noon < 1.2, "noon is between the extremes");
    }

    #[test]
    fn tick_meter_includes_unmodelled_load_making_residual_visible() {
        let mut sim = base_only(0.5);
        sim.unmodelled_load_kw = 2.0;
        run_tick(&mut sim, at(18, 0));

        let asset_sum_kw: f64 = sim.assets.iter().map(|e| e.last_power_kw).sum();
        let meter_kw = sim.grid.net_power_w / 1000.0;
        let residual_kw = meter_kw - asset_sum_kw;
        assert!(
            (residual_kw - 2.0).abs() < 1e-9,
            "at 18:00 the meter must exceed the asset sum by the full peak, got {residual_kw}"
        );

        // And the snapshot-level residual (what the heuristics learn from)
        // sees the same signal.
        let snap = sim.to_sim_snapshot();
        let snap_residual = crate::controller::residual::compute_site_residual_kw(&snap);
        assert!((snap_residual - 2.0).abs() < 1e-9);
    }

    #[test]
    fn tick_meter_equals_asset_sum_when_disabled() {
        let mut sim = base_only(0.5);
        run_tick(&mut sim, at(18, 0));
        let asset_sum_kw: f64 = sim.assets.iter().map(|e| e.last_power_kw).sum();
        let meter_kw = sim.grid.net_power_w / 1000.0;
        assert!(
            (meter_kw - asset_sum_kw).abs() < 1e-9,
            "default 0.0 peak must not change the derived meter"
        );
    }
}
