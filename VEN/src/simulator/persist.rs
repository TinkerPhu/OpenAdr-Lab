use std::path::Path;
use tracing::{error, info, warn};

use super::SimState;

const SIM_STATE_FILE: &str = "sim_state.json";

/// Get the sim state file path within the data directory.
fn sim_path(data_dir: &str) -> String {
    format!("{}/{}", data_dir.trim_end_matches('/'), SIM_STATE_FILE)
}

/// Save sim state to disk. Uses atomic write (temp file + rename).
pub async fn save(state: &SimState, data_dir: &str) -> anyhow::Result<()> {
    let path = sim_path(data_dir);
    let tmp_path = format!("{}.tmp", path);
    let json = serde_json::to_string_pretty(state)?;
    tokio::fs::write(&tmp_path, &json).await?;
    tokio::fs::rename(&tmp_path, &path).await?;
    Ok(())
}

/// Load persisted sim state and replace asset configs from the current params.
///
/// Only mutable runtime state (temperatures, SoCs, energy counters, last_tick) is
/// restored from disk. Asset configs (thermal_mass, k_loss, max_kw, etc.) are always
/// rebuilt from the current params so that configuration changes take effect on restart.
///
/// Falls back to a fresh params-based state when the file is missing, corrupt, or
/// when the persisted asset IDs don't match the current asset list.
pub async fn load_with_params(
    data_dir: &str,
    sim_params: &crate::entities::planner_params::SimulatorParams,
    asset_params: &[crate::entities::asset_params::AssetParams],
    now: chrono::DateTime<chrono::Utc>,
) -> SimState {
    let mut fresh = SimState::from_params(asset_params, now);
    fresh.unmodelled_load_kw = sim_params.unmodelled_load_kw;

    let Some(mut loaded) = load(data_dir).await else {
        return fresh;
    };

    let current_ids: Vec<&str> = fresh.assets.iter().map(|e| e.id.as_str()).collect();
    let loaded_ids: Vec<&str> = loaded.assets.iter().map(|e| e.id.as_str()).collect();
    if current_ids != loaded_ids {
        warn!(
            ?current_ids,
            ?loaded_ids,
            "asset list changed since last persist — starting fresh from params"
        );
        return fresh;
    }

    loaded.asset_configs = fresh.asset_configs;
    loaded.unmodelled_load_kw = fresh.unmodelled_load_kw;
    loaded
}

