//! Every long-running loop this process owns, spawned in one place.
//!
//! Each is wrapped in `tasks::supervised_spawn`, which restarts it after a
//! panic; the closure handed there must therefore be able to rebuild the
//! task from scratch, which is why every captured value is cloned into it
//! rather than moved.

use std::sync::Arc;
use tokio::sync::watch;

use super::World;
use crate::entities::asset::PlanTriggerSignal;
use crate::tasks;

/// Seconds a supervised task waits before restarting after a panic.
const TASK_COOLDOWN_S: u64 = 5;

pub fn spawn_all(w: &World, trigger_rx: watch::Receiver<PlanTriggerSignal>) {
    spawn_vtn_polls(w);
    spawn_sim_tick(w);
    spawn_obligation_check(w);
    spawn_planning(w, trigger_rx);
    spawn_state_persist(w);
    spawn_history_jobs(w);
}

/// GB-09: poll cadence and startup jitter are resolved once per process, so
/// the jitter draw is fixed for its lifetime — matching the old fixed-seconds
/// jitter's semantics, just profile-configurable and percentage-based.
fn spawn_vtn_polls(w: &World) {
    let (state, vtn, notifier) = (&w.state, &w.ports.vtn_port, &w.notifier);
    let jitter_s = w.poll_jitter_s;

    let (s, v, n, secs) = (
        state.clone(),
        vtn.clone(),
        notifier.clone(),
        w.poll.programs_secs,
    );
    tasks::supervised_spawn("poll_programs", TASK_COOLDOWN_S, state.clone(), move || {
        tasks::spawn_program_poll(s.clone(), v.clone(), secs, jitter_s, n.clone())
    });

    let (s, v, n, secs, tx, h, seed) = (
        state.clone(),
        vtn.clone(),
        notifier.clone(),
        w.poll.events_secs,
        w.trigger_tx.clone(),
        w.ports.history.clone(),
        w.ven_name.clone(),
    );
    tasks::supervised_spawn("poll_events", TASK_COOLDOWN_S, state.clone(), move || {
        tasks::spawn_event_poll(
            s.clone(),
            v.clone(),
            tasks::EventPollTiming {
                secs,
                startup_delay_s: jitter_s,
                ven_seed: seed.clone(),
            },
            tx.clone(),
            n.clone(),
            h.clone(),
        )
    });

    let (s, v, n, secs) = (
        state.clone(),
        vtn.clone(),
        notifier.clone(),
        w.poll.reports_secs,
    );
    tasks::supervised_spawn("poll_reports", TASK_COOLDOWN_S, state.clone(), move || {
        tasks::spawn_report_poll(s.clone(), v.clone(), secs, jitter_s, n.clone())
    });
}

fn spawn_sim_tick(w: &World) {
    let (s, sim, sp, vn, tx, dd, etx, wp, wpp, pvm, pvme, blm, blme, nf, cl, tp) = (
        w.state.clone(),
        w.sim.clone(),
        w.sim_params.clone(),
        w.ven_name.clone(),
        w.trigger_tx.clone(),
        w.data_dir.clone(),
        w.planner_event_tx.clone(),
        w.ports.weather.clone(),
        w.weather_pv_params,
        w.ports.pv_measurement.clone(),
        w.pv_measurement_enabled,
        w.ports.base_load_measurement.clone(),
        w.base_load_measurement_enabled,
        w.notifier.clone(),
        w.comms_loss,
        w.ports.telemetry.clone(),
    );
    let (gi, ge) = (w.grid_max_import_kw, w.grid_max_export_kw);
    tasks::supervised_spawn("sim_tick", TASK_COOLDOWN_S, w.state.clone(), move || {
        tasks::spawn_sim_tick(
            s.clone(),
            sim.clone(),
            sp.clone(),
            vn.clone(),
            tx.clone(),
            dd.clone(),
            etx.clone(),
            wp.clone(),
            wpp,
            pvm.clone(),
            pvme,
            blm.clone(),
            blme,
            nf.clone(),
            cl,
            (gi, ge),
            tp.clone(),
        )
    });
}

