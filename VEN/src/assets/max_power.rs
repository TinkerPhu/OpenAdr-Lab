//! `asset_max_power` (`asset-max-power-primitive` D4) and the `Trajectory`
//! types `Asset::simulate_forward` produces — split out of `asset_trait.rs`
//! to stay under that file's 500-production-line budget.

use chrono::{DateTime, Duration, Utc};

use super::{Asset, AssetState};
use crate::entities::capacity_curve::{CommitmentDirection, LimitTier};

/// Trajectory produced by simulate_forward().
pub struct Trajectory {
    pub points: Vec<TrajectoryPoint>,
}

/// State is the state AFTER the step at `ts`.
// `ts`/`power_kw` are read only by the tests that pin `simulate_forward`'s
// contract — that each point pairs the state BEFORE its window's step with the
// *actual* (possibly clamped) power achieved DURING it, at that window's own
// start. `insert_simulated_points`, the sole production consumer, relies on
// exactly that alignment while reading only `state`, so the fields document and
// guard an invariant it depends on rather than being unused scaffolding.
#[allow(dead_code)]
pub struct TrajectoryPoint {
    pub ts: DateTime<Utc>,
    /// Signed: positive = import, negative = export.
    pub power_kw: f64,
    pub state: AssetState,
}

/// Same "if this asset committed now to a sustained extreme in `direction`
/// under `tier`, held from `t1`" question as `asset_max_power`, but reporting
/// every elapsed point along the way instead of only the final one —
/// `(elapsed_s, power_kw, cumulative_energy_kwh)` triples, one per
/// `max_effort_schedule`'s own fine-grained (60s-resolution) step.
///
/// Built once, from a single schedule/`simulate_forward` walk out to
/// `t1 + t2_max`, specifically so a caller sweeping many different `t2`
/// samples (e.g. a capacity-forecast curve) doesn't re-walk the schedule
/// from scratch for each sample — `O(t2_max / 60s)` total work, not
/// `O(samples²)` (`unified-capacity-envelope-engine` D2). `asset_max_power`
/// itself is defined in terms of this (reading the last point), so the two
/// can never independently diverge.
///
/// Not yet called from production code -- `unified-capacity-envelope-engine`
/// (Spec E) is what wires this into the Capacity Forecast's `t2` sweep.
#[allow(dead_code)]
pub fn asset_max_power_series(
    asset: &dyn Asset,
    state: &AssetState,
    t1: DateTime<Utc>,
    t2_max: Duration,
    direction: CommitmentDirection,
    tier: LimitTier,
) -> Vec<(i64, f64, f64)> {
    let t_end = t1 + t2_max;
    let schedule = asset.max_effort_schedule(state, direction, tier, t1, t_end);
    let trajectory = asset.simulate_forward(state, &schedule);
    let mut cumulative_energy_kwh = 0.0;
    let mut series = Vec::with_capacity(trajectory.points.len());
    for (i, point) in trajectory.points.iter().enumerate() {
        let elapsed_s = (point.ts - t1).num_seconds();
        series.push((elapsed_s, point.power_kw, cumulative_energy_kwh));
        // Mirrors `CapacityCurve::energy_kwh_total`'s own windows(2)
        // integration: each point holds constant power until the next one.
        if let Some(next) = trajectory.points.get(i + 1) {
            let dt_h = (next.ts - point.ts).num_milliseconds() as f64 / 3_600_000.0;
            cumulative_energy_kwh += point.power_kw.abs() * dt_h;
        }
    }
    series
}

/// "If this asset committed now to a sustained extreme in `direction` under
/// `tier`, held from `t1` for `t2`, what power is it still delivering at the
/// end and how much energy flowed?" (`asset-max-power-primitive` D4). Pure
/// composition over `asset_max_power_series` — no simulation logic of its
/// own. Returns `(power_kw_at_t1_plus_t2, energy_kwh)`.
#[allow(dead_code)]
pub fn asset_max_power(
    asset: &dyn Asset,
    state: &AssetState,
    t1: DateTime<Utc>,
    t2: Duration,
    direction: CommitmentDirection,
    tier: LimitTier,
) -> (f64, f64) {
    asset_max_power_series(asset, state, t1, t2, direction, tier)
        .last()
        .map(|&(_, power_kw, energy_kwh)| (power_kw, energy_kwh))
        .unwrap_or((0.0, 0.0))
}

