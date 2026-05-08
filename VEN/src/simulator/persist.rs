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

/// Load persisted sim state and replace asset configs from the current profile.
///
/// Only mutable runtime state (temperatures, SoCs, energy counters, last_tick) is
/// restored from disk. Asset configs (thermal_mass, k_loss, max_kw, etc.) are always
/// rebuilt from the profile so that profile changes take effect on every restart.
///
/// Falls back to a fresh profile-based state when the file is missing, corrupt, or
/// when the persisted asset IDs don't match the profile (assets were added/removed).
pub async fn load_with_profile(data_dir: &str, profile: &crate::profile::Profile) -> SimState {
    let fresh = SimState::from_profile(profile);

    let Some(mut loaded) = load(data_dir).await else {
        return fresh;
    };

    let profile_ids: Vec<&str> = fresh.assets.iter().map(|e| e.id.as_str()).collect();
    let loaded_ids: Vec<&str> = loaded.assets.iter().map(|e| e.id.as_str()).collect();
    if profile_ids != loaded_ids {
        warn!(
            ?profile_ids,
            ?loaded_ids,
            "asset list changed since last persist — starting fresh from profile"
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
