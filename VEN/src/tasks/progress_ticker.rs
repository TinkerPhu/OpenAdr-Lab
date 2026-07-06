use tokio::sync::oneshot;
use tokio::task::JoinHandle;

use crate::planner_events::{PlannerEvent, PlannerEventTx};

/// Spawn a 1 s progress-ticker that emits `PlannerEvent::SolvingProgress` while a plan
/// solve is in flight. Returns the task handle and a cancel sender — send on the sender
/// then `.await` the handle to shut it down cleanly before continuing the plan cycle.
pub(super) fn spawn_progress_ticker(
    event_tx: PlannerEventTx,
) -> (JoinHandle<()>, oneshot::Sender<()>) {
    let (cancel_tx, mut cancel_rx) = oneshot::channel::<()>();
    let handle = tokio::spawn(async move {
        let start = std::time::Instant::now();
        let mut ticker = tokio::time::interval(std::time::Duration::from_secs(1));
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let mut iteration: u32 = 0;
        loop {
            tokio::select! {
                _ = ticker.tick() => {
                    iteration += 1;
                    let _ = event_tx.send(PlannerEvent::SolvingProgress {
                        elapsed_ms: start.elapsed().as_millis() as u64,
                        iteration,
                    });
                }
                _ = &mut cancel_rx => break,
            }
        }
    });
    (handle, cancel_tx)
}
