use crate::entities::PlannerObjective;
use chrono::{DateTime, Utc};
use serde::Serialize;
use std::sync::Arc;
use uuid::Uuid;

/// Events pushed to SSE clients during a planning cycle.
///
/// `iteration` in `SolvingProgress` is a wall-clock tick count (1 per second),
/// **not** HiGHS internal iterations — `good_lp` does not expose solver callbacks.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PlannerEvent {
    SolvingStarted {
        objective: PlannerObjective,
        num_slots: usize,
        triggered_at: DateTime<Utc>,
    },
    SolvingProgress {
        elapsed_ms: u64,
        /// Wall-clock tick count (1 per second), not HiGHS internals.
        iteration: u32,
    },
    PlanReady {
        plan_id: Uuid,
        objective: PlannerObjective,
        solver_ms: u64,
        objective_eur: f64,
        friction_eur: f64,
        slot_count: usize,
        trigger: String,
    },
}

pub type PlannerEventTx = Arc<tokio::sync::broadcast::Sender<PlannerEvent>>;
