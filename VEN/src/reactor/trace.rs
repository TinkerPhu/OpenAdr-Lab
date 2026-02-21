use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

use super::arbitration::ReactorMode;
use super::fsm::FsmState;
use super::Setpoints;

const MAX_TRACE_ENTRIES: usize = 1000;

/// Serialize an f32 rounded to 2 decimal places (0.01 resolution).
/// Converts via f64 before rounding to avoid f32 precision artifacts.
fn serialize_round2<S: serde::Serializer>(v: &f32, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_f64(((*v as f64) * 100.0).round() / 100.0)
}

/// Serialize Option<f32> as null or a 2-decimal-place number.
fn serialize_opt_round2<S: serde::Serializer>(v: &Option<f32>, s: S) -> Result<S::Ok, S::Error> {
    match v {
        None => s.serialize_none(),
        Some(f) => s.serialize_f64(((*f as f64) * 100.0).round() / 100.0),
    }
}

/// f32 setpoints stored in the trace ring buffer.
/// Serialized with 0.01 resolution to keep JSON payload compact
/// while retaining sub-kW precision for ramp visibility in the chart.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceSetpoints {
    #[serde(serialize_with = "serialize_round2")]
    pub ev_charge_kw: f32,
    #[serde(serialize_with = "serialize_round2")]
    pub heater_kw: f32,
    /// Active export cap in kW; null = no limit (full output).
    #[serde(serialize_with = "serialize_opt_round2")]
    pub pv_export_limit_kw: Option<f32>,
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
                ev_charge_kw: setpoints.ev_charge_kw as f32,
                heater_kw: setpoints.heater_kw as f32,
                pv_export_limit_kw: setpoints.pv_export_limit_kw.map(|v| v as f32),
                mode: setpoints.mode.clone(),
            },
            constraints,
            reason,
        });
    }
}
