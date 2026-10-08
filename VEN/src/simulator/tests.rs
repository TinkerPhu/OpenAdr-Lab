/// `SimState::add_asset`/`remove_asset` (shiftable-load-as-asset design.md D3):
/// the only dynamic (not boot-fixed) mutation of the asset roster.
mod add_remove_asset_tests {
    use super::super::*;
    use crate::assets::ShiftableLoadAsset;

    fn shiftable_load(power_kw: f64, duration_min: u32) -> ShiftableLoadAsset {
        ShiftableLoadAsset {
            power_kw,
            duration_min,
            earliest_start: Utc::now(),
            latest_end: Utc::now() + chrono::Duration::hours(4),
        }
    }

    fn shiftable_entry(id: &str) -> (AssetEntry, Box<dyn Asset>) {
        let state = AssetState::ShiftableLoad(ShiftableLoadAsset::initial_state());
        (
            AssetEntry::new(id.to_string(), state, 0.0),
            Box::new(shiftable_load(2.0, 60)),
        )
    }

    #[test]
    fn add_shiftable_enters_the_roster_not_yet_started() {
        let mut sim = SimState::from_params(&[], Utc::now());
        sim.add_shiftable("wm", shiftable_load(2.0, 60)).unwrap();
        let (entry, cfg) = sim.find_asset("wm").expect("added to the roster");
        let AssetState::ShiftableLoad(state) = &entry.state else {
            panic!("a shiftable load holds shiftable-load state");
        };
        assert!(!state.started);
        assert_eq!((entry.setpoint_kw, entry.last_power_kw), (0.0, 0.0));
        assert!(cfg.is_cancellable(&entry.state));
    }

    #[test]
    fn add_shiftable_rejects_a_duplicate_id() {
        let mut sim = SimState::from_params(&[], Utc::now());
        sim.add_shiftable("wm", shiftable_load(2.0, 60)).unwrap();
        assert!(sim.add_shiftable("wm", shiftable_load(1.0, 30)).is_err());
        assert_eq!(sim.assets.len(), 1, "the duplicate must not be appended");
    }

    #[test]
    fn add_asset_appends_to_both_parallel_vectors() {
        let mut sim = SimState::from_params(&[], Utc::now());
        let (entry, config) = shiftable_entry("wm");
        assert!(sim.add_asset(entry, config).is_ok());
        assert_eq!(sim.assets.len(), 1);
        assert_eq!(sim.asset_configs.len(), 1);
        assert!(sim.find_asset("wm").is_some());
    }

    #[test]
    fn add_asset_rejects_duplicate_id() {
        let mut sim = SimState::from_params(&[], Utc::now());
        let (entry1, config1) = shiftable_entry("wm");
        let (entry2, config2) = shiftable_entry("wm");
        sim.add_asset(entry1, config1).unwrap();
        let result = sim.add_asset(entry2, config2);
        assert!(result.is_err(), "duplicate asset_id must be rejected");
        assert_eq!(sim.assets.len(), 1, "the duplicate must not be appended");
    }

    #[test]
    fn remove_asset_removes_from_both_parallel_vectors() {
        let mut sim = SimState::from_params(&[], Utc::now());
        let (entry, config) = shiftable_entry("wm");
        sim.add_asset(entry, config).unwrap();
        assert!(sim.remove_asset("wm"));
        assert!(sim.assets.is_empty());
        assert!(sim.asset_configs.is_empty());
    }

    #[test]
    fn remove_asset_is_a_no_op_when_id_not_present() {
        let mut sim = SimState::from_params(&[], Utc::now());
        assert!(!sim.remove_asset("does-not-exist"));
    }

    #[test]
    fn add_then_remove_keeps_other_assets_untouched() {
        let params = [crate::entities::asset_params::AssetParams::Battery(
            crate::entities::asset_params::BatteryParams::default(),
        )];
        let mut sim = SimState::from_params(&params, Utc::now());
        let (entry, config) = shiftable_entry("wm");
        sim.add_asset(entry, config).unwrap();
        assert_eq!(sim.assets.len(), 2);

        sim.remove_asset("wm");
        assert_eq!(sim.assets.len(), 1);
        assert!(sim.find_asset(crate::ids::ASSET_BATTERY).is_some());
    }
}

