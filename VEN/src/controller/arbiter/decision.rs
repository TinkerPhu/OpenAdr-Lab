//! What the controller event log records about the arbiter: decisions, not ticks.

use super::DEAD_BAND_KW;

/// The `ControllerEvent::ArbiterDecision` for `pass` when its decision changed
/// since last tick — a different leading lever, or an unresolved excess
/// appearing or clearing (above `DEAD_BAND_KW`). `prev`/`now` are
/// `(active_lever, unresolved_kw)`; `None` when nothing changed, so the event
/// log records decisions, not ticks.
pub fn decision_event(
    pass: &str,
    prev: (Option<&str>, f64),
    now: (Option<&str>, f64),
    target_kw: Option<f64>,
    excess_kw: Option<f64>,
    ts: chrono::DateTime<chrono::Utc>,
) -> Option<crate::controller::trace::ControllerEvent> {
    let unresolved = |unresolved_kw: f64| unresolved_kw > DEAD_BAND_KW;
    let changed = prev.0 != now.0 || unresolved(prev.1) != unresolved(now.1);
    changed.then(
        || crate::controller::trace::ControllerEvent::ArbiterDecision {
            ts,
            pass: pass.to_string(),
            active_lever: now.0.map(str::to_string),
            target_kw,
            excess_kw,
            unresolved_kw: now.1,
        },
    )
}
