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

/// A firm requirement the plan could not meet: the window is too short, the car
/// is away for too much of it, or the charger cannot deliver it in time.
fn firm_shortfall(inputs: &MilpInputs, sol: &SolveOutput) -> Option<PlanWarning> {
    let required = inputs.e_ev_required_kwh;
    if required <= 1e-6 {
        return None;
    }
    let delivered = sol.e_seg_kwh;
    if delivered >= required - 1e-3 {
        return None;
    }
    let short = required - delivered;
    Some(PlanWarning {
        severity: WarningSeverity::Warning,
        kind: WarningKind::EvCoreEnergyUnmet,
        message: format!(
            "EV charging falls {short:.1} kWh short of its guaranteed {required:.1} kWh \
             before the deadline — the plan delivers {delivered:.1} kWh, which is all the \
             available window allows"
        ),
        suggested_action: Some(
            "move the deadline later, lower the target, or check whether the car is predicted \
             away for part of the window"
                .to_string(),
        ),
    })
}
