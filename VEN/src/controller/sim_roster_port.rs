//! The simulator's asset roster as the application and adapter rings see it: add a shiftable
//! load, cancel one, reset an asset, change its configuration. Domain-typed on purpose - no
//! `&dyn Asset`, no `SimState` - so services and routes never reach into the simulator, and
//! the implementation (`simulator::SimHandle`) owns the lock: it takes it inside each call and
//! releases it before returning, so no caller can hold it across an `.await`.

use std::collections::HashMap;

use async_trait::async_trait;

use crate::entities::device_session::ShiftableLoad;
use crate::entities::DomainError;

/// What `SimRosterPort::cancel_if_cancellable` did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CancelOutcome {
    /// The roster holds no asset with that id; nothing was removed.
    NoSuchAsset,
    /// The asset says it can no longer be cancelled (a started shiftable load is
    /// non-interruptible); it was left in place.
    NotCancellable,
    /// The asset was cancellable and has been removed.
    Removed,
}

#[async_trait]
pub trait SimRosterPort: Send + Sync {
    /// Add a shiftable load to the roster at acceptance time, not yet started. Fails on a
    /// duplicate asset id.
    async fn add_shiftable(&self, load: &ShiftableLoad) -> Result<(), String>;

    /// Remove the asset if (and only if) it says it can still be cancelled. One call, not
    /// "ask, then remove": between two lock acquisitions a load could start.
    async fn cancel_if_cancellable(&self, asset_id: &str) -> CancelOutcome;

    /// Overwrite an asset's mutable state from `values` (the sim-inject UI), after the asset has
    /// checked them against its own limits (`Asset::validate_values`).
    async fn reset_asset(
        &self,
        asset_id: &str,
        values: HashMap<String, f64>,
    ) -> Result<(), DomainError>;

    /// Change an asset's configuration from `values`, after the asset has checked them.
    async fn update_asset_config(
        &self,
        asset_id: &str,
        values: HashMap<String, f64>,
    ) -> Result<(), DomainError>;
}
