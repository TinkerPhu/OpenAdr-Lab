//! WP4.4 (BL-07) — StaleRatePolicy dispatch: price the slots that lie beyond
//! the last known import-rate data. Pure per-cycle computation, called from
//! `build_milp_inputs`.
//!
//! HEURISTIC_FORECAST is a documented stub until Phase 5 (BL-14, learned
//! rate patterns land): it behaves like LAST_KNOWN and says so in the
//! warning. Export and CO₂ rates keep their step-hold behaviour — the policy
//! governs the import price that actually drives scheduling.

use chrono::{DateTime, Utc};

use crate::entities::design_vocabulary::StaleRatePolicy;
use crate::entities::tariff_snapshot::TariffTimeSeries;

pub(crate) struct StaleRateOutcome {
    /// Per-slot import rate [€/kWh], covered slots interpolated, stale slots
    /// filled per policy.
    pub c_imp_eur_kwh: Vec<f64>,
    /// Per-slot staleness flag (drives `PlanTimeSlot.rate_estimated`).
    pub rate_stale: Vec<bool>,
    /// Stable warning text when any slot is stale (WP4.3 notification dedup
    /// relies on the text not changing between plans), `None` otherwise.
    pub warning: Option<String>,
}

/// Nearest-rank percentile of the known rates (pctl clamped to [0, 1]).
fn percentile(sorted: &[f64], pctl: f64) -> Option<f64> {
    if sorted.is_empty() {
        return None;
    }
    let rank = (pctl.clamp(0.0, 1.0) * sorted.len() as f64).ceil() as usize;
    Some(sorted[rank.clamp(1, sorted.len()) - 1])
}

pub(crate) fn apply_stale_rate_policy(
    policy: &StaleRatePolicy,
    safe_pctl: f64,
    tariffs: &TariffTimeSeries,
    slot_starts: &[DateTime<Utc>],
    default_rate_eur_kwh: f64,
) -> StaleRateOutcome {
    let known: Vec<f64> = tariffs
        .import_eur_kwh
        .samples
        .iter()
        .map(|(_, v)| *v)
        .collect();
    let last_known = known.last().copied().unwrap_or(default_rate_eur_kwh);
    let mut sorted = known.clone();
    sorted.sort_by(|a, b| a.total_cmp(b));

    let stale_fill = match policy {
        StaleRatePolicy::LastKnown | StaleRatePolicy::HeuristicForecast => last_known,
        StaleRatePolicy::SafeAverage => {
            percentile(&sorted, safe_pctl).unwrap_or(default_rate_eur_kwh)
        }
        // Max known rate: discretionary load defers into covered slots —
        // the LP analogue of marking the unknown slots FLEXIBLE.
        StaleRatePolicy::DeferToFlexible => sorted.last().copied().unwrap_or(default_rate_eur_kwh),
    };

    let mut c_imp = Vec::with_capacity(slot_starts.len());
    let mut rate_stale = Vec::with_capacity(slot_starts.len());
    for &slot_t in slot_starts {
        let stale = tariffs
            .import_coverage_end
            .is_none_or(|cov_end| slot_t >= cov_end);
        rate_stale.push(stale);
        c_imp.push(if stale {
            stale_fill
        } else {
            tariffs
                .import_eur_kwh
                .interpolate_at(slot_t)
                .unwrap_or(default_rate_eur_kwh)
        });
    }

    let warning = rate_stale.iter().any(|&s| s).then(|| match policy {
        StaleRatePolicy::HeuristicForecast => "Tariff data ends before the planning horizon; \
             HEURISTIC_FORECAST is not implemented yet (Phase 5, BL-14) — stale slots fall back \
             to LAST_KNOWN"
            .to_string(),
        StaleRatePolicy::LastKnown => {
            "Tariff data ends before the planning horizon; stale slots filled by LAST_KNOWN"
                .to_string()
        }
        StaleRatePolicy::SafeAverage => {
            "Tariff data ends before the planning horizon; stale slots filled by SAFE_AVERAGE"
                .to_string()
        }
        StaleRatePolicy::DeferToFlexible => "Tariff data ends before the planning horizon; \
             stale slots deferred by DEFER_TO_FLEXIBLE"
            .to_string(),
    });

    StaleRateOutcome {
        c_imp_eur_kwh: c_imp,
        rate_stale,
        warning,
    }
}
