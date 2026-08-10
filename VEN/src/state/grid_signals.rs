//! `AppState` accessors for OpenADR-derived grid signals — tariffs, the capacity-limit
//! envelope, and alert/SIMPLE/dispatch windows. Split out of `state/mod.rs` (2026-08-10,
//! R-40 "split proactively when next touched") once that file crossed the `VEN/src/`
//! 500-production-line cap. Same `impl AppState` pattern as the rest of `mod.rs` — Rust
//! allows an impl block to be split across files as long as the type stays in one crate.

use crate::entities::capacity::{
    AlertWindow, CapacitySnapshot, DispatchWindow, OadrCapacityState, SimpleWindow,
};
use crate::entities::tariff_snapshot::TariffSnapshot;

use super::AppState;

impl AppState {
    pub async fn planned_tariffs(&self) -> Vec<TariffSnapshot> {
        self.hems.read().await.planned_tariffs.clone()
    }

    pub async fn set_planned_tariffs(&self, tariffs: Vec<TariffSnapshot>) {
        self.hems.write().await.planned_tariffs = tariffs;
    }

    pub async fn capacity_state(&self) -> OadrCapacityState {
        self.hems.read().await.capacity_state.clone()
    }

    pub async fn set_capacity_state(&self, state: OadrCapacityState) {
        self.hems.write().await.capacity_state = state;
    }

    pub async fn planned_capacity_limits(&self) -> Vec<CapacitySnapshot> {
        self.hems.read().await.planned_capacity_limits.clone()
    }

    pub async fn set_planned_capacity_limits(&self, limits: Vec<CapacitySnapshot>) {
        self.hems.write().await.planned_capacity_limits = limits;
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
