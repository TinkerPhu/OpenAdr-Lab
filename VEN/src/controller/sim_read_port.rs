//! What the HTTP adapters read about the simulated assets, as plain data. Each method is one
//! short look at the live roster, answered by the assets themselves (`Asset::forecast`,
//! `capability`, `state_values`, ...); the implementation (`simulator::SimHandle`) takes the
//! simulator lock inside the call and releases it before returning, so a handler cannot hold it
//! across an `.await` and never sees `SimState`, `&dyn Asset` or an asset's state enum.

use async_trait::async_trait;
use chrono::{DateTime, Duration, Utc};
use lab_core::time_series::TimeSeries;

use crate::entities::asset::{AssetCapabilityView, AssetTraceRow, ComfortRate};
use crate::entities::asset_params::AssetRequestSlice;
use crate::entities::ev_usage::EvUsageSimState;
use crate::entities::timeline::TimelineSnapshot;

#[async_trait]
pub trait SimReadPort: Send + Sync {
    /// Domain-only snapshot for the timeline routes: per-asset recent history, the current state
    /// values and the Grid asset, all converted before the lock is released.
    async fn timeline_snapshot(&self, now: DateTime<Utc>) -> TimelineSnapshot;

    /// The asset's own forward-looking power series over `timespan`; `None` = unknown asset.
    async fn asset_forecast(
        &self,
        asset_id: &str,
        timespan: Duration,
        now: DateTime<Utc>,
    ) -> Option<TimeSeries>;

    /// The asset's feasible power range, flexibility floor and key features; `None` = unknown asset.
    async fn asset_capability(&self, asset_id: &str) -> Option<AssetCapabilityView>;

    /// The asset's recorded power over the last `timespan`, anchored by a last-observation-carried-
    /// forward point at `now - timespan`; `None` = unknown asset.
    async fn asset_history(
        &self,
        asset_id: &str,
        timespan: Duration,
        now: DateTime<Utc>,
    ) -> Option<TimeSeries>;

    /// The asset's recorded states over the last `window`, oldest first, each as its
    /// `state_values()` plus `power_kw`. Empty for an unknown asset.
    async fn asset_trace(
        &self,
        asset_id: &str,
        window: Duration,
        now: DateTime<Utc>,
    ) -> Vec<AssetTraceRow>;

    /// The asset's built-in comfort curve; `None` = unknown asset.
    async fn default_comfort_rates(&self, asset_id: &str) -> Option<Vec<ComfortRate>>;

    /// The EV's configured usage schedule; `None` when there is no EV or no schedule.
    async fn usage_schedule_view(&self, now: DateTime<Utc>) -> Option<EvUsageSimState>;

    /// One slice per asset for resolving a user request, each with the asset's own request
    /// defaults and its BUILT-IN comfort curve; a user's override is applied by the caller
    /// (`services::comfort::effective_comfort_rates`).
    async fn request_slices(&self) -> Vec<AssetRequestSlice>;
}
