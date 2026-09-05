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

/// "If this asset committed now to a sustained extreme in `direction` under
/// `tier`, held from `t1` for `t2`, what power is it still delivering at the
/// end and how much energy flowed?" (`asset-max-power-primitive` D4). Pure
/// composition over `max_effort_schedule`/`simulate_forward` — no new
/// simulation logic of its own. Returns `(power_kw_at_t1_plus_t2, energy_kwh)`.
///
/// Not yet called from production code -- this change (`asset-max-power-primitive`)
/// only builds and unit-tests the primitive; wiring it into `capacity_forecast.rs`
/// is Spec E's job (see proposal.md's Impact section).
#[allow(dead_code)]
pub fn asset_max_power(
    asset: &dyn Asset,
    state: &AssetState,
    t1: DateTime<Utc>,
    t2: Duration,
    direction: CommitmentDirection,
    tier: LimitTier,
) -> (f64, f64) {
    let t_end = t1 + t2;
    let schedule = asset.max_effort_schedule(state, direction, tier, t1, t_end);
    let trajectory = asset.simulate_forward(state, &schedule);
    let power_kw = trajectory.points.last().map(|p| p.power_kw).unwrap_or(0.0);
    // Mirrors `CapacityCurve::energy_kwh_total`'s own windows(2) integration:
    // each point holds constant power until the next point; the trajectory's
    // trailing zero-duration point (see `simulate_forward`'s doc comment)
    // contributes no further energy, so it's excluded by `windows(2)` itself.
    let energy_kwh = trajectory
        .points
        .windows(2)
        .map(|w| {
            let dt_h = (w[1].ts - w[0].ts).num_milliseconds() as f64 / 3_600_000.0;
            w[0].power_kw.abs() * dt_h
        })
        .sum();
    (power_kw, energy_kwh)
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
}
