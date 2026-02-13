use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

use super::arbitration::ReactorMode;
use super::fsm::FsmState;
use super::Setpoints;

const MAX_TRACE_ENTRIES: usize = 100;

/// A single decision trace entry — one per reactor tick.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceEntry {
    pub ts: DateTime<Utc>,
    pub mode: String,
    pub fsm_state: String,
    pub active_events: Vec<String>,
    pub winning_intent: Option<String>,
    pub setpoints: Setpoints,
    pub constraints: Vec<String>,
    pub reason: String,
}

/// Ring buffer holding the last N trace entries.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecisionTrace {
    entries: VecDeque<TraceEntry>,
}

impl DecisionTrace {
    pub fn new() -> Self {
        Self {
            entries: VecDeque::with_capacity(MAX_TRACE_ENTRIES),
        }
    }

    pub fn push(&mut self, entry: TraceEntry) {
        if self.entries.len() >= MAX_TRACE_ENTRIES {
            self.entries.pop_front();
        }
        self.entries.push_back(entry);
    }

    pub fn entries(&self) -> Vec<TraceEntry> {
        self.entries.iter().cloned().collect()
    }

    pub fn last_n(&self, n: usize) -> Vec<TraceEntry> {
        self.entries.iter().rev().take(n).cloned().collect()
    }

    pub fn record(
        &mut self,
        now: DateTime<Utc>,
        mode: &ReactorMode,
        fsm_state: &FsmState,
        active_events: Vec<String>,
        winning_intent: Option<String>,
        setpoints: &Setpoints,
        constraints: Vec<String>,
        reason: String,
    ) {
        self.push(TraceEntry {
            ts: now,
            mode: mode.to_string(),
            fsm_state: fsm_state.to_string(),
            active_events,
            winning_intent,
            setpoints: setpoints.clone(),
            constraints,
            reason,
        });
    }
}
