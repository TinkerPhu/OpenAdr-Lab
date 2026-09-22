//! Post-lock housekeeping for the simulator tick: inject-field clearing and
//! the PHASE 6/7 periodic-counter wrappers — split out of `tick.rs` /
//! `publish.rs` to keep both files under the tasks/ file-size cap.

use std::sync::Arc;
use tokio::sync::Mutex;

use crate::simulator::SimState;
use crate::state::AppState;

/// PHASE 1 (post-lock): clear every one-shot inject field applied this tick.
pub(crate) async fn clear_inject_fields(
    state: &AppState,
    cleared_fields: Vec<&'static str>,
    pv_clear: bool,
    base_clear: bool,
) {
    for field in cleared_fields {
        state.clear_inject_field(field).await;
    }
    if pv_clear {
        state.clear_inject_field("pv_irradiance").await;
    }
    if base_clear {
        state.clear_inject_field("base_load_kw").await;
    }
}

/// PHASE 7 counter wrapper: runs `publish::persist_sim_state` only every
/// `persist_every_ticks`, returning the updated counter.
pub(crate) async fn maybe_persist_sim_state(
    mut persist_counter: u64,
    persist_every_ticks: u64,
    sim: &Arc<Mutex<SimState>>,
    data_dir: &str,
) -> u64 {
    persist_counter += 1;
    if persist_counter >= persist_every_ticks {
        persist_counter = 0;
        super::publish::persist_sim_state(sim, data_dir).await;
    }
    persist_counter
}

/// PHASE 7: periodic sim-state persist.
///
/// Was "reports, then persist": the timer-driven report path it also ran is
/// gone (D-5/F-9). Reporting is the obligation loop's job now — a VTN that
/// wants reports asks for them with `reportDescriptors`, and the standing
/// fleet-monitoring event is what keeps an idle fleet visible.
pub(crate) async fn run_periodic_persist(
    persist_counter: u64,
    persist_every_ticks: u64,
    sim: &Arc<Mutex<SimState>>,
    data_dir: &str,
) -> u64 {
    maybe_persist_sim_state(persist_counter, persist_every_ticks, sim, data_dir).await
}
