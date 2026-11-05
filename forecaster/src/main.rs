//! Entry point of the lab-forecaster container. Wiring only; the decisions are in `lab_forecaster::engine`
//! and `lab_forecaster::dispatch`, the broker session in `lab_forecaster::runtime`.

use std::sync::Arc;

use chrono::Utc;
use lab_forecaster::config::Config;
use lab_forecaster::runtime::run;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();
    if let Err(reason) = start().await {
        tracing::error!("{reason}");
        std::process::exit(1);
    }
}

async fn start() -> Result<(), String> {
    let config = Config::from_lookup(&|name| std::env::var(name).ok())?;
    let profile_yaml = std::fs::read_to_string(&config.profile_path)
        .map_err(|e| format!("cannot read the profile {}: {e}", config.profile_path))?;
    run(config, &profile_yaml, Arc::new(Utc::now)).await
}
