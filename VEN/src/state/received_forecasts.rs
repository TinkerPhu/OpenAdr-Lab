//! `AppState` accessors for the forecasts received from the lab-forecaster (`lab_forecast::received_forecasts`):
//! written by the `forecast_feed` subscriber task, read per tick by the feed resolution and by the routes
//! that show where a series came from.

use chrono::{DateTime, Utc};
use lab_forecast::received_forecasts::ReceivedForecasts;

use super::AppState;

impl AppState {
    pub async fn received_forecasts(&self) -> ReceivedForecasts {
        self.hems.read().await.received_forecasts.clone()
    }

    /// The base-load asset the received series belongs to (set once when the subscriber is wired).
    pub async fn set_received_base_load_asset(&self, asset_id: Option<String>) {
        self.hems.write().await.received_forecasts.base_load_asset = asset_id;
    }

    pub async fn apply_received_forecast(&self, key: &str, payload: &[u8], now: DateTime<Utc>) {
        self.hems
            .write()
            .await
            .received_forecasts
            .apply(key, payload, now);
    }
}
