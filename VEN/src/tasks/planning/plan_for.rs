//! The one guard of the planning cycle (openspec `opportunistic-objective` D0, D6): a planning
//! objective is solved exactly as before; the opportunistic one is forecast instead, by stepping
//! a clone of the site with the live tick's own decision.
//!
//! Reads, for the forecast only, what the live tick would read at each slot (EV sessions and
//! settings, heater targets, shiftable loads, dispatch windows, whether limit enforcement is
//! on) and runs the forecast off the async runtime. Decides nothing itself: the requests come
//! from the same functions the tick uses, the plan from `milp_planner::plan_from_forecast`.

use std::collections::BTreeMap;
use std::sync::Arc;

use chrono::{DateTime, Utc};

use crate::controller::arbiter::{opportunistic_requests, session_departures, OverlayEnabled};
use crate::controller::{SolveRequest, SolverPort};
use crate::entities::device_session::{EvSessionQueue, HeaterTarget, ShiftableLoad};
use crate::entities::ev_settings::EvSettings;
use crate::entities::plan::Plan;
use crate::entities::planner_params::PlannerObjective;
use crate::simulator::opportunistic_forecast::{
    opportunistic_forecast, ForecastSite, SlotRequests,
};
use crate::simulator::SimState;
use crate::state::AppState;

/// Solve, or forecast under the opportunistic objective.
pub(super) async fn plan_for(
    objective: PlannerObjective,
    solver: &Arc<dyn SolverPort>,
    solve_req: SolveRequest,
    state: &AppState,
    sim: &SimState,
) -> Plan {
    if objective.dispatches_from_plan() {
        return crate::services::PlanningService::solve_plan(solver, solve_req).await;
    }
    let requests = SiteRequests {
        ev_settings: state.ev_settings_all().await,
        ev_sessions: state.ev_sessions().await,
        heater_targets: state.heater_targets().await,
        shiftable_loads: state.shiftable_loads().await,
    };
    let dispatch_windows = state.dispatch_windows().await;
    let limit_enforcement_enabled = state.limit_enforcement_enabled().await;
    let sim = sim.clone();
    let (planner, now, trigger) = (
        solve_req.planner.clone(),
        solve_req.now,
        solve_req.trigger.clone(),
    );
    tokio::task::spawn_blocking(move || {
        let (schedule, alerts) = (
            solve_req.grid.capacity_schedule.clone(),
            solve_req.grid.alert_windows.clone(),
        );
        let requests_at = |t: DateTime<Utc>| requests.at(t);
        let site = ForecastSite {
            capacity_schedule: &schedule,
            dispatch_windows: &dispatch_windows,
            alert_windows: &alerts,
            limit_enforcement_enabled,
            requests_at: &requests_at,
        };
        crate::controller::milp_planner::plan_from_forecast(solve_req, |grid| {
            opportunistic_forecast(sim, grid, &site)
        })
    })
    .await
    .unwrap_or_else(|join_err| {
        tracing::error!(error = %join_err, "opportunistic forecast panicked; returning fallback plan");
        crate::controller::milp_planner::fallback_plan(
            &planner,
            now,
            trigger,
            &[],
            None,
            format!("opportunistic forecast panicked: {join_err}"),
            objective,
        )
    })
}

/// What the user has asked of the site, as read at the start of the cycle.
struct SiteRequests {
    ev_settings: BTreeMap<String, EvSettings>,
    ev_sessions: BTreeMap<String, EvSessionQueue>,
    heater_targets: BTreeMap<String, HeaterTarget>,
    shiftable_loads: Vec<ShiftableLoad>,
}

impl SiteRequests {
    /// The requests in force at `now`, exactly as the live tick reads them then.
    fn at(&self, now: DateTime<Utc>) -> SlotRequests {
        let (overlay, sessions_now) = OverlayEnabled::at(&self.ev_settings, &self.ev_sessions, now);
        SlotRequests {
            requests: opportunistic_requests(
                now,
                &sessions_now,
                &self.heater_targets,
                &self.shiftable_loads,
            ),
            ev_departure_by_asset: session_departures(&sessions_now),
            ev_surplus_switch_on: overlay,
        }
    }
}
