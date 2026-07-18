mod backoff;
pub mod heuristics_job;
pub mod history_sampler;
pub mod obligation;
pub mod planning;
pub mod poll_events;
pub mod poll_programs;
pub mod poll_reports;
mod poll_signals;
mod progress_ticker;
pub mod sim_tick;
pub mod state_persist;

pub(crate) use heuristics_job::spawn_heuristics_job;
pub(crate) use history_sampler::spawn_history_sampler;
pub(crate) use obligation::spawn_obligation_check;
pub(crate) use planning::spawn_planning;
pub(crate) use poll_events::spawn_event_poll;
pub(crate) use poll_programs::spawn_program_poll;
pub(crate) use poll_reports::spawn_report_poll;
pub(crate) use sim_tick::spawn_sim_tick;
pub(crate) use state_persist::spawn_state_persist;

/// Wrap a background task in a supervisor loop.
///
/// If the task panics or returns, the supervisor logs the event, waits
/// `cooldown_s` seconds, and re-spawns. The VEN process never exits due to
/// a single task failure.
///
/// `make_task` is called each time the task is (re-)started. It must return a
/// `JoinHandle` that drives the task to completion (or panic).
///
/// WP-T3 (`docs/plans/ven-ui-transparency.md`): records each (re)start and
/// completion on `state.task_status` for `GET /tasks/status` — the only place
/// `supervised_spawn`'s restart behavior was previously observable was the log.
pub(crate) fn supervised_spawn(
    name: &'static str,
    cooldown_s: u64,
    state: crate::state::AppState,
    make_task: impl Fn() -> tokio::task::JoinHandle<()> + Send + 'static,
) {
    tokio::spawn(async move {
        loop {
            state.record_task_started(name, chrono::Utc::now()).await;
            match make_task().await {
                Ok(()) => {
                    tracing::warn!(
                        task = name,
                        "exited unexpectedly, restarting in {cooldown_s}s"
                    );
                    state.record_task_completed(name, true).await;
                    state
                        .record_event(
                            chrono::Utc::now(),
                            "task_supervisor",
                            format!("{name} exited unexpectedly, restarting in {cooldown_s}s"),
                        )
                        .await;
                }
                Err(e) => {
                    tracing::error!(task = name, "panicked: {e:?}, restarting in {cooldown_s}s");
                    state.record_task_completed(name, false).await;
                    state
                        .record_event(
                            chrono::Utc::now(),
                            "task_supervisor",
                            format!("{name} panicked: {e:?}, restarting in {cooldown_s}s"),
                        )
                        .await;
                }
            }
            tokio::time::sleep(std::time::Duration::from_secs(cooldown_s)).await;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        atomic::{AtomicU32, Ordering},
        Arc,
    };

    #[tokio::test]
    async fn supervised_spawn_restarts_after_panic() {
        let counter = Arc::new(AtomicU32::new(0));
        let counter_clone = counter.clone();
        let state = crate::state::AppState::new();

        supervised_spawn("test-task", 0, state.clone(), move || {
            let c = counter_clone.clone();
            tokio::spawn(async move {
                c.fetch_add(1, Ordering::SeqCst);
                // Panic on first invocation only
                if c.load(Ordering::SeqCst) == 1 {
                    panic!("deliberate test panic");
                }
            })
        });

        // Poll until counter reaches 2, with a 2-second timeout.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        loop {
            if counter.load(Ordering::SeqCst) >= 2 {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "timed out waiting for supervisor restart (counter={})",
                counter.load(Ordering::SeqCst)
            );
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }

        // WP-T3: the panic + restart must be reflected on AppState. `cooldown_s`
        // is 0 in this test, so the supervisor loop races far ahead of the 10ms
        // polling above — assert on what's deterministic (at least one restart
        // was recorded, and completion recording works at all), not an exact
        // count or the specific last outcome.
        let statuses = state.task_statuses().await;
        let entry = statuses.get("test-task").expect("entry must exist");
        assert!(
            entry.restart_count >= 1,
            "at least one restart must be recorded, got {}",
            entry.restart_count
        );
        assert!(
            entry.last_success.is_some(),
            "at least one completion must be recorded"
        );

        // WP-T4: a task_supervisor event log entry must also have been recorded.
        let log = state.event_log_snapshot().await;
        assert!(
            log.iter()
                .any(|e| e.category == "task_supervisor" && e.message.contains("test-task")),
            "expected a task_supervisor event log entry naming test-task, got {log:?}"
        );
    }
}
