use super::interval::ActiveInterval;
use serde::{Deserialize, Serialize};

/// The type of control the reactor should apply.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ReactorMode {
    Idle,
    ExportCapLimit,
    ImportCapLimit,
    Price,
    Simple,
    ChargeSetpoint,
}

impl std::fmt::Display for ReactorMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Idle => write!(f, "IDLE"),
            Self::ExportCapLimit => write!(f, "EXPORT_CAP"),
            Self::ImportCapLimit => write!(f, "IMPORT_CAP"),
            Self::Price => write!(f, "PRICE"),
            Self::Simple => write!(f, "SIMPLE"),
            Self::ChargeSetpoint => write!(f, "CHARGE_SETPOINT"),
        }
    }
}

/// The winning control intent from arbitration.
#[derive(Debug, Clone)]
pub struct ControlIntent {
    pub mode: ReactorMode,
    pub value: f64,
    pub event_ids: Vec<String>,
    pub description: String,
}

/// Select the winning control intent from active intervals.
///
/// Priority rules:
/// 1. Hard constraints (EXPORT_CAPACITY_LIMIT, IMPORT_CAPACITY_LIMIT) beat incentives (PRICE)
/// 2. Among same type: lower priority number wins (0 = highest)
/// 3. Tie-break: most recently created event
pub fn arbitrate(intervals: &[ActiveInterval]) -> Option<ControlIntent> {
    if intervals.is_empty() {
        return None;
    }

    // Separate hard constraints from incentives
    let mut constraints: Vec<&ActiveInterval> = Vec::new();
    let mut incentives: Vec<&ActiveInterval> = Vec::new();

    for iv in intervals {
        match iv.payload_type.as_str() {
            "EXPORT_CAPACITY_LIMIT" | "IMPORT_CAPACITY_LIMIT" | "SIMPLE" | "CHARGE_STATE_SETPOINT" => constraints.push(iv),
            "PRICE" => incentives.push(iv),
            _ => {} // ignore unknown types
        }
    }

    // Hard constraints always win
    let winning_pool = if !constraints.is_empty() {
        &constraints
    } else if !incentives.is_empty() {
        &incentives
    } else {
        return None;
    };

    // Sort by priority (ascending), then by created (descending = newest first)
    let mut sorted: Vec<&&ActiveInterval> = winning_pool.iter().collect();
    sorted.sort_by(|a, b| {
        a.priority
            .cmp(&b.priority)
            .then_with(|| b.created.cmp(&a.created))
    });

    let winner = sorted[0];
    let mode = match winner.payload_type.as_str() {
        "EXPORT_CAPACITY_LIMIT" => ReactorMode::ExportCapLimit,
        "IMPORT_CAPACITY_LIMIT" => ReactorMode::ImportCapLimit,
        "SIMPLE" => ReactorMode::Simple,
        "CHARGE_STATE_SETPOINT" => ReactorMode::ChargeSetpoint,
        _ => ReactorMode::Price,
    };

    let event_ids: Vec<String> = intervals.iter().map(|iv| iv.event_id.clone()).collect();
    let description = format!(
        "{}={:.1} (event: {}, priority: {})",
        winner.payload_type, winner.payload_value, winner.event_name, winner.priority
    );

    Some(ControlIntent {
        mode,
        value: winner.payload_value,
        event_ids,
        description,
    })
}
