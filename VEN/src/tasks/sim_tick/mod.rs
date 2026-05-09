// Simulator tick background task.

mod helpers;
mod publish;
mod tick;

use std::sync::Arc;
use tokio::sync::Mutex;

use crate::entities::asset::PlanTrigger;
use crate::profile::Profile;
use crate::simulator::SimState;
use crate::state::AppState;
use crate::vtn::VtnClient;

pub(crate) fn spawn_sim_tick(
    state: AppState,
    sim: Arc<Mutex<SimState>>,
    profile: Arc<Profile>,
    ven_name: String,
    vtn: VtnClient,
    trigger_tx: Arc<tokio::sync::watch::Sender<PlanTrigger>>,
    data_dir: String,
) -> tokio::task::JoinHandle<()> {
    let tick_s = profile.simulator.tick_s;
    let persist_every_s = profile.simulator.persist_every_s;
    let report_interval_s = profile.simulator.report_interval_s;
    tokio::spawn(async move {
        let mut tick_interval = tokio::time::interval(std::time::Duration::from_secs(tick_s));
        let mut persist_counter: u64 = 0;
        let persist_every_ticks = if tick_s > 0 {
            persist_every_s / tick_s
        } else {
            15
        };
        let mut report_counter: u64 = 0;
        let report_every_ticks = if tick_s > 0 && report_interval_s > 0 {
            report_interval_s / tick_s
        } else {
            0
        };

        loop {
            tick_interval.tick().await;
            let (new_persist_counter, new_report_counter) =
                tick::tick_once(
                    state.clone(),
                    sim.clone(),
                    profile.clone(),
                    ven_name.clone(),
                    vtn.clone(),
                    trigger_tx.clone(),
                    data_dir.clone(),
                    persist_counter,
                    persist_every_ticks,
                    report_counter,
                    report_every_ticks,
                    tick_s,
                )
                .await;

            persist_counter = new_persist_counter;
            report_counter = new_report_counter;
        }
    })
}
