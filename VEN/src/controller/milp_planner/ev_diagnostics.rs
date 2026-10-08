//! The EV's "you were promised energy and did not get it" warning. Split out of
//! `results.rs` to stay under the VEN/src/ 500-production-line cap
//! (`ven-architecture` rule, `.claude/CLAUDE.md`).
//!
//! `ev-comfort-piecewise-core` narrowed this to one meaning: a **firm** request
//! whose guaranteed energy the plan cannot deliver in the window it has. That is
//! the only case where the system promised something and fell short.
//!
//! It deliberately no longer fires for a soft request that charges less than its
//! target. Under the comfort curve a soft target is a preference priced per kWh,
//! so "charged 14 of 25 kWh because the bid stopped covering the cost" is the
//! model working as asked, not an unmet obligation — the old all-or-nothing core
//! is what made that look like a failure (GB-41).

use crate::entities::plan::{PlanWarning, WarningKind, WarningSeverity};

use super::types::{MilpInputs, SolveOutput};

/// The EV shortfall warning for this cycle, if any.
pub(super) fn ev_warnings(inputs: &MilpInputs, sol: &SolveOutput) -> Vec<PlanWarning> {
    firm_shortfall(inputs, sol).into_iter().collect()
}

/// A firm obligation the plan could not meet: the window is too short, the car is
/// away for too much of it, or the charger cannot deliver it in time.
///
/// R-93: the gap is now the solved shortfall slack, not a comparison the diagnostic
/// re-derives. The model itself decided how much it had to give up, so there is no
/// second rule here that can disagree with the plan — and the slack is per
/// obligation, so the warning can say which request fell short.
fn firm_shortfall(inputs: &MilpInputs, sol: &SolveOutput) -> Option<PlanWarning> {
    let (idx, short) = sol
        .ev_shortfall_kwh
        .iter()
        .enumerate()
        .filter(|(_, &kwh)| kwh > 1e-3)
        .max_by(|a, b| a.1.total_cmp(b.1))
        .map(|(i, &kwh)| (i, kwh))?;
    let ob = inputs.ev_obligations.get(idx)?;
    let required =
        ((ob.target_soc_frac - inputs.soc_ev_init.unwrap_or(0.0)) * inputs.ev_battery_kwh).max(0.0);
    let delivered = (required - short).max(0.0);
    let whose = match ob.session_id {
        Some(id) => format!(" for request {id}"),
        None => String::new(),
    };
    Some(PlanWarning {
        severity: WarningSeverity::Warning,
        kind: WarningKind::EvCoreEnergyUnmet,
        message: format!(
            "EV charging{whose} falls {short:.1} kWh short of its guaranteed {required:.1} kWh              before the deadline — the plan delivers {delivered:.1} kWh, which is all the              available window allows"
        ),
        suggested_action: Some(
            "move the deadline later, lower the target, or check whether the car is predicted              away for part of the window"
                .to_string(),
        ),
    })
}
