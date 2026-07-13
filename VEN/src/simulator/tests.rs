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

/// `SimState::peek_pv_kw` — a read-only preview of this tick's PV output,
/// added to fix the one-tick PV lag in `apply_surplus_ev_overlay` (found via
/// the phase 3+4 review's EV grid-residual toggle, 2026-07-12). The anchor
/// test proves peek() and tick() can never silently diverge.
mod peek_pv_kw_tests {
    use super::super::*;
    use crate::entities::asset_params::{AssetParams, PvParams};
    use chrono::TimeZone;

    fn pv_state(rated_kw: f64) -> SimState {
        SimState::from_params(&[AssetParams::Pv(PvParams {
            id: crate::ids::ASSET_PV.to_string(),
            rated_kw,
        })])
    }

    fn noon() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 7, 12, 12, 0, 0).unwrap()
    }

    #[test]
    fn peek_pv_kw_returns_none_without_pv_asset() {
        let sim = SimState::from_params(&[]);
        assert_eq!(sim.peek_pv_kw(noon(), 30.0, None, 0.1), None);
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
            .peek_pv_kw(now, dt_s, None, pv_alpha)
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
            .peek_pv_kw(noon(), 30.0, Some(0.5), 0.1)
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
            .peek_pv_kw(noon(), 30.0, Some(1.0), 0.1)
            .expect("PV asset is configured");
        assert!(
            (preview + 2.0).abs() < 1e-9,
            "export limit of -2.0 kW must clamp full-irradiance output, got {preview}"
        );
    }
}

/// Simulated appliance noise on BaseLoad (coffee/cooking/TV bumps) — verifies
/// `SimState::tick` actually applies `BaseLoad::appliance_noise_kw`, not just
/// that the pure function itself behaves correctly in isolation (covered in
/// `assets::base_load::tests`).
mod base_load_noise_tests {
    use super::super::*;
    use crate::entities::asset_params::{AssetParams, BaseLoadParams};
    use chrono::TimeZone;

    fn base_load_state(baseline_kw: f64) -> SimState {
        SimState::from_params(&[AssetParams::BaseLoad(BaseLoadParams {
            id: crate::ids::ASSET_BASE_LOAD.to_string(),
            baseline_kw,
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