fn spawn_obligation_check(w: &World) {
    let (s, sim, v, vn, h) = (
        w.state.clone(),
        w.sim.clone(),
        w.ports.vtn_port.clone(),
        w.ven_name.clone(),
        w.ports.history.clone(),
    );
    tasks::supervised_spawn(
        "obligation_check",
        TASK_COOLDOWN_S,
        w.state.clone(),
        move || {
            tasks::spawn_obligation_check(s.clone(), sim.clone(), v.clone(), vn.clone(), h.clone())
        },
    );
}

fn spawn_planning(w: &World, trigger_rx: watch::Receiver<PlanTriggerSignal>) {
    // GB-54: a stable per-VEN phase in the replan grid, so a fleet sharing a
    // host does not solve in lockstep — and so a deploy that restarts all of
    // them does not align them, which is how GB-54 was found.
    let replan_offset_s = crate::entities::planner_params::replan_phase_offset_s(
        &w.ven_name,
        w.planner_params.replan_interval_s,
    );
    let (s, pp, ap, sv, rx, sim, ao, etx, nf, wp, wpp, hp) = (
        w.state.clone(),
        w.planner_params.clone(),
        w.asset_params.clone(),
        w.ports.solver.clone(),
        trigger_rx,
        w.sim.clone(),
        w.active_objective.clone(),
        w.planner_event_tx.clone(),
        w.notifier.clone(),
        w.ports.weather.clone(),
        w.weather_pv_params,
        w.ports.history.clone(),
    );
    let (gi, ge) = (w.grid_max_import_kw, w.grid_max_export_kw);
    tasks::supervised_spawn("planning", TASK_COOLDOWN_S, w.state.clone(), move || {
        tasks::spawn_planning(
            s.clone(),
            pp.clone(),
            gi,
            ge,
            ap.clone(),
            sv.clone(),
            rx.clone(),
            sim.clone(),
            ao.clone(),
            etx.clone(),
            nf.clone(),
            chrono::Utc::now,
            wp.clone(),
            wpp,
            hp.clone(),
            replan_offset_s,
        )
    });
}

fn spawn_state_persist(w: &World) {
    let Some(path) = w.persist_path.clone() else {
        return;
    };
    let s = w.state.clone();
    tasks::supervised_spawn(
        "state_persist",
        TASK_COOLDOWN_S,
        w.state.clone(),
        move || tasks::spawn_state_persist(s.clone(), path.clone()),
    );
}

/// The history sampler, the base-load window refresher and the daily heuristics
/// learner — all no-ops without a history store, so all are gated on the one `Option`.
fn spawn_history_jobs(w: &World) {
    let Some(history) = w.ports.history.clone() else {
        return;
    };

    let (s, sim, retention_days, n) = (
        w.state.clone(),
        w.sim.clone(),
        w.history_retention_days,
        w.notifier.clone(),
    );
    let h = history.clone();
    tasks::supervised_spawn(
        "history_sampler",
        TASK_COOLDOWN_S,
        w.state.clone(),
        move || {
            tasks::spawn_history_sampler(
                sim.clone(),
                h.clone(),
                s.clone(),
                retention_days,
                n.clone(),
            )
        },
    );

    let (sim, h) = (w.sim.clone(), history.clone());
    tasks::supervised_spawn(
        "base_load_window",
        TASK_COOLDOWN_S,
        w.state.clone(),
        move || tasks::spawn_base_load_window(h.clone(), sim.clone()),
    );

    let (s, cfg) = (w.state.clone(), w.heuristics_config);
    tasks::supervised_spawn(
        "heuristics_job",
        TASK_COOLDOWN_S,
        w.state.clone(),
        move || tasks::spawn_heuristics_job(history.clone(), s.clone(), cfg),
    );
}

/// Controller decisions on the fleet channel (§6.2). Not supervised and not
/// part of `spawn_all`: it is a fan-out of a broadcast the controller already
/// publishes, spawned during assembly precisely so a decision does not become
/// slower or fallible for being watched.
pub fn spawn_fleet_trace(
    state: &crate::state::AppState,
    telemetry: Arc<dyn crate::controller::telemetry_port::TelemetryPort>,
    ven_name: String,
) {
    tasks::fleet_trace::spawn(state.subscribe_controller_trace(), telemetry, ven_name);
}
