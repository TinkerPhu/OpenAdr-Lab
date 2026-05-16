// Simulator tick background task.

mod helpers;
mod publish;
mod tick;

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::controller::absorber::AbsorberState;
use crate::entities::asset::PlanTrigger;
use crate::entities::planner_params::{AbsorberParams, SimulatorParams};
use crate::planner_events::PlannerEventTx;
use crate::simulator::SimState;
use crate::state::AppState;
use crate::vtn::VtnClient;

pub(crate) fn spawn_sim_tick(
    state: AppState,
    sim: Arc<Mutex<SimState>>,
    sim_params: SimulatorParams,
    absorber_params: AbsorberParams,
    ven_name: String,
    vtn: VtnClient,
    trigger_tx: Arc<tokio::sync::watch::Sender<PlanTrigger>>,
    data_dir: String,
    event_tx: PlannerEventTx,
    deviation_pending: Arc<std::sync::atomic::AtomicBool>,
) -> tokio::task::JoinHandle<()> {
    let tick_s = sim_params.tick_s;
    let persist_every_s = sim_params.persist_every_s;
    let report_interval_s = sim_params.report_interval_s;
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

        let mut absorber_state = AbsorberState {
            residual_ticks: 0,
            last_state_change_ts: HashMap::new(),
            settling_ticks: HashMap::new(),
            active_overlay_kw: HashMap::new(),
            correction_is_active: false,
            last_emitted_correction_kw: 0.0,
        };

        loop {
            tick_interval.tick().await;
            let (new_absorber_state, new_persist_counter, new_report_counter) =
                tick::tick_once(
                    absorber_state,
                    state.clone(),
                    sim.clone(),
                    absorber_params.clone(),
                    ven_name.clone(),
                    vtn.clone(),
                    trigger_tx.clone(),
                    data_dir.clone(),
                    event_tx.clone(),
                    deviation_pending.clone(),
                    persist_counter,
                    persist_every_ticks,
                    report_counter,
                    report_every_ticks,
                    tick_s,
                )
                .await;

            absorber_state = new_absorber_state;
            persist_counter = new_persist_counter;
            report_counter = new_report_counter;
        }
    })
}
