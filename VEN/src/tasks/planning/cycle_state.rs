//! Everything a plan cycle reads out of `AppState` before it solves anything.
//!
//! Split out of `cycle.rs` when that file hit the `tasks/` 200-line cap
//! (R-40: split proactively when next touched). It is a real seam rather than
//! a line-count dodge: reading the world and solving against it are two jobs,
//! and the reads all have to happen *before* the sim snapshot is cloned —
//! which is itself load-bearing, since the one-shot `pv_irradiance` inject is
//! cleared by the tick and reading it after the clone can lose it.

use crate::entities::device_session::{BaselineOverride, EvSession, HeaterTarget, ShiftableLoad};
use crate::entities::planner_params::PlannerObjective;
use crate::entities::sim_inject::SimInjectState;
use crate::entities::tariff_snapshot::TariffSnapshot;
use crate::state::AppState;

/// The world as one plan cycle found it.
pub(crate) struct CycleState {
    pub rates: Vec<TariffSnapshot>,
    pub ev_sess: Option<EvSession>,
    pub heat_tgt: Option<HeaterTarget>,
    pub shift_loads: Vec<ShiftableLoad>,
    pub bl_override: Option<BaselineOverride>,
    pub obj: PlannerObjective,
    pub inject_snap: SimInjectState,
    /// A planned PV override from `/sim/inject`, separate from the snapshot
    /// because the planner takes it as its own input.
    pub pv_forecast_override: Option<f64>,
}

/// Read the cycle's inputs in one pass.
///
/// Takes no clock on purpose: nothing here is time-dependent, it only reads.
/// The time-sensitive part — patching a pending inject into the cloned
/// snapshot — stays in `cycle.rs`, next to the clone it patches.
pub(crate) async fn read_cycle_state(
    state: &AppState,
    active_objective: &tokio::sync::RwLock<PlannerObjective>,
) -> CycleState {
    let inject_snap = state.inject_state().await;
    CycleState {
        rates: state.planned_tariffs().await,
        ev_sess: state.ev_session().await,
        heat_tgt: state.heater_target().await,
        shift_loads: state.shiftable_loads().await,
        bl_override: state.baseline_override().await,
        obj: *active_objective.read().await,
        pv_forecast_override: inject_snap.pv_plan_kw,
        inject_snap,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The inject is read once and both of its consumers see the same value —
    /// reading it twice is how the planned-PV override and the snapshot patch
    /// could disagree about a one-shot inject.
    #[tokio::test]
    async fn pv_override_comes_from_the_same_inject_read_as_the_snapshot() {
        let state = AppState::new();
        let mut inject = state.inject_state().await;
        inject.pv_plan_kw = Some(4.2);
        state.set_inject_state(inject).await;

        let objective = tokio::sync::RwLock::new(PlannerObjective::default());
        let read = read_cycle_state(&state, &objective).await;
        assert_eq!(read.pv_forecast_override, Some(4.2));
        assert_eq!(read.inject_snap.pv_plan_kw, Some(4.2));
    }
}
