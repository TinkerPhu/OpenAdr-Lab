//! `CapacityCurve` — a closed-form, direction-specific power/duration/energy
//! forecast: "if the site committed now to sustained max import (or export),
//! how does the achievable power step down over elapsed time, and how much
//! energy is behind it." Distinct from `SiteFlexibilityForecastSlot`, whose
//! per-slot `up_kw`/`down_kw` are independent point-in-time counterfactuals
//! and must never be integrated over time — see
//! `controller::capacity_forecast` for the computation.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// The site's grid direction — import or export. Named for the sustained
/// commitment a capacity curve models (the two curves are not mirror images:
/// the contributing asset set and bounds differ per direction), and the one
/// import/export selector elsewhere too, e.g. which capacity limit
/// `entities::capacity::tightest_capacity_limit` reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CommitmentDirection {
    /// Sustained maximum import (site draws more from the grid).
    Import,
    /// Sustained maximum export (site sends more to the grid).
    Export,
}

/// Which ceiling an asset's "go all-in" extreme is measured against
/// (`asset-max-power-primitive`). **Honest scope note:** only `PvInverter`
/// has a real, distinct ceiling below `Physical` today (its
/// `generation_limit_kw`, sourced from a VTN/plan capacity limit for
/// `Contractual` or a manual sim-inject override for `UserSet`). Every other
/// asset kind (Battery/EvCharger/Heater/BaseLoad/ShiftableLoadAsset) is
/// tier-invariant — `Contractual` and `UserSet` currently return the same
/// answer as `Physical` for those kinds, because no per-asset contractual or
/// user-set ceiling concept exists for them yet. This is not a stub to be
/// silently assumed complete; it reflects what the codebase actually has.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LimitTier {
    /// The asset's own physical ceiling — exactly what `Asset::capability()`
    /// reports, for every asset kind.
    Physical,
    /// A ceiling imposed externally (today: PV's VTN/plan-sourced
    /// `generation_limit_kw`). Falls back to `Physical` for every other kind.
    Contractual,
    /// A ceiling the user set manually (today: PV's manual sim-inject
    /// `generation_limit_kw` override). Falls back to `Physical` for every
    /// other kind.
    UserSet,
}

/// One point where the achievable power changes (an asset saturates, a
/// shiftable load is placed). `power_kw` holds from this step's `elapsed_s`
/// until the next step (or the horizon end) — a step function, not an
/// interpolated line.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CapacityCurveStep {
    /// Seconds elapsed since `CapacityCurve::start`.
    pub elapsed_s: i64,
    /// Achievable net grid power at and after this elapsed time (kW).
    /// SIGNED: positive = import, negative = export — the same convention
    /// `Asset::max_effort_setpoint`/`capability()` use everywhere else
    /// (`unified-capacity-envelope-engine`, Spec E). The unsigned-magnitude
    /// convention this field originally used is now applied only at the
    /// actual external boundary that needs it
    /// (`controller::report_intervals::build_capacity_forecast_intervals`,
    /// for OpenADR's direction-tagged-by-name report payloads) — see
    /// `controller::capacity_headroom`'s module doc for the full reasoning.
    pub power_kw: f64,
}

/// A closed-form power/duration/energy capacity forecast for one commitment
/// direction, anchored at `start`. Not cached or treated as binding across
/// ticks — recompute fresh whenever a fresh answer is needed.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CapacityCurve {
    pub direction: CommitmentDirection,
    pub start: DateTime<Utc>,
    /// The **instantaneous** power of the all-in trajectory at each
    /// breakpoint (`assets::max_power`'s contract). Ordered by `elapsed_s`,
    /// ascending, starting at 0. This is what flows, so it is what
    /// `energy_kwh_total` integrates and what the OpenADR per-interval
    /// capacity report states — but it is NOT what the site can promise to
    /// hold: a cycling heater spikes here every time its relay closes.
    pub steps: Vec<CapacityCurveStep>,
    /// The **sustained** commitment at each of the same breakpoints: the
    /// constant power holdable from `start` through that instant
    /// (`sustainable_power_kw`). This is the number a commitment means, and
    /// the one the Site Headroom chart plots. Derived from `steps` by
    /// `CapacityCurve::new` — never set it independently.
    #[serde(default)]
    pub sustained: Vec<CapacityCurveStep>,
}

