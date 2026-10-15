//! Which emergency mode each heater runs this tick.

use std::collections::HashMap;

use crate::controller::SimSnapshot;
use crate::entities::sim_inject::SimInjectState;

/// Each heater's emergency mode this tick, `(curtail, absorb)` by asset id. Manual sim-inject
/// heater overrides win over the arbiter's decision (mirrors the existing "manual override
/// wins" precedent for PV smoothing) and apply to every asset answering the emergency
/// what-ifs; only when neither inject field is set does each heater get the mode the arbiter
/// chose for it (`arbiter_mode`, from `ArbiterOutcome::heater_emergency_mode`).
pub fn heater_emergency_modes(
    inject: &SimInjectState,
    arbiter_mode: &HashMap<String, (bool, bool)>,
    sim: &SimSnapshot,
) -> HashMap<String, (Option<bool>, Option<bool>)> {
    if inject.heater_emergency_curtail.is_some() || inject.heater_emergency_absorb.is_some() {
        let injected = (
            inject.heater_emergency_curtail,
            inject.heater_emergency_absorb,
        );
        sim.assets
            .iter()
            .filter(|(_, a)| a.emergency_what_ifs.is_some())
            .map(|(id, _)| (id.clone(), injected))
            .collect()
    } else {
        arbiter_mode
            .iter()
            .map(|(id, (curtail, absorb))| (id.clone(), (Some(*curtail), Some(*absorb))))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// W5a: an inject's emergency mode reaches every heater; otherwise each heater gets
    /// the mode the arbiter chose for it, and only that one.
    #[test]
    fn heater_emergency_modes_per_heater() {
        use crate::controller::sim_snapshot::GridSnapshot;
        let heater = |id: &str| {
            let snap = crate::services::test_support::asset_snapshots::snapshot_from_asset(
                &crate::assets::heater::Heater::from_params(&test_heater_params()),
                crate::assets::AssetState::Heater(crate::assets::heater::Heater::initial_state(
                    &test_heater_params(),
                )),
                "heater",
                0.0,
                0.0,
            );
            (id.to_string(), snap)
        };
        let sim = SimSnapshot {
            ts: chrono::Utc::now(),
            grid: GridSnapshot {
                net_power_kw: 0.0,
                voltage_v: 230.0,
                import_kwh: 0.0,
                export_kwh: 0.0,
                import_limit_kw: f64::MAX,
                export_limit_kw: -f64::MAX,
            },
            assets: HashMap::from([heater("heater-a"), heater("heater-b")]),
        };
        let arbiter = HashMap::from([("heater-a".to_string(), (true, false))]);

        let by_arbiter = heater_emergency_modes(&SimInjectState::default(), &arbiter, &sim);
        assert_eq!(by_arbiter.len(), 1);
        assert_eq!(by_arbiter["heater-a"], (Some(true), Some(false)));

        let inject = SimInjectState {
            heater_emergency_absorb: Some(true),
            ..Default::default()
        };
        let by_inject = heater_emergency_modes(&inject, &arbiter, &sim);
        assert_eq!(by_inject.len(), 2);
        assert_eq!(by_inject["heater-b"], (None, Some(true)));
    }

    fn test_heater_params() -> crate::entities::asset_params::HeaterParams {
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("profiles")
            .join("test.yaml");
        let profile: crate::profile::Profile =
            serde_yaml::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        profile
            .asset_params()
            .into_iter()
            .find_map(|p| match p {
                crate::entities::asset_params::AssetParams::Heater(h) => Some(h),
                _ => None,
            })
            .unwrap()
    }
}
