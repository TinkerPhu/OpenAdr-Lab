//! `AppState` accessors for the OpenADR objects this VEN has polled from the
//! VTN — programs, events and reports as they arrived on the wire. Split out of
//! `state/mod.rs` (R-40 "split proactively when next touched") once that file
//! crossed the `VEN/src/` 500-production-line cap. Same split-`impl AppState`
//! pattern as `grid_signals.rs`, `obligations.rs` and `wire_health.rs`.
//!
//! These hold the objects that *parsed*; what the VTN sent and this VEN refused
//! lives next door in `wire_health.rs`.

use crate::controller::vtn_port::{OadrEvent, OadrProgram, OadrReport};

use super::AppState;

impl AppState {
    pub async fn set_programs(&self, programs: Vec<OadrProgram>) {
        self.polling.write().await.programs = programs;
    }

    pub async fn set_events(&self, mut events: Vec<OadrEvent>, max_keep: usize) {
        events.truncate(max_keep);
        self.polling.write().await.events = events;
    }

    pub async fn set_reports(&self, reports: Vec<OadrReport>) {
        self.polling.write().await.reports = reports;
    }

    pub async fn programs(&self) -> Vec<OadrProgram> {
        self.polling.read().await.programs.clone()
    }

    pub async fn events(&self) -> Vec<OadrEvent> {
        self.polling.read().await.events.clone()
    }

    pub async fn reports(&self) -> Vec<OadrReport> {
        self.polling.read().await.reports.clone()
    }
}
