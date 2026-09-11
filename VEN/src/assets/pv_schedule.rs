//! `PvInverter`'s weather/decay-aware forward projection — split out of
//! `pv.rs` to stay under the file-size cap. `pv-competence-consolidation`:
//! this is the mechanism that makes `Asset::max_effort_schedule`/`forecast()`
//! actually authoritative for points beyond "now", replacing the site-level
//! `pv_frames`/`pv_ceiling_kw` special-casing this phase retires.

use chrono::{DateTime, Duration, Utc};

use super::pv::{PvInverter, PvPowerInputs, PvState};
use super::{Asset, AssetState, Trajectory, TrajectoryPoint};
use crate::entities::capacity_curve::{CommitmentDirection, LimitTier};

impl PvInverter {
    /// The precedence + clipping rules `step_inner` applies, as a pure function
    /// of explicitly-passed inputs rather than of `self`'s live fields.
    ///
    /// `SimState::peek_pv_kw` previews a tick *before* `tick()` writes this
    /// tick's weather/measurement/override values onto the config, so it holds
    /// those values as parameters and cannot go through `step_inner`. Both call
    /// this instead. Keeping one implementation is what stops the two from
    /// drifting — they already had: the preview was missing `inverter_max_kw`
    /// clipping entirely, overstating export by (rated_kw − inverter_max_kw)
    /// whenever DC potential exceeded the inverter's AC ceiling.
    pub fn resolve_power_kw(&self, inputs: &PvPowerInputs) -> f64 {
        let raw_kw = self.uncurtailed_power_kw(inputs);
        self.generation_limit_kw
            .map(|lim| raw_kw.max(lim)) // lim ≤ 0; max() clamps to less export
            .unwrap_or(raw_kw)
    }

    /// `resolve_power_kw` before any `generation_limit_kw` clamp — the true
    /// panel/inverter physical ceiling for the given inputs
    /// (`asset-max-power-primitive` D2: `LimitTier::Physical` for PV means
    /// this, not whatever `capability()`/`actual_power_kw` currently reports,
    /// since that's already post-curtailment whenever a limit is active).
    pub fn uncurtailed_power_kw(&self, inputs: &PvPowerInputs) -> f64 {
        let base_kw = inputs.measured_power_kw.or(inputs.weather_power_kw);
        let dc_potential_kw = if inputs.irradiance_forced {
            self.rated_kw * inputs.irradiance
        } else {
            match base_kw {
                Some(kw) => (kw.max(0.0) + inputs.irradiance_offset * self.rated_kw).max(0.0),
                None => self.rated_kw * inputs.irradiance,
            }
        };
        // Inverter's own AC-side ceiling clips DC potential before any commanded limit —
        // see openspec/changes/pv-curtailment-history/.
        -dc_potential_kw.min(self.inverter_max_kw) // negative = export
    }

    /// The uncurtailed (Physical-tier) export ceiling at a future point —
    /// the one function `max_effort_schedule` and `forecast()` both build on
    /// for `elapsed_s > 0`, so PV's own trait methods never disagree with
    /// each other. `elapsed_s` (seconds since `weather_forecast` was
    /// captured this tick) is what `PvSmoothingState::decayed_offset_after`
    /// needs to correctly project the live offset forward — NOT reusable via
    /// `uncurtailed_power_kw`'s own `t=0`-only inputs, hence a distinct
    /// method rather than a parameter added there.
    ///
    /// Weather, when available (`self.weather_forecast` present, same
    /// freshness/config gating `weather_power_kw` itself already went
    /// through this tick), is authoritative for `ts` directly — it already
    /// reflects whatever will actually happen at that future moment, so no
    /// separate offset-decay term applies on top of it (matching
    /// `uncurtailed_power_kw`'s own `measured_power_kw.or(weather_power_kw)`
    /// precedence, where a decaying offset only applies to the *base*
    /// itself, not stacked on top of a value already believed authoritative).
    /// Absent weather, falls back to the sin model plus the offset projected
    /// forward by `elapsed_s` — the same graceful degradation
    /// `pv_ceiling_kw`/`forecast()` already had.
    pub(crate) fn uncurtailed_power_kw_at(&self, ts: DateTime<Utc>, elapsed_s: f64) -> f64 {
        let dc_potential_kw = match &self.weather_forecast {
            Some(series) if !series.is_empty() => {
                crate::entities::solar::weather_pv_kw_for_slots(series, &[ts])
                    .first()
                    .copied()
                    .unwrap_or(0.0)
                    .max(0.0)
            }
            _ => {
                let natural = Self::natural_irradiance_at(ts);
                let decayed = crate::simulator::pv_smoothing::decayed_offset(
                    self.irradiance_offset,
                    elapsed_s,
                    self.tau_s,
                );
                // Clamp the irradiance FRACTION to [0,1] before scaling by rated_kw —
                // matches pv_ceiling_kw's and step_inner's PvPowerInputs.irradiance
                // contract. Scaling natural/decayed separately (as this used to) lets an
                // inject offset push the fraction above 1.0 uncapped whenever
                // inverter_max_kw > rated_kw, diverging from both of those (found via
                // pv-competence-consolidation's section 4 numeric-equivalence test).
                (natural + decayed).clamp(0.0, 1.0) * self.rated_kw
            }
        };
        -dc_potential_kw.min(self.inverter_max_kw)
    }

