// Simulator tick loop body, extracted to keep sim_tick/mod.rs under 200 lines.

use chrono::Utc;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{debug, info, warn};

use crate::controller;
use crate::entities::asset::PlanTrigger;
use crate::planner_events::PlannerEventTx;
use crate::profile::Profile;
use crate::simulator::SimState;
use crate::state::{AppState, EvSettings};
use crate::vtn::VtnClient;
use crate::controller::absorber::AbsorberState;

pub(crate) async fn tick_once(
    mut absorber_state: AbsorberState,
    state: AppState,
    sim: Arc<Mutex<SimState>>,
    profile: Arc<Profile>,
    ven_name: String,
    vtn: VtnClient,
    trigger_tx: Arc<tokio::sync::watch::Sender<PlanTrigger>>,
    data_dir: String,
    event_tx: PlannerEventTx,
    deviation_pending: Arc<std::sync::atomic::AtomicBool>,
    mut persist_counter: u64,
    persist_every_ticks: u64,
    mut report_counter: u64,
    report_every_ticks: u64,
    tick_s: u64,
) -> (AbsorberState, u64, u64) {
    let now = chrono::Utc::now();
    let dt_s = tick_s as f64;

    // PHASE 0: Snapshot — events, inject, plan, capacity, tariffs, overlay flag
    let _events = state.events().await;
    let inject = state.inject_state().await;

    // Pre-tick: snapshot plan/capacity/tariffs for dispatcher
    let plan_snap = state.active_plan().await;
    let capacity_snap = state.capacity_state().await;
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
    let (
        tick_sensor,
        tick_sim_snap,
        tick_envelope,
        cleared_fields,
        pv_clear,
        base_clear,
        residual_kw,
    ) = {
        let mut sim_guard = sim.lock().await;

        // PHASE 1: Apply Behaviour A one-shot injections; collect fields to clear.
        let cleared_fields = super::helpers::apply_sim_injections(&inject, &mut *sim_guard);
        let pv_clear = inject.pv_irradiance.is_some();
        let base_clear = inject.base_load_kw.is_some();

        // PHASE 2: Build setpoints (prev grid for Layer 1; effective capacity; dispatcher)
        let prev_actual_net_kw = sim_guard.grid.net_power_w / 1000.0;
        let plan_signed_net_kw = plan_snap
            .as_ref()
            .and_then(|p| p.current_slot(now))
            .map(|s| s.net_import_kw - s.net_export_kw)
            .unwrap_or(0.0);

        let mut sp_map = super::helpers::build_tick_setpoints(
            &*sim_guard,
            plan_snap.as_ref(),
            &capacity_snap,
            &inject,
            overlay_enabled,
            now,
        );

        // PHASE 3: Layer 1 multi-asset deviation absorber (Tier 1 real-time control).
        let deviation_kw = prev_actual_net_kw - plan_signed_net_kw;
        let residual_kw = controller::absorber::apply_deviation_absorption(
            &mut absorber_state,
            deviation_kw,
            &mut sp_map,
            &*sim_guard,
            plan_snap.as_ref(),
            &profile,
            now,
            &event_tx,
            ev_sess_tick.as_ref(),
        );

        // PHASE 4: Simulator tick — apply setpoints → update device states.
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

        // PHASE 5 (in-lock): extract snapshots and mutate history/grid/envelope.
        let (tick_sensor, tick_sim_snap, tick_envelope) =
            super::helpers::finalize_tick_outputs(&mut *sim_guard, &capacity_snap, now);

        (
            tick_sensor,
            tick_sim_snap,
            tick_envelope,
            cleared_fields,
            pv_clear,
            base_clear,
            residual_kw,
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
    let _sim_snapshot = super::publish::publish_sim_tick_result(
        tick_sensor,
        tick_sim_snap,
        tick_envelope,
        plan_snap.as_ref(),
        &state,
        &*trigger_tx,
        &rates_snap,
        dt_s,
        now,
    )
    .await;

    // PHASE 6: Layer 2 — accumulate absorbed residual deviation → DeviceDeviation trigger.
    super::helpers::accumulate_deviation(
        &mut absorber_state,
        residual_kw,
        &profile,
        &*trigger_tx,
        &deviation_pending,
        now,
    );

    // PHASE 7: Periodic measurement reports (T049)
    if report_every_ticks > 0 {
        report_counter += 1;
        if report_counter >= report_every_ticks {
            report_counter = 0;
            super::publish::run_measurement_reports(&state, &sim, &vtn, &ven_name, now).await;
        }
    }

    // PHASE 8: Periodic persist
    persist_counter += 1;
    if persist_counter >= persist_every_ticks {
        persist_counter = 0;
        super::publish::persist_sim_state(&sim, &data_dir).await;
    }

    (absorber_state, persist_counter, report_counter)
}