/// `SimState::tick()`'s generic post-step removal pass (design.md D3a of
/// shiftable-load-as-asset): a finished asset must disappear from the roster
/// with no per-kind branching in the removal pass itself.
mod shiftable_load_removal_tests {
    use super::super::*;
    use crate::assets::ShiftableLoadAsset;

    fn shiftable_only(power_kw: f64, duration_min: u32) -> SimState {
        let mut sim = SimState::from_params(&[], Utc::now());
        let load = ShiftableLoadAsset {
            power_kw,
            duration_min,
            earliest_start: Utc::now(),
            latest_end: Utc::now() + chrono::Duration::hours(4),
        };
        sim.add_shiftable("wm", load).unwrap();
        sim
    }

    fn run_tick(sim: &mut SimState, dt_s: f64, setpoints: HashMap<String, f64>) {
        sim.tick(TickInputs::new(dt_s, Utc::now(), setpoints));
    }

    #[test]
    fn finished_shiftable_load_is_removed_from_the_roster_the_same_tick() {
        let mut sim = shiftable_only(2.0, 1); // 1-minute duration
        run_tick(&mut sim, 60.0, HashMap::from([("wm".to_string(), 2.0)]));
        assert!(
            sim.find_asset("wm").is_none(),
            "finished shiftable load must be removed the same tick it finishes"
        );
    }

    #[test]
    fn pending_shiftable_load_is_not_removed() {
        let mut sim = shiftable_only(2.0, 60);
        run_tick(&mut sim, 30.0, HashMap::new());
        assert!(
            sim.find_asset("wm").is_some(),
            "a pending (not started) load must not be removed"
        );
    }