/// Both directions' curves for one commitment start — the
/// `GET /flexibility/capacity` response body. `start` is the instant both
/// curves are anchored at: the per-tick `now`, or, for a `?start=` request,
/// the plan slot boundary the requested start snapped down to.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CapacityCurves {
    pub start: DateTime<Utc>,
    pub import: CapacityCurve,
    pub export: CapacityCurve,
}

impl CapacityCurve {
    /// The only way to build a curve: derives `sustained` from `steps` so the
    /// two can never disagree (one-concept-one-function — the sustained view
    /// is a function of the instantaneous one, not a second source of truth).
    pub fn new(
        direction: CommitmentDirection,
        start: DateTime<Utc>,
        steps: Vec<CapacityCurveStep>,
    ) -> Self {
        let mut curve = Self {
            direction,
            start,
            steps,
            sustained: Vec::new(),
        };
        curve.sustained = curve.sustained_steps();
        curve
    }

    /// Cumulative energy (kWh) across the whole curve — trapezoidal-free
    /// since each step holds constant power until the next step (or the
    /// curve's last step, which contributes no further energy: there is no
    /// "next" elapsed time to bound its duration).
    pub fn energy_kwh_total(&self) -> f64 {
        self.steps
            .windows(2)
            .map(|w| {
                let dt_h = (w[1].elapsed_s - w[0].elapsed_s) as f64 / 3600.0;
                w[0].power_kw.abs() * dt_h
            })
            .sum()
    }

    /// Energy (kWh, signed like `power_kw`) that flows in the first
    /// `elapsed_s` seconds of the commitment. Integrates the same step
    /// function `energy_kwh_total` does, but stops mid-step when the window
    /// ends inside one, and keeps the sign so the caller can tell an import
    /// window from an export one.
    pub fn energy_kwh_within(&self, elapsed_s: i64) -> f64 {
        if elapsed_s <= 0 {
            return 0.0;
        }
        self.steps
            .windows(2)
            .map(|w| {
                let from = w[0].elapsed_s.max(0);
                let to = w[1].elapsed_s.min(elapsed_s);
                if to <= from {
                    return 0.0;
                }
                w[0].power_kw * (to - from) as f64 / 3600.0
            })
            .sum()
    }

    /// The **constant** power the site could actually hold for `elapsed_s` —
    /// the honest answer to "what can I commit to for this long", as opposed
    /// to `steps`, which is the instantaneous power of the all-in trajectory
    /// at each instant (`assets::max_power::asset_max_power`'s contract: "what
    /// power is it still delivering at the end").
    ///
    /// The two differ whenever an asset's availability returns after lapsing —
    /// a thermostat-cycling heater is the everyday case: it is genuinely
    /// drawing its full power during each burst, so the instantaneous curve
    /// rightly spikes, but a commitment can only be held at the duty-cycled
    /// average. Taking the running minimum instead would be the opposite error
    /// (it would report the between-bursts floor, denying capability the site
    /// really does have), so this is energy-based: what flowed, spread evenly
    /// over the window.
    ///
    /// A non-positive window is degenerate — there is no duration to average
    /// over — so it returns the instantaneous power in effect at `start`, the
    /// limit as the window shrinks to nothing. That also keeps the seam the
    /// `movable-capacity-curve-start` work established: a curve anchored at an
    /// instant still touches the headroom band at that instant, whether the
    /// caller reads `steps` or `sustained`.
    pub fn sustainable_power_kw(&self, elapsed_s: i64) -> f64 {
        if elapsed_s <= 0 {
            return self.steps.first().map(|s| s.power_kw).unwrap_or(0.0);
        }
        self.energy_kwh_within(elapsed_s) / (elapsed_s as f64 / 3600.0)
    }