/// Load sim state from disk. Returns None if file missing or corrupt.
pub async fn load(data_dir: &str) -> Option<SimState> {
    let path = sim_path(data_dir);
    if !Path::new(&path).exists() {
        info!(path, "no sim state file found, starting fresh");
        return None;
    }

    match tokio::fs::read_to_string(&path).await {
        Ok(json) => match serde_json::from_str(&json) {
            Ok(state) => {
                info!(path, "loaded sim state from disk");
                Some(state)
            }
            Err(e) => {
                warn!(path, error = %e, "corrupt sim state file, starting fresh");
                None
            }
        },
        Err(e) => {
            error!(path, error = %e, "failed to read sim state file");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entities::asset_params::{AssetParams, BatteryParams};
    use crate::entities::planner_params::SimulatorParams;
    use chrono::{TimeZone, Utc};

    fn temp_data_dir() -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("ven_sim_persist_test_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn now() -> chrono::DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 8, 14, 12, 0, 0).unwrap()
    }

    fn battery_params(id: &str) -> AssetParams {
        AssetParams::Battery(BatteryParams {
            id: id.to_string(),
            capacity_kwh: 10.0,
            max_charge_kw: 3.0,
            max_discharge_kw: 3.0,
            initial_soc: 0.5,
            round_trip_efficiency: 0.95,
            min_soc: 0.1,
            c_terminal_eur_kwh: None,
        })
    }

    #[tokio::test]
    async fn save_then_load_round_trip_restores_mutable_state() {
        let dir = temp_data_dir();
        let data_dir = dir.to_str().unwrap();

        let mut state = SimState::from_params(&[battery_params("battery")], now());
        let (entry, _) = state.find_asset_mut("battery").unwrap();
        entry.state = crate::assets::AssetState::Battery(crate::assets::BatteryState {
            soc: 0.73,
            actual_power_kw: 1.5,
        });

        save(&state, data_dir).await.unwrap();
        let loaded = load(data_dir).await.expect("must load what was just saved");

        let (loaded_entry, _) = loaded.find_asset("battery").unwrap();
        match &loaded_entry.state {
            crate::assets::AssetState::Battery(s) => {
                assert!((s.soc - 0.73).abs() < 1e-9, "soc must survive round-trip");
                assert!((s.actual_power_kw - 1.5).abs() < 1e-9);
            }
            other => panic!("expected Battery state, got {other:?}"),
        }

        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn load_missing_file_returns_none() {
        let dir = temp_data_dir();
        let result = load(dir.to_str().unwrap()).await;
        assert!(result.is_none(), "no file written yet — must be None");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn load_corrupt_file_returns_none_not_panic() {
        let dir = temp_data_dir();
        let data_dir = dir.to_str().unwrap();
        tokio::fs::write(dir.join(SIM_STATE_FILE), b"{ not valid json ]")
            .await
            .unwrap();

        let result = load(data_dir).await;
        assert!(
            result.is_none(),
            "corrupt file must fall back to None, not panic or error out"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn load_with_params_returns_fresh_state_when_no_file_exists() {
        let dir = temp_data_dir();
        let data_dir = dir.to_str().unwrap();
        let sim_params = SimulatorParams {
            unmodelled_load_kw: 2.5,
            ..Default::default()
        };
        let asset_params = [battery_params("battery")];

        let state = load_with_params(data_dir, &sim_params, &asset_params, now()).await;

        assert_eq!(state.assets.len(), 1);
        assert_eq!(state.assets[0].id, "battery");
        assert!((state.unmodelled_load_kw - 2.5).abs() < 1e-9);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn load_with_params_restores_mutable_state_but_rebuilds_configs_from_current_params() {
        let dir = temp_data_dir();
        let data_dir = dir.to_str().unwrap();

        // Save state built from an "old" config (capacity_kwh = 10.0), with a
        // mutated SoC that must survive the restart.
        let mut saved = SimState::from_params(&[battery_params("battery")], now());
        let (entry, _) = saved.find_asset_mut("battery").unwrap();
        entry.state = crate::assets::AssetState::Battery(crate::assets::BatteryState {
            soc: 0.42,
            actual_power_kw: 0.0,
        });
        save(&saved, data_dir).await.unwrap();

        // Restart with a *changed* profile — capacity_kwh bumped from 10.0 to 20.0 —
        // exercising the doc comment's claim that configs are always rebuilt from
        // current params, never taken from the persisted file.
        let mut new_params = match battery_params("battery") {
            AssetParams::Battery(p) => p,
            _ => unreachable!(),
        };
        new_params.capacity_kwh = 20.0;
        let asset_params = [AssetParams::Battery(new_params)];
        let sim_params = SimulatorParams::default();

        let restarted = load_with_params(data_dir, &sim_params, &asset_params, now()).await;

        let (entry, cfg) = restarted.find_asset("battery").unwrap();
        match &entry.state {
            crate::assets::AssetState::Battery(s) => {
                assert!(
                    (s.soc - 0.42).abs() < 1e-9,
                    "mutable SoC must be restored from disk"
                );
            }
            other => panic!("expected Battery state, got {other:?}"),
        }
        match cfg {
            crate::assets::AssetConfig::Battery(b) => assert!(
                (b.capacity_kwh - 20.0).abs() < 1e-9,
                "config must come from the *current* params, not the persisted file"
            ),
            other => panic!("expected Battery config, got {other:?}"),
        }

        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn load_with_params_falls_back_to_fresh_when_asset_ids_changed() {
        let dir = temp_data_dir();
        let data_dir = dir.to_str().unwrap();

        let mut saved = SimState::from_params(&[battery_params("battery-old")], now());
        let (entry, _) = saved.find_asset_mut("battery-old").unwrap();
        entry.state = crate::assets::AssetState::Battery(crate::assets::BatteryState {
            soc: 0.99,
            actual_power_kw: 0.0,
        });
        save(&saved, data_dir).await.unwrap();

        // Restart with a *different* asset id — simulates a profile change that
        // adds/removes/renames an asset since the last persist.
        let asset_params = [battery_params("battery-new")];
        let sim_params = SimulatorParams::default();

        let restarted = load_with_params(data_dir, &sim_params, &asset_params, now()).await;

        assert!(
            restarted.find_asset("battery-old").is_none(),
            "stale asset id from disk must not leak into the fresh state"
        );
        let (entry, _) = restarted
            .find_asset("battery-new")
            .expect("must fall back to a fresh state built from current params");
        match &entry.state {
            crate::assets::AssetState::Battery(s) => assert!(
                (s.soc - 0.5).abs() < 1e-9,
                "fresh state must use initial_soc from params, not the stale 0.99 on disk"
            ),
            other => panic!("expected Battery state, got {other:?}"),
        }

        std::fs::remove_dir_all(&dir).ok();
    }
}
