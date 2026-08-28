//! GB-41 diagnostic: split out of `results.rs` to stay under the VEN/src/
//! 500-production-line cap (`ven-architecture` rule, `.claude/CLAUDE.md`).
//!
//! A soft-deadline EV session (`MilpLoadMode::MayRun`) legitimately lets the solver
//! choose `z_ev_core = 0` for the whole horizon when charging doesn't pay for itself —
//! but from outside the solve, that looks identical to a stuck/inert EV. The GB-41
//! investigation (four of nine fleet EVs charged nothing across a 24h run) found no
//! historical record of which case it was, because per-slot planned power isn't
//! persisted, only `PlanWarning::kind`. This warning makes the legitimate-skip case
//! visible going forward without needing a live reproduction to tell them apart.

use crate::entities::asset_params::EvParams;
use crate::entities::device_session::EvSession;
use crate::entities::plan::{PlanWarning, WarningKind, WarningSeverity};

use super::types::{MilpInputs, MilpLoadMode, SolveOutput};

pub(super) fn unmet_warning(
    inputs: &MilpInputs,
    sol: &SolveOutput,
    ev_session: Option<&EvSession>,
    ev_cfg: Option<&EvParams>,
) -> Option<PlanWarning> {
    if inputs.ev_mode != MilpLoadMode::MayRun || sol.z_ev_core >= 0.5 {
        return None;
    }
    let session = ev_session?;
    let ev_cfg = ev_cfg?;
    let current_soc = inputs.soc_ev_init.unwrap_or(session.target_soc);
    let core_kwh = ((session.target_soc - current_soc) * ev_cfg.battery_kwh).max(0.0);
    if core_kwh <= 1e-6 {
        return None;
    }
    Some(PlanWarning {
        severity: WarningSeverity::Warning,
        kind: WarningKind::EvCoreEnergyUnmet,
        message: format!(
            "EV '{}' soft-deadline session wants {core_kwh:.1} kWh more (soc {:.0}% -> target {:.0}% by {}) but the plan schedules none of it — the solver found charging not worth its cost this cycle",
            ev_cfg.id,
            current_soc * 100.0,
            session.target_soc * 100.0,
            session.departure_time.format("%H:%M"),
        ),
        suggested_action: Some(
            "check the session's comfort rate / v_ev_core_eur_kwh against the current tariff — raise it if the EV should charge regardless of price".to_string(),
        ),
    })
}
