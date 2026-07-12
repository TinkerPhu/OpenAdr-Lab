// Simulator tick loop body, extracted to keep sim_tick/mod.rs under 200 lines.

use std::sync::Arc;
use tokio::sync::Mutex;

use crate::controller::SimulatorPort;
use crate::controller::VtnPort;
use crate::entities::asset::PlanTrigger;
use crate::planner_events::PlannerEventTx;
use crate::simulator::SimState;
use crate::state::{AppState, EvSettings};

#[allow(clippy::too_many_arguments)]
pub(crate) async fn tick_once(
    state: AppState,
    sim: Arc<Mutex<SimState>>,
    ven_name: String,
    vtn: Arc<dyn VtnPort>,
    trigger_tx: Arc<tokio::sync::watch::Sender<PlanTrigger>>,
    data_dir: String,
    _event_tx: PlannerEventTx,
    mut persist_counter: u64,
    persist_every_ticks: u64,
    mut report_counter: u64,
    report_every_ticks: u64,
    tick_s: u64,
) -> (u64, u64) {
    let now = chrono::Utc::now();
    let dt_s = tick_s as f64;

    // PHASE 0: Snapshot — events, inject, plan, capacity, tariffs, overlay flag
    let _events = state.events().await;
    let inject = state.inject_state().await;

    // Pre-tick: snapshot plan/capacity/tariffs for dispatcher
    let plan_snap = state.active_plan().await;
    let capacity_snap = state.capacity_state().await;
    let dispatch_windows = state.dispatch_windows().await;
    let alert_windows = state.alert_windows().await;
    let rates_snap = state.planned_tariffs().await;

    // Compute overlay_enabled: user toggle AND no active EvSession.
    let ev_sess_tick = state.ev_session().await;
    let ev_settings_tick = state.ev_settings().await;
    let session_active = ev_sess_tick.is_some();
    let overlay_enabled = ev_settings_tick.opportunistic_charging_enabled && !session_active;
    if ev_settings_tick.paused_by_active_session != session_active {
        state
            .set_ev_settings(EvSettings {
                paused_by_active_session: session_active,
                ..ev_settings_tick
            })
            .await;
    }

    // Lock sim for physics only — no .await inside the block.
    let (tick_sensor, tick_sim_snap, tick_envelope, cleared_fields, pv_clear, base_clear) = {
        let mut sim_guard = sim.lock().await;

        // PHASE 1: Apply Behaviour A one-shot injections; collect fields to clear.
        let cleared_fields = super::helpers::apply_sim_injections(&inject, &mut sim_guard);
        let pv_clear = inject.pv_irradiance.is_some();
        let base_clear = inject.base_load_kw.is_some();

        // PHASE 2: Build setpoints (dispatcher from MILP plan + capacity + overlay)
        let pre_snap = sim_guard
            .snapshot()
            .expect("SimState::snapshot is infallible");

        // `pre_snap` is taken before physics runs this tick, so its PV power
        // is last tick's value. `peek_pv_kw` previews what physics is about
        // to compute for `now`, avoiding the one-tick lag `pre_snap` would
        // otherwise introduce into the EV-surplus overlay.
        let live_pv_kw =
            sim_guard.peek_pv_kw(now, dt_s, inject.pv_irradiance, inject.pv_irradiance_alpha);

        let sp_map = super::helpers::build_tick_setpoints(
            &pre_snap,
            plan_snap.as_ref(),
            &capacity_snap,
            &inject,
            overlay_enabled,
            now,
            &dispatch_windows,
            &alert_windows,
            live_pv_kw,
        );

        // PHASE 3: Simulator tick — apply setpoints → update device states.
        sim_guard.tick(
            dt_s,
            sp_map,
            now,
            inject.pv_irradiance,
            inject.pv_irradiance_alpha,
            inject.ambient_temp_c,
            inject.heater_temp_min_c,
            inject.heater_temp_max_c,
            inject.base_load_kw,
            inject.base_load_alpha,
            inject.ev_plugged,
            inject.ev_soc_target,
        );

        // PHASE 4 (in-lock): extract snapshots and mutate history/grid/envelope.
        let (tick_sensor, tick_sim_snap, tick_envelope) =
            super::helpers::finalize_tick_outputs(&mut sim_guard, &capacity_snap, now);

        (
            tick_sensor,
            tick_sim_snap,
            tick_envelope,
            cleared_fields,
            pv_clear,
            base_clear,
        )
    };

    // PHASE 1 (post-lock): clear one-shot inject fields.
    for field in cleared_fields {
        state.clear_inject_field(field).await;
    }
    if pv_clear {
        state.clear_inject_field("pv_irradiance").await;
    }
    if base_clear {
        state.clear_inject_field("base_load_kw").await;
    }

    // PHASE 5 (post-lock): async state publishes — sensor, shiftable, ledger, envelope.
    let snap_for_reports = tick_sim_snap.clone();
    let _sim_snapshot = super::publish::publish_sim_tick_result(
        tick_sensor,
        tick_sim_snap,
        tick_envelope,
        plan_snap.as_ref(),
        &state,
        &trigger_tx,
        &rates_snap,
        dt_s,
        now,
    )
    .await;

    // PHASE 6: Periodic measurement reports (T049)
    if report_every_ticks > 0 {
        report_counter += 1;
        if report_counter >= report_every_ticks {
            report_counter = 0;
            super::publish::run_measurement_reports(
                &state,
                &snap_for_reports,
                vtn.as_ref(),
                &ven_name,
                now,
            )
            .await;
        }
    }

    // PHASE 7: Periodic persist
    persist_counter += 1;
    if persist_counter >= persist_every_ticks {
        persist_counter = 0;
        super::publish::persist_sim_state(&sim, &data_dir).await;
    }

    (persist_counter, report_counter)
}

#[cfg(test)]
#[path = "tick_tests.rs"]
mod tick_tests;
