use chrono::Utc;
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};
use tracing::info;

use crate::controller::{HistoryPort, SolverPort, WeatherForecastPort};
use crate::entities::asset::{PlanTrigger, PlanTriggerSignal};
use crate::entities::asset_params::{AssetParams, PvForecastParams};
use crate::entities::planner_params::{PlannerObjective, PlannerParams};
use crate::planner_events::PlannerEventTx;
use crate::simulator::SimState;
use crate::state::AppState;

mod cycle;
mod cycle_state;

#[allow(clippy::too_many_arguments)]
pub(crate) fn spawn_planning(
    state: AppState,
    planner: PlannerParams,
    grid_max_import_kw: f64,
    grid_max_export_kw: f64,
    asset_params: Vec<AssetParams>,
    solver: Arc<dyn SolverPort>,
    mut trigger_rx: tokio::sync::watch::Receiver<PlanTriggerSignal>,
    sim: Arc<Mutex<SimState>>,
    active_objective: Arc<RwLock<PlannerObjective>>,
    event_tx: PlannerEventTx,
    notifier: crate::services::notify::Notifier,
    now_fn: impl Fn() -> chrono::DateTime<Utc> + Send + Sync + 'static,
    weather: Arc<dyn WeatherForecastPort>,
    weather_pv_params: Option<PvForecastParams>,
    history: Option<Arc<dyn HistoryPort>>,
    // GB-54 — this VEN's stable phase within the replan grid. Built from the
    // VEN's name by `planner_params::replan_phase_offset_s` at start-up.
    replan_phase_offset_s: u64,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        // Initial delay: let event poll populate rates before first plan
        tokio::time::sleep(std::time::Duration::from_secs(
            planner.planning_initial_delay_s,
        ))
        .await;
        // First cycle is always Periodic; later cycles are set by the select! below.
        // A local (not borrow()ing the watch channel) prevents stale retained values
        // from mis-classifying timeout cycles as hard triggers, bypassing the gate.
        let mut wake_trigger = PlanTriggerSignal::bare(PlanTrigger::Periodic);
        loop {
            // GB-55: hold a rate-change reaction for a jittered moment before
            // solving. A VTN rate update reaches the whole fleet at once and hard
            // triggers bypass GB-54's periodic grid, so without this every VEN
            // solves on the same instant. Held *before* `now_fn()` so the plan is
            // built from the time it is actually solved at, not from when the
            // trigger arrived.
            if matches!(wake_trigger.trigger, PlanTrigger::RateChange) {
                let delay_s = crate::entities::planner_params::rate_change_delay_s(
                    planner.rate_change_trigger_delay_s,
                    planner.rate_change_trigger_jitter_pct,
                    planner.replan_interval_s,
                    rand::random::<f64>(),
                );
                if delay_s > 0 {
                    info!(delay_s, "planner loop: holding RateChange replan (GB-55)");
                    tokio::time::sleep(std::time::Duration::from_secs(delay_s)).await;
                }
            }

            let wall_now = now_fn();
            // Align to the nearest step boundary so all replans within the same window
            // share identical slot grids (gate stability, warm-start prerequisite).
            // wall_now is kept separately for Plan.created_at (gate decay uses real age).
            let now = crate::services::planning::align_to_step(wall_now, planner.plan_step_s);
            let signal = wake_trigger.clone();
            let trigger = signal.trigger.clone();
            let trigger_reason = format!("{:?}", trigger);

            // Hard triggers (user action, state change) must not be constrained by the anchor
            // from a previous Periodic cycle — clear it so the next solve is fully free.
            if !matches!(trigger, PlanTrigger::Periodic) {
                state.set_anchor_until(None).await;
            }

            info!(trigger = %trigger_reason, "planner loop: starting plan cycle");

            cycle::run_plan_cycle(
                &state,
                &sim,
                &planner,
                grid_max_import_kw,
                grid_max_export_kw,
                &asset_params,
                &solver,
                &active_objective,
                &event_tx,
                &notifier,
                &weather,
                weather_pv_params.as_ref(),
                &signal,
                wall_now,
                now,
                history.clone(),
            )
            .await;

            // Wait for next trigger OR the next periodic slot.
            // Record what woke us: timeout → Periodic, channel change → that trigger.
            // This ensures the acceptance gate sees Periodic for routine replans
            // and is only bypassed for genuine event-driven triggers.
            //
            // GB-54: the wait is to the next point on this VEN's own absolute
            // replan grid, not a flat sleep from here. Sleeping from the end of a
            // cycle made the real period `replan_interval_s + solve_time`, so
            // fleet members drifted at per-VEN rates and periodically collided on
            // a shared host — see `next_replan_at`.
            let due = crate::entities::planner_params::next_replan_at(
                now_fn(),
                planner.replan_interval_s,
                replan_phase_offset_s,
            );
            let wait_s = (due - now_fn()).num_seconds().clamp(0, i64::MAX) as u64;
            wake_trigger = tokio::select! {
                _ = tokio::time::sleep(std::time::Duration::from_secs(wait_s)) => PlanTriggerSignal::bare(PlanTrigger::Periodic),
                _ = trigger_rx.changed() => trigger_rx.borrow_and_update().clone(),
            };
        }
    })
}

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use std::sync::Arc;
    use tokio::sync::{broadcast, watch, Mutex, RwLock};

    use crate::entities::asset::{PlanTrigger, PlanTriggerSignal};
    use crate::entities::planner_params::{PlannerObjective, PlannerParams};
    use crate::planner_events::PlannerEvent;
    use crate::services::test_support::mock_solver_port::MockSolverPort;
    use crate::simulator::SimState;
    use crate::state::AppState;

    use super::spawn_planning;

    /// Minimal `Plan` for test construction — no cycle in
    /// `spawn_planning_constructs_without_panic` ever runs to completion (the
    /// task is aborted right after spawn), so this value is never consumed.
    fn minimal_plan() -> crate::entities::plan::Plan {
        serde_json::from_value(serde_json::json!({
            "id": uuid::Uuid::new_v4().to_string(),
            "created_at": Utc::now().to_rfc3339(),
            "trigger": "PERIODIC",
            "horizon": {
                "start_time": "2026-01-01T00:00:00Z",
                "end_time": "2026-01-02T00:00:00Z",
                "step_size_s": 900,
                "num_steps": 96,
                "far_horizon": "2026-01-02T00:00:00Z"
            },
            "slots": [],
            "summary": {
                "total_cost_eur": 0.0,
                "total_co2_g": 0.0,
                "total_import_kwh": 0.0,
                "total_export_kwh": 0.0
            },
            "envelopes": [],
            "warnings": [],
            "objective_eur": 0.0,
            "friction_eur": 0.0
        }))
        .expect("minimal test Plan must deserialize")
    }

    fn minimal_sim() -> Arc<Mutex<SimState>> {
        let s: SimState = serde_json::from_value(serde_json::json!({
            "asset_configs": [],
            "assets": [],
            "grid": {
                "net_power_w": 0.0, "import_w": 0.0, "export_w": 0.0,
                "voltage_v": 0.0, "import_kwh": 0.0, "export_kwh": 0.0
            },
            "last_tick": chrono::Utc::now().to_rfc3339()
        }))
        .expect("minimal SimState must deserialize");
        Arc::new(Mutex::new(s))
    }

    #[tokio::test]
    async fn spawn_planning_constructs_without_panic() {
        let (trigger_tx, trigger_rx) =
            watch::channel(PlanTriggerSignal::bare(PlanTrigger::Periodic));
        let (event_bcast_tx, _) = broadcast::channel::<PlannerEvent>(1);
        let event_tx = Arc::new(event_bcast_tx);
        let solver = Arc::new(MockSolverPort::returning(minimal_plan()));
        let sim = minimal_sim();
        let active_objective = Arc::new(RwLock::new(PlannerObjective::default()));

        let handle = spawn_planning(
            AppState::new(),
            PlannerParams::default(),
            10.0,
            10.0,
            vec![],
            solver,
            trigger_rx,
            sim,
            active_objective,
            event_tx,
            crate::services::notify::Notifier::new(None),
            Utc::now,
            Arc::new(crate::controller::NoopWeatherPort),
            None,
            None,
            0,
        );
        handle.abort();
        let _ = trigger_tx; // keep alive until abort
                            // passes if no panic during construction and abort
    }
}