#[cfg(test)]
mod asset_max_power_tests {
    //! `asset-max-power-primitive`: `max_effort_schedule`'s fine-grained
    //! default body + `asset_max_power`'s composition, verified against a
    //! worked numeric example — the whole reason for fine-grained stepping
    //! instead of one coarse `simulate_forward` call is that `step()` doesn't
    //! detect exhaustion *within* an over-long `dt` (confirmed against
    //! `Battery::step_inner`); a naive 2-point schedule would silently
    //! over-report both power and energy once the window outlasts exhaustion.

    use super::*;
    use crate::assets::battery::{Battery, BatteryState};

    fn battery(capacity_kwh: f64, max_kw: f64, soc: f64) -> (Battery, AssetState) {
        (
            Battery {
                capacity_kwh,
                max_charge_kw: max_kw,
                max_discharge_kw: max_kw,
                round_trip_efficiency: 1.0,
                min_soc: 0.1,
            },
            AssetState::Battery(BatteryState {
                soc,
                actual_power_kw: 0.0,
            }),
        )
    }

    #[test]
    fn max_effort_schedule_default_body_detects_exhaustion_within_the_window() {
        // 10 kWh capacity, 5 kW charge rate, soc=0.9 -> only 1 kWh headroom,
        // fully charged in 0.2h. Query a 1h window: a naive 2-point schedule
        // would report 5.0 kW constant and 5.0 kWh energy (wrong); the
        // fine-grained default must detect the SoC ceiling partway through.
        let (bat, state) = battery(10.0, 5.0, 0.9);
        let t1 = Utc::now();
        let t_end = t1 + Duration::hours(1);
        let schedule = Asset::max_effort_schedule(
            &bat,
            &state,
            CommitmentDirection::Import,
            LimitTier::Physical,
            t1,
            t_end,
        );
        let trajectory = bat.simulate_forward(&state, &schedule);
        let final_power = trajectory.points.last().unwrap().power_kw;
        assert_eq!(
            final_power, 0.0,
            "battery must report 0 kW once full, not the constant 5 kW request"
        );
    }

    #[test]
    fn asset_max_power_matches_manual_schedule_and_reports_correct_energy() {
        // Same setup: exactly 1 kWh of headroom, so asset_max_power's energy
        // must be ~1.0 kWh, not 5.0 kWh (the naive-coarse-step bug's answer).
        let (bat, state) = battery(10.0, 5.0, 0.9);
        let t1 = Utc::now();
        let t2 = Duration::hours(1);
        let (power_kw, energy_kwh) = asset_max_power(
            &bat,
            &state,
            t1,
            t2,
            CommitmentDirection::Import,
            LimitTier::Physical,
        );
        assert_eq!(power_kw, 0.0, "must be 0 kW at t_end, already full");
        // Tolerance is one 60s-resolution step's worth of energy (5 kW *
        // 1/60 h ≈ 0.083 kWh), not a tight bound: 1 kWh / 5 kW lands exactly
        // on a 60-step boundary, so which side of the SoC>=1.0 check the
        // final step falls on is sensitive to floating-point rounding in the
        // repeating-decimal 1/60h arithmetic -- a real discretization
        // characteristic of the fine-grained default body, not a bug (the
        // ample-headroom test below confirms the non-boundary-exact case is
        // precise well within this tolerance).
        assert!(
            (energy_kwh - 1.0).abs() < 0.09,
            "expected ~1.0 kWh of real headroom (±1 discretization step), got {energy_kwh}"
        );
    }

    #[test]
    fn asset_max_power_with_ample_headroom_delivers_the_full_rate_throughout() {
        // 10 kWh capacity, 5 kW rate, soc=0.5 -> 5 kWh headroom, needs 1h to
        // fill. A 0.5h window should never hit the ceiling.
        let (bat, state) = battery(10.0, 5.0, 0.5);
        let t1 = Utc::now();
        let t2 = Duration::minutes(30);
        let (power_kw, energy_kwh) = asset_max_power(
            &bat,
            &state,
            t1,
            t2,
            CommitmentDirection::Import,
            LimitTier::Physical,
        );
        assert_eq!(power_kw, 5.0);
        assert!(
            (energy_kwh - 2.5).abs() < 0.01,
            "expected 5.0 kW * 0.5h = 2.5 kWh, got {energy_kwh}"
        );
    }

