//! WP4.4 (BL-07) — StaleRatePolicy dispatch: price the slots that lie beyond
//! the last known rate data. Pure per-cycle computation, called from
//! `build_milp_inputs` for both the import tariff and (BL-17 closeout) CO2
//! intensity, generically over any `TimeSeries` + coverage-end pair.
//!
//! GB-42: HEURISTIC_FORECAST fills a stale slot from the same clock time 24h
//! earlier (`diurnal_fill`), reusing the current cycle's own known series
//! when the reference lands inside it (always true in steady state, since a
//! stale slot's 24h-back reference sits at/after `now` — see
//! `docs/history/project_journal.md`, search "GB-42"), and falling back to a
//! caller-supplied historical `diurnal_reference` series (sourced from the
//! `grid_samples` history store) when the 24h-back reference crosses a
//! weekday/weekend boundary relative to the slot itself — Friday's shape is
//! a bad estimate for Saturday, so that case reaches back 168h instead.
//! Degrades to LAST_KNOWN when neither source has data for the reference
//! timestamp (e.g. a fresh VEN with no history yet). Export rates keep their
//! step-hold behaviour (no coverage-end tracked for export) — the policy
//! governs the import price and CO2 intensity that actually drive
//! scheduling.

use chrono::{DateTime, Datelike, Duration, Utc, Weekday};

use crate::common::TimeSeries;
use crate::entities::design_vocabulary::StaleRatePolicy;

fn is_weekend(dt: DateTime<Utc>) -> bool {
    matches!(dt.weekday(), Weekday::Sat | Weekday::Sun)
}

/// GB-42: diurnal-persistence fill for one stale slot. `series` is this
/// cycle's own known (covered) series — checked first for the 24h-back
/// reference, since that timestamp is always `>= now` in steady state and
/// therefore still inside currently-published data, not genuine history.
/// `diurnal_reference`, when present, is history-store-backed and used for
/// the 168h-back reference (day-type mismatch) or as a second-chance lookup.
/// Falls back to `last_known` (today's LAST_KNOWN behavior) when neither
/// source covers either reference timestamp.
fn diurnal_fill(
    slot_start: DateTime<Utc>,
    series: &TimeSeries,
    diurnal_reference: Option<&TimeSeries>,
    last_known: f64,
) -> f64 {
    let ref_24 = slot_start - Duration::hours(24);
    let ref_168 = slot_start - Duration::hours(24 * 7);
    let same_day_type = is_weekend(ref_24) == is_weekend(slot_start);

    // Only trust the series' 24h-back value when the day types actually match —
    // falling through to it on a mismatch (e.g. Friday's shape for a Saturday
    // slot) would silently defeat the guard this function exists to enforce.
    // ref_168 is always day-type-matched by construction (168h = exactly one
    // week), so it's a safe second chance in either branch.
    let same_day_value = same_day_type
        .then(|| series.interpolate_at(ref_24))
        .flatten();

    same_day_value
        .or_else(|| diurnal_reference.and_then(|h| h.interpolate_at(ref_168)))
        .unwrap_or(last_known)
}

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
/// `diurnal_reference`: history-store-backed series for GB-42's HEURISTIC_FORECAST
/// 168h-back lookback (day-type mismatch case); `None` when no history is
/// configured or available yet — see `diurnal_fill`.
#[allow(clippy::too_many_arguments)]
pub(crate) fn apply_stale_rate_policy(
    policy: &StaleRatePolicy,
    safe_pctl: f64,
    series: &TimeSeries,
    coverage_end: Option<DateTime<Utc>>,
    slot_bounds: &[(DateTime<Utc>, DateTime<Utc>)],
    default_rate: f64,
    label: &str,
    diurnal_reference: Option<&TimeSeries>,
) -> StaleRateOutcome {
    let known: Vec<f64> = series.samples.iter().map(|(_, v)| *v).collect();
    let last_known = known.last().copied().unwrap_or(default_rate);
    let mut sorted = known.clone();
    sorted.sort_by(|a, b| a.total_cmp(b));

    let stale_fill = match policy {
        StaleRatePolicy::LastKnown => last_known,
        StaleRatePolicy::SafeAverage => percentile(&sorted, safe_pctl).unwrap_or(default_rate),
        // Max known rate: discretionary load defers into covered slots —
        // the LP analogue of marking the unknown slots FLEXIBLE.
        StaleRatePolicy::DeferToFlexible => sorted.last().copied().unwrap_or(default_rate),
        // GB-42: HEURISTIC_FORECAST's fill varies per slot (diurnal_fill below),
        // so it has no single precomputed scalar — this arm is never read.
        StaleRatePolicy::HeuristicForecast => last_known,
    };

    let mut values = Vec::with_capacity(slot_bounds.len());
    let mut rate_stale = Vec::with_capacity(slot_bounds.len());
    for &(slot_start, slot_end) in slot_bounds {
        let stale = coverage_end.is_none_or(|cov_end| slot_start >= cov_end);
        rate_stale.push(stale);
        values.push(if stale {
            if matches!(policy, StaleRatePolicy::HeuristicForecast) {
                diurnal_fill(slot_start, series, diurnal_reference, last_known)
            } else {
                stale_fill
            }
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
            "{label} ends before the planning horizon; HEURISTIC_FORECAST fills stale slots \
             from a 24h/168h diurnal reference, degrading to LAST_KNOWN where reference data \
             is insufficient"
        ),
        StaleRatePolicy::LastKnown => {
            format!("{label} ends before the planning horizon; stale slots filled by LAST_KNOWN")
        }
        StaleRatePolicy::SafeAverage => {
            format!("{label} ends before the planning horizon; stale slots filled by SAFE_AVERAGE")
        }
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
