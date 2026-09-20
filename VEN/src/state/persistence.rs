//! Writing the VEN's polled state to disk and reading it back.
//!
//! Split out of `state/mod.rs` (R-40 "split proactively when next touched")
//! once that file crossed the `VEN/src/` 500-production-line cap. Same
//! split-`impl AppState` pattern as `grid_signals.rs`, `openadr_objects.rs`
//! and `wire_health.rs`.
//!
//! The reading half is deliberately section-wise. A state file written by an
//! older build can carry a section this build no longer accepts — a protocol
//! field that has since become required, say — and losing that section is
//! unavoidable, but losing the ones beside it is not. That is the same rule
//! `controller::wire_reject` applies to a peer's objects, applied here to our
//! own: one unusable object costs exactly itself, and says so.

use serde::{Deserialize, Serialize};

use crate::controller::vtn_port::{OadrEvent, OadrProgram, OadrReport};
use crate::simulator::SensorSnapshot;

use super::AppState;

#[derive(Serialize, Deserialize)]
struct PersistedVenState {
    programs: Vec<OadrProgram>,
    events: Vec<OadrEvent>,
    reports: Vec<OadrReport>,
    sensor: SensorSnapshot,
}

/// Read one section, falling back rather than failing the whole restore.
///
/// `fallback` rather than `Default`: a `SensorSnapshot` has no sensible zero
/// value — deriving one would restore a reading timestamped at the epoch,
/// which reads as real data rather than as absent data.
fn section<T: serde::de::DeserializeOwned>(
    root: &serde_json::Value,
    name: &str,
    fallback: impl FnOnce() -> T,
) -> T {
    let Some(v) = root.get(name) else {
        return fallback();
    };
    serde_json::from_value(v.clone()).unwrap_or_else(|e| {
        tracing::warn!(
            section = name,
            error = %e,
            "persisted state section is not readable by this build; \
             dropping just this section and keeping the rest"
        );
        fallback()
    })
}

impl AppState {
    /// Restore what this file still holds. Only a file that is not JSON at all
    /// is an error; anything else is a partial restore, which is strictly
    /// better than starting empty.
    pub async fn load_from_json(&self, json: &str) -> anyhow::Result<()> {
        let root: serde_json::Value = serde_json::from_str(json)?;
        {
            let mut p = self.polling.write().await;
            p.programs = section(&root, "programs", Vec::new);
            p.events = section(&root, "events", Vec::new);
            p.reports = section(&root, "reports", Vec::new);
        }
        {
            let mut cs = self.ctrl_sim.write().await;
            cs.sensor = section(&root, "sensor", SensorSnapshot::empty_now);
        }
        Ok(())
    }

    pub async fn to_json(&self) -> anyhow::Result<String> {
        // Acquire each lock separately (INVARIANT: no guard held across a second lock acquisition).
        let (programs, events, reports) = {
            let p = self.polling.read().await;
            (p.programs.clone(), p.events.clone(), p.reports.clone())
        };
        let sensor = self.ctrl_sim.read().await.sensor.clone();
        let state = PersistedVenState {
            programs,
            events,
            reports,
            sensor,
        };
        Ok(serde_json::to_string_pretty(&state)?)
    }
}