    // ── asset_max_power_series (unified-capacity-envelope-engine D2) ───────

    #[test]
    fn asset_max_power_series_last_point_matches_asset_max_power() {
        // Same boundary-exact 1kWh/5kW headroom fixture as the test above --
        // confirms asset_max_power (now defined in terms of the series) still
        // reports the identical answer it always did, not a second,
        // independently-diverging computation.
        let (bat, state) = battery(10.0, 5.0, 0.9);
        let t1 = Utc::now();
        let t2 = Duration::hours(1);
        let series = asset_max_power_series(
            &bat,
            &state,
            t1,
            t2,
            CommitmentDirection::Import,
            LimitTier::Physical,
        );
        let &(_, series_power_kw, series_energy_kwh) = series.last().unwrap();
        let (power_kw, energy_kwh) = asset_max_power(
            &bat,
            &state,
            t1,
            t2,
            CommitmentDirection::Import,
            LimitTier::Physical,
        );
        assert_eq!(series_power_kw, power_kw);
        assert_eq!(series_energy_kwh, energy_kwh);
    }

    #[test]
    fn asset_max_power_series_reports_every_60s_step_not_just_the_endpoint() {
        // Ample headroom (never hits the ceiling within t2_max), so every
        // step should show the constant 5 kW rate and monotonically
        // increasing cumulative energy -- proves the series is genuinely
        // per-step, not just a single repeated endpoint value.
        let (bat, state) = battery(10.0, 5.0, 0.5);
        let t1 = Utc::now();
        let t2_max = Duration::minutes(10); // -> 10 points at 60s resolution
        let series = asset_max_power_series(
            &bat,
            &state,
            t1,
            t2_max,
            CommitmentDirection::Import,
            LimitTier::Physical,
        );
        // max_effort_schedule's default body pushes t1, t1+60s, ..., t1+540s
        // (10 points at 60s resolution) plus one final trailing point at
        // exactly t_end (=t1+600s here) -- 11 total, not 10.
        assert_eq!(
            series.len(),
            11,
            "expected 10 steps plus a trailing t_end point"
        );
        assert!(
            series.iter().all(|&(_, power_kw, _)| power_kw == 5.0),
            "ample headroom must deliver the full rate at every step"
        );
        for window in series.windows(2) {
            assert!(
                window[1].2 > window[0].2,
                "cumulative energy must strictly increase step to step while power is nonzero"
            );
        }
    }

    #[test]
    fn shiftable_loads_series_lands_on_the_same_60s_grid_as_a_continuous_asset() {
        // unified-capacity-envelope-engine D2's "no resampling needed" claim:
        // ShiftableLoadAsset overrides max_effort_schedule directly (not
        // max_effort_setpoint) but still steps at the same 60s resolution as
        // the default body every continuous asset uses.
        use crate::assets::shiftable_load::ShiftableLoadAsset;

        let t1 = Utc::now();
        let load = ShiftableLoadAsset {
            power_kw: 2.0,
            duration_min: 5,
            earliest_start: t1,
            latest_end: t1 + Duration::minutes(30),
        };
        let load_state = AssetState::ShiftableLoad(ShiftableLoadAsset::initial_state());
        let (bat, bat_state) = battery(10.0, 5.0, 0.5);

        let t2_max = Duration::minutes(10);
        let load_series = asset_max_power_series(
            &load,
            &load_state,
            t1,
            t2_max,
            CommitmentDirection::Import,
            LimitTier::Physical,
        );
        let bat_series = asset_max_power_series(
            &bat,
            &bat_state,
            t1,
            t2_max,
            CommitmentDirection::Import,
            LimitTier::Physical,
        );

        let load_elapsed: Vec<i64> = load_series.iter().map(|&(e, _, _)| e).collect();
        let bat_elapsed: Vec<i64> = bat_series.iter().map(|&(e, _, _)| e).collect();
        assert_eq!(
            load_elapsed, bat_elapsed,
            "both asset kinds must land on the identical elapsed-time grid"
        );
    }
}
