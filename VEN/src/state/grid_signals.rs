//! `AppState` accessors for OpenADR-derived grid signals — tariffs, the capacity-limit
//! envelope, and alert/SIMPLE/dispatch windows. Split out of `state/mod.rs` (2026-08-10,
//! R-40 "split proactively when next touched") once that file crossed the `VEN/src/`
//! 500-production-line cap. Same `impl AppState` pattern as the rest of `mod.rs` — Rust
//! allows an impl block to be split across files as long as the type stays in one crate.

use crate::entities::capacity::{
    AlertWindow, CapacitySnapshot, DispatchWindow, OadrCapacityState, SimpleWindow,
};
use crate::entities::tariff_snapshot::TariffSnapshot;
use chrono::{DateTime, Utc};

use super::AppState;

impl AppState {
    pub async fn planned_tariffs(&self) -> Vec<TariffSnapshot> {
        self.hems.read().await.planned_tariffs.clone()
    }

    pub async fn set_planned_tariffs(&self, tariffs: Vec<TariffSnapshot>) {
        self.hems.write().await.planned_tariffs = tariffs;
    }

    /// The capacity state in force at `now` (GB-48): read from the schedule when asked — with a
    /// sim-injected limit standing in where the VTN announced none — never stored, so it turns
    /// over at its interval boundary and not at the next event poll.
    pub async fn capacity_state_at(&self, now: DateTime<Utc>) -> OadrCapacityState {
        let inject = self.inject_state().await;
        let hems = self.hems.read().await;
        OadrCapacityState::at(
            &hems.planned_capacity_limits,
            &inject,
            now,
            hems.capacity_updated_at,
        )
    }

    pub async fn planned_capacity_limits(&self) -> Vec<CapacitySnapshot> {
        self.hems.read().await.planned_capacity_limits.clone()
    }

    /// Replace the capacity schedule with what the event poll at `now` listed; the poll's time is
    /// kept as `capacity_updated_at` when it listed any capacity payload.
    pub async fn set_planned_capacity_limits(
        &self,
        limits: Vec<CapacitySnapshot>,
        now: DateTime<Utc>,
    ) {
        let mut hems = self.hems.write().await;
        hems.capacity_updated_at = (!limits.is_empty()).then_some(now);
        hems.planned_capacity_limits = limits;
    }

    pub async fn alert_windows(&self) -> Vec<AlertWindow> {
        self.hems.read().await.alert_windows.clone()
    }

    pub async fn set_alert_windows(&self, alerts: Vec<AlertWindow>) {
        self.hems.write().await.alert_windows = alerts;
    }

    pub async fn simple_windows(&self) -> Vec<SimpleWindow> {
        self.hems.read().await.simple_windows.clone()
    }

    pub async fn set_simple_windows(&self, windows: Vec<SimpleWindow>) {
        self.hems.write().await.simple_windows = windows;
    }

    pub async fn dispatch_windows(&self) -> Vec<DispatchWindow> {
        self.hems.read().await.dispatch_windows.clone()
    }

    pub async fn set_dispatch_windows(&self, windows: Vec<DispatchWindow>) {
        self.hems.write().await.dispatch_windows = windows;
    }
}