    #[test]
    fn running_but_not_yet_finished_shiftable_load_is_not_removed() {
        let mut sim = shiftable_only(2.0, 60);
        run_tick(&mut sim, 30.0, HashMap::from([("wm".to_string(), 2.0)]));
        assert!(
            sim.find_asset("wm").is_some(),
            "a running, not-yet-finished load must not be removed"
        );
    }
}

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

    use chrono::{Duration, TimeZone};
    use rand::{rngs::StdRng, SeedableRng};

    fn at(h: u32, m: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 7, 20, h, m, 0).unwrap()
    }

    fn run_tick(sim: &mut SimState, now: DateTime<Utc>) {
        sim.tick(TickInputs::new(30.0, now, HashMap::new()));
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
    use crate::entities::asset_params::{AssetParams, BaseLoadParams};
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
        sim.tick(TickInputs::new(30.0, now, HashMap::new()));
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
    use crate::entities::asset_params::{AssetParams, PvParams};

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
        sim.tick(TickInputs {
            pv_irradiance_override,
            weather_pv_kw,
            ..TickInputs::new(30.0, now, HashMap::new())
        });
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

/// `AssetSnapshot.values` is flattened into the same JSON object as the typed fields, so a
/// `state_values()` key named like a typed field silently overwrites it in `GET /sim` (R-116:
/// a shiftable load's rated `power_kw` hid its actual power). No asset kind may do that.
mod snapshot_values_tests {
    use super::super::*;
    use crate::assets::ShiftableLoadAsset;
    use crate::entities::asset_params::{
        AssetParams, BaseLoadParams, BatteryParams, EvParams, HeaterParams, PvParams,
    };

    #[test]
    fn typed_answers_are_carried_only_by_the_assets_that_have_them() {
        let params = [
            AssetParams::Battery(BatteryParams::default()),
            AssetParams::Ev(EvParams::default()),
            AssetParams::Heater(HeaterParams::default()),
            AssetParams::Pv(PvParams::default()),
            AssetParams::BaseLoad(BaseLoadParams::default()),
        ];
        let snapshot = SimState::from_params(&params, Utc::now()).to_sim_snapshot();
        for (id, asset) in &snapshot.assets {
            let is_heater = id == crate::ids::ASSET_HEATER;
            let is_pv = id == crate::ids::ASSET_PV;
            assert_eq!(
                asset.emergency_what_ifs.is_some(),
                is_heater,
                "what-ifs on '{id}'"
            );
            assert_eq!(asset.ac_ceiling_kw.is_some(), is_pv, "AC ceiling on '{id}'");
        }
    }

    #[test]
    fn no_asset_kind_reports_a_state_value_named_like_a_typed_snapshot_field() {
        let params = [
            AssetParams::Battery(BatteryParams::default()),
            AssetParams::Ev(EvParams::default()),
            AssetParams::Heater(HeaterParams::default()),
            AssetParams::Pv(PvParams::default()),
            AssetParams::BaseLoad(BaseLoadParams::default()),
        ];
        let mut sim = SimState::from_params(&params, Utc::now());
        let load = ShiftableLoadAsset {
            power_kw: 2.0,
            duration_min: 60,
            earliest_start: Utc::now(),
            latest_end: Utc::now() + chrono::Duration::hours(4),
        };
        sim.add_shiftable("wm", load).unwrap();

        let snapshot = sim.to_sim_snapshot();
        assert!(snapshot.assets.len() >= 6, "every asset kind is covered");
        for (id, asset) in &snapshot.assets {
            let mut typed = asset.clone();
            typed.values.clear();
            let typed = serde_json::to_value(&typed).unwrap();
            let typed_keys = typed.as_object().unwrap();
            for key in asset.values.keys() {
                assert!(
                    !typed_keys.contains_key(key),
                    "asset '{id}': state value '{key}' collides with the typed snapshot field of the same name"
                );
            }
        }
    }
}

/// What the sim records and what is injected into it from outside a tick (`recording.rs`).
mod recording_tests {
    use super::super::*;
    use crate::assets::LoadWindowStats;
    use crate::entities::asset_params::{AssetParams, BaseLoadParams, BatteryParams};

    fn sim() -> SimState {
        SimState::from_params(
            &[
                AssetParams::Battery(BatteryParams::default()),
                AssetParams::BaseLoad(BaseLoadParams::default()),
            ],
            Utc::now(),
        )
    }

    #[test]
    fn record_history_pushes_one_point_per_asset_with_its_power_and_state() {
        let mut sim = sim();
        sim.asset_mut(crate::ids::ASSET_BATTERY)
            .unwrap()
            .last_power_kw = 2.5;
        let now = Utc::now();

        sim.record_history(now, 10.0, -5.0);

        for entry in &sim.assets {
            assert_eq!(entry.history.len(), 1, "one point for {}", entry.id);
        }
        let battery = sim.asset(crate::ids::ASSET_BATTERY).unwrap();
        let point = battery.history.latest().expect("a recorded point");
        assert_eq!((point.ts, point.power_kw), (now, 2.5));
    }

    #[test]
    fn record_history_updates_the_grid_asset_in_kw_with_the_signed_limits() {
        let mut sim = sim();
        sim.grid.net_power_w = 3_000.0;

        sim.record_history(Utc::now(), 10.0, -5.0);

        let grid = &sim.grid_asset.state;
        assert_eq!(grid.net_power_kw, 3.0, "watts converted once, by the meter");
        assert_eq!((grid.import_limit_kw, grid.export_limit_kw), (10.0, -5.0));
    }

    #[test]
    fn record_history_appends_on_every_call() {
        let mut sim = sim();
        sim.record_history(Utc::now(), 10.0, -5.0);
        sim.record_history(Utc::now(), 10.0, -5.0);
        let battery = sim.asset(crate::ids::ASSET_BATTERY).unwrap();
        assert_eq!(battery.history.len(), 2);
    }

    #[test]
    fn set_base_load_observed_window_shows_up_in_the_base_loads_key_features() {
        let mut sim = sim();
        sim.set_base_load_observed_window(LoadWindowStats::from_power_kw([0.2, 0.4]));
        let (entry, cfg) = sim.find_asset(crate::ids::ASSET_BASE_LOAD).unwrap();
        let features = cfg.key_features(&entry.state);
        assert_eq!(features.len(), 2, "avg and max");
    }

    #[test]
    fn set_base_load_observed_window_is_a_no_op_without_a_base_load() {
        let mut sim = SimState::from_params(
            &[AssetParams::Battery(BatteryParams::default())],
            Utc::now(),
        );
        sim.set_base_load_observed_window(LoadWindowStats::from_power_kw([1.0]));
        assert_eq!(sim.assets.len(), 1);
    }
}