    /// `pv-competence-consolidation`: PV's own weather/decay-aware projection,
    /// replacing the trait default (which would hold `max_effort_setpoint`'s
    /// live answer flat across the whole schedule — exactly the "PV's
    /// ceiling flattened to a constant" regression `capacity_headroom.rs`'s
    /// deleted PV special-casing existed to work around). Only
    /// `LimitTier::Physical` genuinely varies over time (weather changes,
    /// the manual-override offset decays); `Contractual`/`UserSet` describe
    /// a *currently* active curtailment limit, projecting that forward is a
    /// separate question this phase's own non-goals leave alone — those
    /// tiers keep the trait default's flat-hold shape, inlined here since
    /// overriding `Asset::max_effort_schedule` (the thin delegate in
    /// `pv.rs`) forfeits the default. `pub(super)`: called only from that
    /// one trait-method delegate.
    pub(super) fn max_effort_schedule_inner(
        &self,
        state: &AssetState,
        direction: CommitmentDirection,
        tier: LimitTier,
        t1: DateTime<Utc>,
        t_end: DateTime<Utc>,
    ) -> Vec<(DateTime<Utc>, f64)> {
        if direction != CommitmentDirection::Export || tier != LimitTier::Physical {
            let setpoint = self.max_effort_setpoint(state, direction, tier);
            if t_end <= t1 {
                return vec![(t1, setpoint), (t1, setpoint)];
            }
            let mut schedule = Vec::new();
            let mut t = t1;
            while t < t_end {
                schedule.push((t, setpoint));
                t += Duration::seconds(60);
            }
            schedule.push((t_end, setpoint));
            return schedule;
        }
        // t1 itself: identical to max_effort_setpoint's answer by construction
        // (elapsed_s=0 -> decayed_offset_after returns the offset unchanged;
        // weather_forecast, when present, is sampled at t1 the same way
        // weather_power_kw already is for "now") — the seam this phase exists
        // to guarantee matches, not just happens to match.
        if t_end <= t1 {
            let v = self.uncurtailed_power_kw_at(t1, 0.0);
            return vec![(t1, v), (t1, v)];
        }
        let mut schedule = Vec::new();
        let mut t = t1;
        while t < t_end {
            let elapsed_s = (t - t1).num_seconds() as f64;
            schedule.push((t, self.uncurtailed_power_kw_at(t, elapsed_s)));
            t += Duration::seconds(60);
        }
        let elapsed_s = (t_end - t1).num_seconds() as f64;
        schedule.push((t_end, self.uncurtailed_power_kw_at(t_end, elapsed_s)));
        schedule
    }

    /// `pv-competence-consolidation` section 5b: one `TrajectoryPoint` per
    /// `setpoints` entry, each built directly from that entry's own
    /// timestamp via `uncurtailed_power_kw_at` — bypassing `Asset::step`'s
    /// no-timestamp default entirely (see `simulate_forward`'s doc comment
    /// in `pv.rs` for why that default silently flattens PV's ceiling).
    /// Deliberately UNCURTAILED (no `generation_limit_kw` clamp), matching
    /// `max_effort_schedule_inner`'s own Physical/Export convention — both of
    /// this method's callers (`asset_max_power_series`, `simulated_trajectory`)
    /// ask a Physical-tier "what's the true ceiling" question, not "what
    /// would actually happen under today's active limit" (that's what
    /// `PvInverter::max_effort_setpoint`'s Contractual/UserSet tiers are for,
    /// and they deliberately do NOT project forward — a separate, already-
    /// documented non-goal). `setpoints`' own `f64` field is ignored for the
    /// same reason: PV has no commandable setpoint the way battery/EV/heater
    /// do, so there is nothing meaningful to read from it.
    /// `elapsed_s` is measured from `setpoints[0].0`, matching every caller's
    /// own convention (`asset_max_power_series`/`simulated_trajectory` both
    /// build `setpoints` starting at their own commitment/forecast origin).
    pub(super) fn simulate_forward_inner(
        &self,
        initial: &AssetState,
        setpoints: &[(DateTime<Utc>, f64)],
    ) -> Trajectory {
        let AssetState::Pv(_) = initial else {
            unreachable!("PvInverter/state mismatch")
        };
        let Some(&(t0, _)) = setpoints.first() else {
            return Trajectory { points: vec![] };
        };
        let points = setpoints
            .iter()
            .map(|&(ts, _)| {
                let elapsed_s = (ts - t0).num_seconds() as f64;
                let power_kw = self.uncurtailed_power_kw_at(ts, elapsed_s);
                TrajectoryPoint {
                    ts,
                    power_kw,
                    state: AssetState::Pv(PvState {
                        actual_power_kw: power_kw,
                        generation_limit_kw: self.generation_limit_kw,
                        curtailment_source: self.curtailment_source,
                    }),
                }
            })
            .collect();
        Trajectory { points }
    }
}