    /// The sustained-commitment curve: for each of this curve's own
    /// breakpoints, the constant power holdable from `start` through it.
    /// This is what a chart labelled "commitment" must plot: each point is a
    /// promise the whole preceding window can keep. Note it is *not* monotone
    /// in general — a contribution that only starts later (a battery idle at
    /// first, an EV that comes home) genuinely raises the average a longer
    /// window can hold. Monotonicity is a property of energy-limited assets,
    /// not of this curve.
    pub fn sustained_steps(&self) -> Vec<CapacityCurveStep> {
        self.steps
            .iter()
            .map(|s| CapacityCurveStep {
                elapsed_s: s.elapsed_s,
                power_kw: self.sustainable_power_kw(s.elapsed_s),
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn make_curve(steps: Vec<(i64, f64)>) -> CapacityCurve {
        CapacityCurve::new(
            CommitmentDirection::Export,
            Utc.with_ymd_and_hms(2026, 8, 20, 12, 0, 0).unwrap(),
            steps
                .into_iter()
                .map(|(elapsed_s, power_kw)| CapacityCurveStep {
                    elapsed_s,
                    power_kw,
                })
                .collect(),
        )
    }

    // ── sustained commitment (energy ÷ duration) ─────────────────────────

    /// The everyday case this exists for: a thermostat-cycling heater blipping
    /// on for 1 min in every 8. The instantaneous curve rightly spikes to the
    /// full 6.6 kW during each burst; a commitment can only be held at the
    /// duty-cycled average.
    fn cycling_heater_curve() -> CapacityCurve {
        // 0.6 kW floor, +6.0 kW for the first minute of each 8-minute period.
        let mut steps = vec![];
        for period in 0..5 {
            let t0 = period * 480;
            steps.push(CapacityCurveStep {
                elapsed_s: t0,
                power_kw: 6.6,
            });
            steps.push(CapacityCurveStep {
                elapsed_s: t0 + 60,
                power_kw: 0.6,
            });
        }
        steps.push(CapacityCurveStep {
            elapsed_s: 2400,
            power_kw: 0.6,
        });
        CapacityCurve::new(
            CommitmentDirection::Import,
            Utc.with_ymd_and_hms(2026, 9, 26, 12, 0, 0).unwrap(),
            steps,
        )
    }

    #[test]
    fn sustainable_power_kw_is_the_duty_cycled_average_not_the_spike() {
        let curve = cycling_heater_curve();
        // Over 8 minutes: 6.6 kW for 60 s + 0.6 kW for 420 s
        //   = (6.6*60 + 0.6*420)/480 = 1.35 kW.
        let p = curve.sustainable_power_kw(480);
        assert!(
            (p - 1.35).abs() < 1e-9,
            "expected the duty-cycled 1.35 kW, got {p}"
        );
        assert!(
            p < 6.6 && p > 0.6,
            "a commitment is neither the spike nor the floor"
        );
    }

    #[test]
    fn sustainable_power_kw_never_exceeds_the_instantaneous_spike() {
        let curve = cycling_heater_curve();
        let peak = curve
            .steps
            .iter()
            .map(|s| s.power_kw)
            .fold(f64::MIN, f64::max);
        for d in [60, 300, 480, 1200, 2400] {
            assert!(
                curve.sustainable_power_kw(d) <= peak + 1e-9,
                "sustainable power at {d}s exceeded the instantaneous peak"
            );
        }
    }

    #[test]
    fn sustainable_power_kw_equals_the_level_of_a_flat_curve() {
        // Nothing cycles: committing for any duration gives the same power.
        let curve = make_curve(vec![(0, -4.0), (3600, -4.0)]);
        for d in [60, 900, 3600] {
            assert!(
                (curve.sustainable_power_kw(d) + 4.0).abs() < 1e-9,
                "a flat curve must sustain its own level at {d}s"
            );
        }
    }

    #[test]
    fn sustainable_power_kw_falls_as_an_energy_limited_asset_runs_out() {
        // A battery: 5 kW until empty at 30 min, then nothing. The longer the
        // commitment, the lower the constant power it can hold.
        let curve = make_curve(vec![(0, 5.0), (1800, 0.0), (7200, 0.0)]);
        let half = curve.sustainable_power_kw(1800);
        let hour = curve.sustainable_power_kw(3600);
        let two_h = curve.sustainable_power_kw(7200);
        assert!((half - 5.0).abs() < 1e-9, "full power while it lasts");
        assert!((hour - 2.5).abs() < 1e-9, "2.5 kWh spread over 1 h");
        assert!((two_h - 1.25).abs() < 1e-9, "same energy over 2 h");
        assert!(two_h < hour && hour < half, "must decay with duration");
    }

    #[test]
    fn sustainable_power_kw_falls_back_to_the_instant_value_for_a_zero_window() {
        // Degenerate window: no duration to average over, so the answer is the
        // instantaneous power at `start` -- which is what keeps the sustained
        // curve touching the headroom band at t = 0.
        let curve = cycling_heater_curve();
        assert_eq!(curve.sustainable_power_kw(0), 6.6);
        assert_eq!(curve.sustainable_power_kw(-60), 6.6);
        let empty = CapacityCurve::new(
            CommitmentDirection::Import,
            Utc.with_ymd_and_hms(2026, 9, 26, 12, 0, 0).unwrap(),
            vec![],
        );
        assert_eq!(empty.sustainable_power_kw(0), 0.0);
    }

    #[test]
    fn energy_kwh_within_stops_inside_a_step() {
        // 5 kW for the first 1800 s; asking for 900 s must give half of it.
        let curve = make_curve(vec![(0, 5.0), (1800, 0.0), (3600, 0.0)]);
        assert!((curve.energy_kwh_within(900) - 1.25).abs() < 1e-9);
        assert!((curve.energy_kwh_within(1800) - 2.5).abs() < 1e-9);
        // Past the end, the last step contributes nothing (no bound) --
        // same convention energy_kwh_total uses.
        assert!((curve.energy_kwh_within(99_999) - 2.5).abs() < 1e-9);
    }

    #[test]
    fn sustained_steps_keeps_the_breakpoints_and_carries_sustained_power() {
        let curve = cycling_heater_curve();
        let sustained = curve.sustained_steps();
        assert_eq!(sustained.len(), curve.steps.len());
        for (a, b) in curve.steps.iter().zip(&sustained) {
            assert_eq!(a.elapsed_s, b.elapsed_s, "breakpoints must be preserved");
        }
        // The first breakpoint is degenerate (zero-length): it carries the
        // instantaneous value, so the curve meets the headroom band at t = 0.
        assert_eq!(sustained[0].power_kw, curve.steps[0].power_kw);
        // Later ones converge toward the duty-cycled average, not the spike.
        let last = sustained.last().unwrap().power_kw;
        assert!(
            last > 0.6 && last < 2.0,
            "expected the duty-cycled average, got {last}"
        );
    }

    #[test]
    fn energy_kwh_total_integrates_step_function() {
        // 5 kW held for the first 1800s (0.5h), then 2 kW held until 3600s (0.5h more).
        let curve = make_curve(vec![(0, 5.0), (1800, 2.0), (3600, 0.0)]);
        let energy = curve.energy_kwh_total();
        assert!(
            (energy - 3.5).abs() < 1e-9,
            "expected 5.0*0.5 + 2.0*0.5 = 3.5 kWh, got {energy}"
        );
    }

    #[test]
    fn energy_kwh_total_zero_for_single_step() {
        // No "next" elapsed time to bound the last step's duration.
        let curve = make_curve(vec![(0, 5.0)]);
        assert_eq!(curve.energy_kwh_total(), 0.0);
    }

    #[test]
    fn energy_kwh_total_zero_for_empty_curve() {
        let curve = make_curve(vec![]);
        assert_eq!(curve.energy_kwh_total(), 0.0);
    }

    #[test]
    fn energy_kwh_total_uses_magnitude_for_import_direction() {
        // Import-direction power is expressed as positive kW by convention,
        // but the formula must not silently flip sign for either direction.
        let curve = CapacityCurve::new(
            CommitmentDirection::Import,
            Utc.with_ymd_and_hms(2026, 8, 20, 12, 0, 0).unwrap(),
            vec![
                CapacityCurveStep {
                    elapsed_s: 0,
                    power_kw: 4.0,
                },
                CapacityCurveStep {
                    elapsed_s: 900,
                    power_kw: 0.0,
                },
            ],
        );
        assert!((curve.energy_kwh_total() - 1.0).abs() < 1e-9);
    }
}
