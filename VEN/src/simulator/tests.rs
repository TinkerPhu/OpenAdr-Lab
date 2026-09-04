mod port_tests {
    use super::super::*;

    fn _assert_send_sync<T: Send + Sync>() {}

    #[test]
    fn sim_state_is_send_sync() {
        _assert_send_sync::<SimState>();
    }

    #[test]
    fn snapshot_returns_ok_for_empty_state() {
        let sim = SimState::from_params(&[], Utc::now());
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

/// R-24: `SimState::from_params`'s `last_tick` and `derive_grid_meter`'s voltage
/// noise must come from injected sources (a `now` param and a seedable RNG),
/// not wall-clock `Utc::now()`/unseeded `thread_rng()` — otherwise repeated
/// runs of the same scenario are never bit-for-bit reproducible.
mod clock_and_rng_tests {
    use super::super::*;
    use crate::entities::asset_params::PvCurtailmentSource;
    use chrono::{Duration, TimeZone};
    use rand::{rngs::StdRng, SeedableRng};

    fn at(h: u32, m: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 7, 20, h, m, 0).unwrap()
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
            PvCurtailmentSource::None,
            None, // pv_measured_kw
            None, // base_load_measured_kw
            None, // base_load_heuristic_kw
        );
    }

    #[test]
    fn from_params_sets_last_tick_to_the_injected_now() {
        let now = at(9, 0);
        let sim = SimState::from_params(&[], now);
        assert_eq!(sim.last_tick, now);
    }

    #[test]
    fn same_seed_produces_identical_voltage_sequence_across_ticks() {
        let now = at(9, 0);
        let mut sim_a = SimState::from_params_seeded(&[], now, StdRng::seed_from_u64(7));
        let mut sim_b = SimState::from_params_seeded(&[], now, StdRng::seed_from_u64(7));

        let mut voltages_a = Vec::new();
        let mut voltages_b = Vec::new();
        for i in 1..=5 {
            let t = now + Duration::seconds(30 * i);
            run_tick(&mut sim_a, t);
            run_tick(&mut sim_b, t);
            voltages_a.push(sim_a.grid.voltage_v);
            voltages_b.push(sim_b.grid.voltage_v);
        }

        assert_eq!(
            voltages_a, voltages_b,
            "identically-seeded SimState instances must produce identical voltage sequences"
        );
    }

    #[test]
    fn different_seeds_produce_different_voltage_sequences() {
        let now = at(9, 0);
        let mut sim_a = SimState::from_params_seeded(&[], now, StdRng::seed_from_u64(1));
        let mut sim_b = SimState::from_params_seeded(&[], now, StdRng::seed_from_u64(2));

        let mut voltages_a = Vec::new();
        let mut voltages_b = Vec::new();
        for i in 1..=5 {
            let t = now + Duration::seconds(30 * i);
            run_tick(&mut sim_a, t);
            run_tick(&mut sim_b, t);
            voltages_a.push(sim_a.grid.voltage_v);
            voltages_b.push(sim_b.grid.voltage_v);
        }

        assert_ne!(
            voltages_a, voltages_b,
            "different seeds should (overwhelmingly likely) diverge"
        );
    }
}

// `peek_pv_kw` tests — moved to tests/peek_pv_kw_tests.rs (own file, exempt from
// the file-size cap like other `tests/` subdirectory content) once this file
// approached the cap after adding the weather-suppression-decay regression tests.
mod peek_pv_kw_tests;

// `peek_base_load_kw` tests — same rationale as peek_pv_kw_tests above.
mod peek_base_load_kw_tests;

// `base_load_noise_tests` — same rationale as peek_pv_kw_tests above; moved
// out once tests.rs crossed the cap after adding the BL-40 fallback tests.
mod base_load_noise_tests;

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

/// The simulated grid meter is exactly the sum of modelled asset power —
/// there is no separate meter perturbation. The site's unmetered consumption
/// is modelled as the `base_load` asset (see
/// `docs/architecture/forecasting_model.md`).
mod grid_meter_tests {
    use super::super::*;
    use crate::entities::asset_params::{AssetParams, BaseLoadParams, PvCurtailmentSource};
    use chrono::TimeZone;

    fn at(h: u32, m: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 7, 16, h, m, 0).unwrap()
    }

    fn base_only(baseline_kw: f64) -> SimState {
        SimState::from_params(
            &[AssetParams::BaseLoad(BaseLoadParams {
                id: crate::ids::ASSET_BASE_LOAD.to_string(),
                baseline_kw,
                spikes: vec![],
            })],
            at(0, 0),
        )
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
            PvCurtailmentSource::None,
            None, // pv_measured_kw
            None, // base_load_measured_kw
            None, // base_load_heuristic_kw
        );
    }

    #[test]
    fn tick_meter_equals_asset_sum() {
        let mut sim = base_only(0.5);
        run_tick(&mut sim, at(18, 0));
        let asset_sum_kw: f64 = sim.assets.iter().map(|e| e.last_power_kw).sum();
        let meter_kw = sim.grid.net_power_w / 1000.0;
        assert!(
            (meter_kw - asset_sum_kw).abs() < 1e-9,
            "the derived meter must be exactly the modelled-asset sum"
        );
    }
}

/// Regression for the production bug found on ven-1: a manual PV irradiance
/// override left weather fully suppressed for roughly an hour after release,
/// because `weather_power_kw` was nulled for as long as the decaying offset
/// hadn't reached exact zero. It must now stay visible immediately.
mod pv_weather_blend_tests {
    use super::super::*;
    use crate::entities::asset_params::{AssetParams, PvCurtailmentSource, PvParams};

    fn pv_only(rated_kw: f64) -> SimState {
        SimState::from_params(
            &[AssetParams::Pv(PvParams {
                id: crate::ids::ASSET_PV.to_string(),
                rated_kw,
                inverter_max_kw: rated_kw,
                co2_g_kwh: 0.0,
            })],
            Utc::now(),
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn run_tick(
        sim: &mut SimState,
        now: DateTime<Utc>,
        pv_irradiance_override: Option<f64>,
        weather_pv_kw: Option<f64>,
    ) {
        sim.tick(
            30.0,
            HashMap::new(),
            now,
            pv_irradiance_override,
            0.1,
            None,
            None,
            None,
            None,
            0.1,
            None,
            None,
            weather_pv_kw,
            None,
            None,
            None,
            PvCurtailmentSource::None,
            None, // pv_measured_kw
            None, // base_load_measured_kw
            None, // base_load_heuristic_kw
        );
    }

    #[test]
    fn weather_stays_visible_immediately_after_a_manual_override_is_released() {
        let mut sim = pv_only(10.0);
        let now = Utc::now();

        // Tick 1: manual override forced.
        run_tick(&mut sim, now, Some(0.9), Some(4.0));

        // Tick 2: override released (None) — weather is fresh and available,
        // but the just-released offset hasn't decayed to zero yet.
        run_tick(
            &mut sim,
            now + chrono::Duration::seconds(30),
            None,
            Some(4.0),
        );

        let pv_cfg = sim
            .asset_configs
            .iter()
            .find_map(|c| c.as_any().downcast_ref::<crate::assets::PvInverter>())
            .expect("pv asset config must exist");
        assert!(
            pv_cfg.weather_power_kw.is_some(),
            "weather_power_kw must not be nulled on the tick right after release, \
             even though the manual offset is still decaying"
        );
        assert!(
            !pv_cfg.irradiance_forced,
            "irradiance_forced must be false once the override is released"
        );
    }
}
