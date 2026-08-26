// Simulator tick background task.

mod arbiter_glue;
mod context;
mod dispatch_override;
mod finalize;
mod forecast_wiring;
mod helpers;
mod post_lock;
mod publish;
mod tick;

use std::sync::Arc;
use tokio::sync::Mutex;

use crate::controller::HistoryPort;
use crate::controller::MeasurementPort;
use crate::controller::VtnPort;
use crate::controller::WeatherForecastPort;
use crate::entities::asset::PlanTrigger;
use crate::entities::asset_params::PvForecastParams;
use crate::entities::planner_params::SimulatorParams;
use crate::planner_events::PlannerEventTx;
use crate::simulator::SimState;
use crate::state::AppState;

#[allow(clippy::too_many_arguments)]
pub(crate) fn spawn_sim_tick(
    state: AppState,
    sim: Arc<Mutex<SimState>>,
    sim_params: SimulatorParams,
    ven_name: String,
    vtn: Arc<dyn VtnPort>,
    trigger_tx: Arc<tokio::sync::watch::Sender<PlanTrigger>>,
    data_dir: String,
    event_tx: PlannerEventTx,
    weather: Arc<dyn WeatherForecastPort>,
    weather_pv_params: Option<PvForecastParams>,
    pv_co2_g_kwh: f64,
    pv_measurement: Arc<dyn MeasurementPort>,
    pv_measurement_enabled: bool,
    base_load_measurement: Arc<dyn MeasurementPort>,
    base_load_measurement_enabled: bool,
    notifier: crate::services::notify::Notifier,
    history: Option<Arc<dyn HistoryPort>>,
    comms_loss_config: Option<crate::profile::comms_loss::CommsLossConfig>,
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

        loop {
            tick_interval.tick().await;
            let (new_persist_counter, new_report_counter) = tick::tick_once(
                state.clone(),
                sim.clone(),
                ven_name.clone(),
                vtn.clone(),
                trigger_tx.clone(),
                data_dir.clone(),
                event_tx.clone(),
                persist_counter,
                persist_every_ticks,
                report_counter,
                report_every_ticks,
                tick_s,
                weather.clone(),
                weather_pv_params,
                pv_co2_g_kwh,
                pv_measurement.clone(),
                pv_measurement_enabled,
                base_load_measurement.clone(),
                base_load_measurement_enabled,
                notifier.clone(),
                history.clone(),
                comms_loss_config,
            )
            .await;

            persist_counter = new_persist_counter;
            report_counter = new_report_counter;
        }
    })
}
