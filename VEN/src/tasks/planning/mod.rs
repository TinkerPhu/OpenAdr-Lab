use chrono::Utc;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::info;

use crate::controller::{HistoryPort, SolverPort, WeatherForecastPort};
use crate::entities::asset::{PlanTrigger, PlanTriggerSignal};
use crate::entities::asset_params::AssetParams;
use crate::entities::planner_params::PlannerParams;
use crate::planner_events::PlannerEventTx;
use crate::simulator::SimState;
use crate::state::AppState;

mod assemble;
mod cycle;
mod cycle_state;
mod plan_for;

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
    active_objective: crate::app_ctx::ActiveObjective,
    event_tx: PlannerEventTx,
    notifier: crate::services::notify::Notifier,
    now_fn: impl Fn() -> chrono::DateTime<Utc> + Send + Sync + 'static,
    weather: Arc<dyn WeatherForecastPort>,
    weather_pv_params: crate::entities::asset_params::PvWeatherParams,
    history: Option<Arc<dyn HistoryPort>>,
    telemetry: Arc<dyn crate::controller::telemetry_port::TelemetryPort>,
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
            // Held *before* `now_fn()` so the plan is built from the time it is
            // actually solved at, not from when the trigger arrived.
            hold_rate_change(&planner, &wake_trigger.trigger).await;

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
                state.set_anchor_until(Default::default()).await;
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
                &weather_pv_params,
                &signal,
                wall_now,
                now,
                history.clone(),
                telemetry.as_ref(),
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

/// GB-55: hold a rate-change reaction for a jittered moment before solving. A VTN rate update
/// reaches the whole fleet at once and hard triggers bypass GB-54's periodic grid, so without
/// this every VEN solves on the same instant.
async fn hold_rate_change(planner: &PlannerParams, trigger: &PlanTrigger) {
    if !matches!(trigger, PlanTrigger::RateChange) {
        return;
    }
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
            "session_flexibility": [],
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
                "net_power_kw": 0.0, "import_kw": 0.0, "export_kw": 0.0,
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
        let active_objective = Arc::new(RwLock::new(
            crate::entities::objective_setting::ObjectiveSetting::from_profile(
                PlannerObjective::default(),
            ),
        ));

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
            Default::default(),
            None,
            Arc::new(crate::controller::telemetry_port::NoTelemetry),
            0,
        );
        handle.abort();
        let _ = trigger_tx; // keep alive until abort
                            // passes if no panic during construction and abort
    }

    /// Records the plans published on the `plan` leaf.
    #[derive(Default)]
    struct PlanLeaf(std::sync::Mutex<Vec<uuid::Uuid>>);

    #[async_trait::async_trait]
    impl crate::controller::telemetry_port::TelemetryPort for PlanLeaf {
        async fn publish_telemetry(&self, _: crate::controller::telemetry_port::TelemetryBody<'_>) {
        }
        fn is_connected(&self) -> bool {
            true
        }
        fn publish_plan(&self, plan: Arc<crate::entities::plan::Plan>) {
            self.0.lock().unwrap().push(plan.id);
        }
    }

    /// 6.3: the plan a cycle adopts goes out on the fleet channel's `plan` leaf.
    #[tokio::test]
    async fn an_adopted_plan_is_published_on_the_plan_leaf() {
        let (_trigger_tx, trigger_rx) =
            watch::channel(PlanTriggerSignal::bare(PlanTrigger::Periodic));
        let (event_bcast_tx, _) = broadcast::channel::<PlannerEvent>(8);
        let plan = minimal_plan();
        let state = AppState::new();
        let leaf = Arc::new(PlanLeaf::default());
        let planner = PlannerParams {
            planning_initial_delay_s: 0,
            ..PlannerParams::default()
        };
        let handle = spawn_planning(
            state.clone(),
            planner,
            10.0,
            10.0,
            vec![],
            Arc::new(MockSolverPort::returning(plan.clone())),
            trigger_rx,
            minimal_sim(),
            Arc::new(RwLock::new(
                crate::entities::objective_setting::ObjectiveSetting::from_profile(
                    PlannerObjective::MinCost,
                ),
            )),
            Arc::new(event_bcast_tx),
            crate::services::notify::Notifier::new(None),
            Utc::now,
            Arc::new(crate::controller::NoopWeatherPort),
            Default::default(),
            None,
            leaf.clone(),
            0,
        );
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
        while state.active_plan().await.is_none() && std::time::Instant::now() < deadline {
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        handle.abort();
        let adopted = state
            .active_plan()
            .await
            .expect("the first plan is adopted");
        assert_eq!(*leaf.0.lock().unwrap(), vec![adopted.id]);
    }

    /// 7.4: under the opportunistic objective the cycle adopts the forecast plan, with slots
    /// over the planner's horizon and the simulated status, and never calls the solver.
    #[tokio::test]
    async fn an_opportunistic_cycle_adopts_a_forecast_plan() {
        let (_trigger_tx, trigger_rx) =
            watch::channel(PlanTriggerSignal::bare(PlanTrigger::Periodic));
        let (event_bcast_tx, _) = broadcast::channel::<PlannerEvent>(8);
        let state = AppState::new();
        let planner = PlannerParams {
            planning_initial_delay_s: 0,
            ..PlannerParams::default()
        };
        // A solve would panic into the infeasible fallback, which the status check rejects.
        let solver = Arc::new(crate::services::test_support::mock_solver_port::PanickingSolverPort);
        let handle = spawn_planning(
            state.clone(),
            planner.clone(),
            10.0,
            10.0,
            vec![],
            solver,
            trigger_rx,
            minimal_sim(),
            Arc::new(RwLock::new(
                crate::entities::objective_setting::ObjectiveSetting::from_profile(
                    PlannerObjective::Opportunistic,
                ),
            )),
            Arc::new(event_bcast_tx),
            crate::services::notify::Notifier::new(None),
            Utc::now,
            Arc::new(crate::controller::NoopWeatherPort),
            Default::default(),
            None,
            Arc::new(crate::controller::telemetry_port::NoTelemetry),
            0,
        );
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
        while state.active_plan().await.is_none() && std::time::Instant::now() < deadline {
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        handle.abort();
        let plan = state
            .active_plan()
            .await
            .expect("the forecast plan is adopted");
        assert_eq!(plan.objective, PlannerObjective::Opportunistic);
        assert_eq!(
            plan.solve_status,
            crate::entities::plan::SolveStatus::Simulated
        );
        assert_eq!(
            plan.slots.len(),
            crate::entities::time_grid::slot_count(&planner.plan_zones)
        );
    }
}
