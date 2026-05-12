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
    _sim_params: &crate::entities::planner_params::SimulatorParams,
    asset_params: &[crate::entities::asset_params::AssetParams],
) -> SimState {
    let fresh = SimState::from_params(asset_params);

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
