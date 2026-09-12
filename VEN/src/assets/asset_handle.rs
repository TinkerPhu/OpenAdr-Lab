//! `AssetHandle` — split out of `asset_trait.rs` to keep that file under the
//! 500-production-line budget (same reason `max_power.rs` was split out).

use chrono::{DateTime, Duration, Utc};

use super::{
    Asset, AssetCapability, AssetFlexibilityFloor, AssetHistoryBuffer, AssetState, HistoryPoint,
    Trajectory,
};

// ─── AssetHandle ──────────────────────────────────────────────────────────────

/// Wraps individual fields from a `(AssetConfig, AssetEntry)` pair to implement
/// the full `Asset` trait, including `id()`, `current_state()`, and `history()`.
///
/// Takes individual field references instead of `&AssetEntry` to avoid a circular
/// dependency (`AssetEntry` lives in `simulator`, which imports from `assets`).
///
/// Usage:
/// ```ignore
/// let handle = AssetHandle {
///     config: &entry_config,
///     id: &entry.id,
///     state: &entry.state,
///     history: &entry.history,
/// };
/// ```
// AssetHandle is used in tests and serves as the intended path for dyn Asset dispatch.
#[allow(dead_code)]
pub struct AssetHandle<'a> {
    pub config: &'a dyn Asset,
    pub id: &'a str,
    pub state: &'a AssetState,
    pub history: &'a AssetHistoryBuffer,
}

impl<'a> Asset for AssetHandle<'a> {
    fn id(&self) -> &str {
        self.id
    }

    fn current_state(&self) -> AssetState {
        self.state.clone()
    }

    fn history(&self, window: Duration, now: DateTime<Utc>) -> Vec<HistoryPoint> {
        self.history.slice(window, now)
    }

    fn capability(&self, state: &AssetState) -> AssetCapability {
        self.config.capability(state)
    }

    fn flexibility_floor(&self, state: &AssetState) -> AssetFlexibilityFloor {
        self.config.flexibility_floor(state)
    }

    fn step(&self, state: &AssetState, setpoint_kw: f64, dt: Duration) -> (AssetState, f64) {
        self.config.step(state, setpoint_kw, dt)
    }

    fn forced_power_kw(&self, state: &AssetState) -> Option<f64> {
        self.config.forced_power_kw(state)
    }

    /// Delegates to `self.config` rather than inheriting `Asset::simulate_forward`'s
    /// step()-based default — without this, `PvInverter`'s and `EvCharger`'s own
    /// overrides would be invisible through `AssetHandle`/`simulated_trajectory`.
    fn simulate_forward(
        &self,
        initial: &AssetState,
        setpoints: &[(DateTime<Utc>, f64)],
    ) -> Trajectory {
        self.config.simulate_forward(initial, setpoints)
    }

    /// `AssetHandle<'a>` isn't `'static` (it borrows), so it can never
    /// actually be represented as `dyn Any` — downcast the concrete
    /// `AssetConfig` variant it wraps instead. Never called in practice:
    /// `to_boxed_asset()` (the trait-object construction path) hands out the
    /// owned concrete type directly, not an `AssetHandle`.
    fn as_any(&self) -> &dyn std::any::Any {
        unimplemented!(
            "AssetHandle::as_any() is not supported — downcast the concrete asset type instead"
        )
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        unimplemented!(
            "AssetHandle::as_any_mut() is not supported — downcast the concrete asset type instead"
        )
    }

    /// `AssetHandle<'a>` isn't `'static` either (see `as_any`'s doc comment),
    /// so it can never produce a `Box<dyn Asset>` (which defaults to
    /// `Box<dyn Asset + 'static>`). Never called in practice.
    fn clone_box(&self) -> Box<dyn Asset> {
        unimplemented!(
            "AssetHandle::clone_box() is not supported — it borrows and can't be made 'static"
        )
    }
}
