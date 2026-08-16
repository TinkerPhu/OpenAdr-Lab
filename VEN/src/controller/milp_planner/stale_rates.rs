//! WP4.4 (BL-07) — StaleRatePolicy dispatch: price the slots that lie beyond
//! the last known rate data. Pure per-cycle computation, called from
//! `build_milp_inputs` for both the import tariff and (BL-17 closeout) CO2
//! intensity, generically over any `TimeSeries` + coverage-end pair.
//!
//! HEURISTIC_FORECAST is a documented stub until Phase 5 (BL-14, learned
//! rate patterns land): it behaves like LAST_KNOWN and says so in the
//! warning. Export rates keep their step-hold behaviour (no coverage-end
//! tracked for export) — the policy governs the import price and CO2
//! intensity that actually drive scheduling.

use chrono::{DateTime, Utc};

use crate::common::TimeSeries;
use crate::entities::design_vocabulary::StaleRatePolicy;

pub(crate) struct StaleRateOutcome {
    /// Per-slot value, covered slots interpolated (time-weighted mean over
    /// the slot), stale slots filled per policy. Same units as the input
    /// series (€/kWh for import tariff, g/kWh for CO2 intensity).
    pub values: Vec<f64>,
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

/// `label` names the data source in warning text (e.g. "Tariff data", "GHG data").
pub(crate) fn apply_stale_rate_policy(
    policy: &StaleRatePolicy,
    safe_pctl: f64,
    series: &TimeSeries,
    coverage_end: Option<DateTime<Utc>>,
    slot_bounds: &[(DateTime<Utc>, DateTime<Utc>)],
    default_rate: f64,
    label: &str,
) -> StaleRateOutcome {
    let known: Vec<f64> = series.samples.iter().map(|(_, v)| *v).collect();
    let last_known = known.last().copied().unwrap_or(default_rate);
    let mut sorted = known.clone();
    sorted.sort_by(|a, b| a.total_cmp(b));

    let stale_fill = match policy {
        StaleRatePolicy::LastKnown | StaleRatePolicy::HeuristicForecast => last_known,
        StaleRatePolicy::SafeAverage => percentile(&sorted, safe_pctl).unwrap_or(default_rate),
        // Max known rate: discretionary load defers into covered slots —
        // the LP analogue of marking the unknown slots FLEXIBLE.
        StaleRatePolicy::DeferToFlexible => sorted.last().copied().unwrap_or(default_rate),
    };

    let mut values = Vec::with_capacity(slot_bounds.len());
    let mut rate_stale = Vec::with_capacity(slot_bounds.len());
    for &(slot_start, slot_end) in slot_bounds {
        let stale = coverage_end.is_none_or(|cov_end| slot_start >= cov_end);
        rate_stale.push(stale);
        values.push(if stale {
            stale_fill
        } else {
            // R-16 (BL-11): price the slot at its time-weighted mean so a slot
            // straddling a boundary blends both rates instead of taking the
            // slot-start rate for its whole width.
            series
                .time_weighted_mean(slot_start, slot_end)
                .or_else(|| series.interpolate_at(slot_start))
                .unwrap_or(default_rate)
        });
    }

    let warning = rate_stale.iter().any(|&s| s).then(|| match policy {
        StaleRatePolicy::HeuristicForecast => format!(
            "{label} ends before the planning horizon; HEURISTIC_FORECAST is not implemented \
             yet (Phase 5, BL-14) — stale slots fall back to LAST_KNOWN"
        ),
        StaleRatePolicy::LastKnown => {
            format!("{label} ends before the planning horizon; stale slots filled by LAST_KNOWN")
        }
        StaleRatePolicy::SafeAverage => format!(
            "{label} ends before the planning horizon; stale slots filled by SAFE_AVERAGE"
        ),
        StaleRatePolicy::DeferToFlexible => format!(
            "{label} ends before the planning horizon; stale slots deferred by DEFER_TO_FLEXIBLE"
        ),
    });

    StaleRateOutcome {
        values,
        rate_stale,
        warning,
    }
}
