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
