use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

use super::arbitration::ReactorMode;
use super::fsm::FsmState;
use super::Setpoints;

const MAX_TRACE_ENTRIES: usize = 1000;

/// Integer-rounded setpoints stored in the trace ring buffer.
/// Using i32 instead of f64 saves ~40 % of JSON payload per entry;
/// kW precision beyond 1 W is not meaningful for a 1-second tick.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceSetpoints {
    pub ev_charge_kw: i32,
    pub heater_kw: i32,
    pub pv_curtailment_pct: i32, // 0–100 (rounded from 0.0–1.0)
    pub mode: String,
}

/// A single decision trace entry — one per reactor tick.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceEntry {
    pub ts: DateTime<Utc>,
    pub mode: String,
    pub fsm_state: String,
    pub active_events: Vec<String>,
    pub winning_intent: Option<String>,
    pub setpoints: TraceSetpoints,
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
            setpoints: TraceSetpoints {
                ev_charge_kw: setpoints.ev_charge_kw.round() as i32,
                heater_kw: setpoints.heater_kw.round() as i32,
                pv_curtailment_pct: (setpoints.pv_curtailment * 100.0).round() as i32,
                mode: setpoints.mode.clone(),
            },
            constraints,
            reason,
        });
    }
}
