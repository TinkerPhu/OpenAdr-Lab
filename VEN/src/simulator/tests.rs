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
        assert!(result.is_ok(), "snapshot() must succeed for a valid SimState");
        let snap = result.unwrap();
        // Grid defaults are zero
        assert_eq!(snap.grid.net_power_w, 0.0);
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
            serde_yaml::from_str(&profile_yaml)
                .expect("ven-1.yaml must parse as a valid Profile");

        let params = profile.asset_params();
        let schema = schema_from_params(&params);
        // Sort keys for deterministic JSON output
        let mut keys: Vec<_> = schema.keys().cloned().collect();
        keys.sort();
        let ordered: std::collections::BTreeMap<_, _> =
            keys.iter().map(|k| (k.clone(), schema[k].clone())).collect();
        let actual_json = serde_json::to_string_pretty(&ordered)
            .expect("schema must be JSON-serialisable");

        let fixture = fixture_path();
        if !fixture.exists() {
            // First run: write the golden file and pass
            std::fs::create_dir_all(fixture.parent().unwrap())
                .expect("fixtures dir must be creatable");
            std::fs::write(&fixture, &actual_json)
                .expect("fixture file must be writable");
            println!("schema_snapshot: fixture created at {}", fixture.display());
            return;
        }

        let expected_json = std::fs::read_to_string(&fixture)
            .expect("fixture file must be readable");
        assert_eq!(
            actual_json, expected_json,
            "GET /sim/schema JSON has changed — update the fixture if the change is intentional"
        );
    }
}
