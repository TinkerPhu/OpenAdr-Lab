use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use super::{
    Asset, AssetCapability, AssetFlexibilityFloor, AssetState, ControlDescriptor, MilpParticipant,
};
use crate::common::{Interpolation, TimeSeries};
use crate::controller::milp_planner::{
    AssetKind, AssetMilpContext, AssetMilpParams, ShiftableLoadMilpContext, ShiftableLoadScalars,
};
use crate::entities::asset::{ComfortRate, CompletionPolicy, PowerAdjustability};
use crate::entities::device_session::{EvSession, HeaterTarget};

/// Shiftable-load config: fixed power, non-interruptible once started, hard
/// `[earliest_start, latest_end]` window. See design.md D1/D2 of the
/// `shiftable-load-as-asset` change for the physical model and why this is a
/// dynamic (not boot-fixed) asset — no `ShiftableLoadParams`/`AssetParams`
/// variant exists; instances are constructed directly from the HEMS request
/// and pushed via `SimState::add_asset`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShiftableLoadAsset {
    pub power_kw: f64,
    pub duration_min: u32,
    pub earliest_start: DateTime<Utc>,
    pub latest_end: DateTime<Utc>,
}

/// Shiftable-load mutable state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShiftableLoadState {
    pub started: bool,
    /// Run time accumulated so far \[min\]. Only meaningful once `started`.
    pub elapsed_min: f64,
    pub actual_power_kw: f64,
}

impl ShiftableLoadAsset {
    pub fn initial_state() -> ShiftableLoadState {
        ShiftableLoadState {
            started: false,
            elapsed_min: 0.0,
            actual_power_kw: 0.0,
        }
    }

    pub fn is_finished(&self, state: &ShiftableLoadState) -> bool {
        state.started && state.elapsed_min >= self.duration_min as f64
    }

    /// Pure physics step. A nonzero commanded setpoint latches `started`;
    /// once started, `setpoint_kw` is ignored for the rest of the run —
    /// the load draws its fixed `power_kw` until `duration_min` elapses,
    /// then draws nothing. Never modulates: it is always at 0 or `power_kw`.
    pub fn step_inner(
        &self,
        state: &ShiftableLoadState,
        setpoint_kw: f64,
        dt: Duration,
    ) -> (ShiftableLoadState, f64) {
        if self.is_finished(state) {
            return (
                ShiftableLoadState {
                    started: state.started,
                    elapsed_min: state.elapsed_min,
                    actual_power_kw: 0.0,
                },
                0.0,
            );
        }
        let started = state.started || setpoint_kw > 1e-6;
        if !started {
            return (
                ShiftableLoadState {
                    started: false,
                    elapsed_min: 0.0,
                    actual_power_kw: 0.0,
                },
                0.0,
            );
        }
        let dt_min = dt.num_milliseconds() as f64 / 60_000.0;
        let elapsed = (state.elapsed_min + dt_min).min(self.duration_min as f64);
        (
            ShiftableLoadState {
                started: true,
                elapsed_min: elapsed,
                actual_power_kw: self.power_kw,
            },
            self.power_kw,
        )
    }

    /// While pending: can be commanded on (`power_kw`) or left off (`0`).
    /// While running: forced on, no off option (non-interruptible).
    /// Once finished: forced off.
    pub fn capability_inner(&self, state: &ShiftableLoadState) -> AssetCapability {
        let steps = if self.is_finished(state) {
            vec![0.0]
        } else if state.started {
            vec![self.power_kw]
        } else {
            vec![0.0, self.power_kw]
        };
        AssetCapability {
            max_export_kw: 0.0,
            max_import_kw: *steps.last().unwrap_or(&0.0),
            adjustability: PowerAdjustability::Stepped,
            power_steps_kw: steps,
        }
    }

    /// 0 while pending or finished; forced to `power_kw` while running —
    /// there is no lower setpoint available once started.
    pub fn flexibility_floor_inner(&self, state: &ShiftableLoadState) -> AssetFlexibilityFloor {
        let floor = if state.started && !self.is_finished(state) {
            self.power_kw
        } else {
            0.0
        };
        AssetFlexibilityFloor {
            min_export_kw: 0.0,
            min_import_kw: floor,
        }
    }

    pub fn default_setpoint(&self) -> f64 {
        0.0 // hold off by default; dispatcher/MILP decides when to start
    }

    pub fn state_values(&self, state: &ShiftableLoadState) -> HashMap<String, f64> {
        let mut m = HashMap::new();
        m.insert("power_kw".into(), self.power_kw);
        m.insert("duration_min".into(), self.duration_min as f64);
        m.insert("elapsed_min".into(), state.elapsed_min);
        m.insert("started".into(), if state.started { 1.0 } else { 0.0 });
        // Encoded as unix seconds so capacity_forecast.rs can read a shiftable
        // load's window straight off the live SimSnapshot instead of needing
        // a separate `&[ShiftableLoad]` parameter (shiftable-load-as-asset
        // proposal.md scope).
        m.insert(
            "earliest_start_unix".into(),
            self.earliest_start.timestamp() as f64,
        );
        m.insert("latest_end_unix".into(), self.latest_end.timestamp() as f64);
        m
    }

    pub fn control_schema(&self) -> Vec<ControlDescriptor> {
        vec![]
    }

    pub fn reset(&self, _state: &mut ShiftableLoadState, _values: HashMap<String, f64>) {
        // No sim-inject-editable fields: power/duration/window are fixed at
        // request time, and elapsed/started are physics-driven, not user-set.
    }

    pub fn update_config(&mut self, _values: HashMap<String, f64>) {
        // No runtime-editable config — see `reset`'s doc comment.
    }

    /// Holds the current physics forward: if pending, stays at 0 (this
    /// method has no plan-awareness, so it can't predict a future start —
    /// see `simulator::forecast` for the plan-driven equivalent). If
    /// running, projects the fixed-power tail until `duration_min` elapses.
    pub fn forecast(
        &self,
        state: &ShiftableLoadState,
        timespan: Duration,
        now: DateTime<Utc>,
    ) -> TimeSeries {
        if timespan <= Duration::zero() {
            return TimeSeries::empty(Interpolation::Linear);
        }
        let end = now + timespan;
        let mut samples = Vec::new();
        let mut t = now;
        let mut s = state.clone();
        while t < end {
            let (next, kw) = self.step_inner(&s, 0.0, Duration::seconds(60));
            samples.push((t, kw));
            s = next;
            t += Duration::seconds(60);
        }
        let (_, end_kw) = self.step_inner(&s, 0.0, Duration::zero());
        samples.push((end, end_kw));
        TimeSeries {
            samples,
            interpolation: Interpolation::Linear,
        }
    }

    pub fn default_comfort_rates(&self) -> Vec<ComfortRate> {
        vec![]
    }

    pub fn default_completion_policy(&self) -> CompletionPolicy {
        CompletionPolicy::Stop
    }

    pub fn default_post_deadline_comfort_bid(&self) -> Option<f64> {
        None
    }
}

impl Asset for ShiftableLoadAsset {
    fn step(&self, state: &AssetState, setpoint_kw: f64, dt: Duration) -> (AssetState, f64) {
        let AssetState::ShiftableLoad(s) = state else {
            unreachable!("ShiftableLoadAsset/state mismatch")
        };
        let (ns, p) = self.step_inner(s, setpoint_kw, dt);
        (AssetState::ShiftableLoad(ns), p)
    }

    fn capability(&self, state: &AssetState) -> AssetCapability {
        let AssetState::ShiftableLoad(s) = state else {
            unreachable!("ShiftableLoadAsset/state mismatch")
        };
        self.capability_inner(s)
    }

    fn flexibility_floor(&self, state: &AssetState) -> AssetFlexibilityFloor {
        let AssetState::ShiftableLoad(s) = state else {
            unreachable!("ShiftableLoadAsset/state mismatch")
        };
        self.flexibility_floor_inner(s)
    }

    fn default_setpoint(&self) -> f64 {
        Self::default_setpoint(self)
    }

    fn control_schema(&self) -> Vec<ControlDescriptor> {
        Self::control_schema(self)
    }

    fn update_config(&mut self, values: HashMap<String, f64>) {
        Self::update_config(self, values)
    }

    fn default_comfort_rates(&self) -> Vec<ComfortRate> {
        Self::default_comfort_rates(self)
    }

    fn default_completion_policy(&self) -> CompletionPolicy {
        Self::default_completion_policy(self)
    }

    fn default_post_deadline_comfort_bid(&self) -> Option<f64> {
        Self::default_post_deadline_comfort_bid(self)
    }

    fn state_values(&self, state: &AssetState) -> HashMap<String, f64> {
        let AssetState::ShiftableLoad(s) = state else {
            unreachable!("ShiftableLoadAsset/state mismatch")
        };
        Self::state_values(self, s)
    }

    fn reset(&self, state: &mut AssetState, values: HashMap<String, f64>) {
        let AssetState::ShiftableLoad(s) = state else {
            unreachable!("ShiftableLoadAsset/state mismatch")
        };
        Self::reset(self, s, values)
    }

    fn forecast(&self, state: &AssetState, timespan: Duration, now: DateTime<Utc>) -> TimeSeries {
        let AssetState::ShiftableLoad(s) = state else {
            unreachable!("ShiftableLoadAsset/state mismatch")
        };
        Self::forecast(self, s, timespan, now)
    }

    fn asset_type(&self) -> crate::entities::asset::AssetType {
        crate::entities::asset::AssetType::WashingMachine
    }

    fn asset_type_str(&self) -> &'static str {
        "shiftable_load"
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }

    fn clone_box(&self) -> Box<dyn Asset> {
        Box::new(self.clone())
    }

    fn is_removable(&self, state: &AssetState) -> bool {
        let AssetState::ShiftableLoad(s) = state else {
            unreachable!("ShiftableLoadAsset/state mismatch")
        };
        self.is_finished(s)
    }

    fn as_milp_participant(&self) -> Option<&dyn MilpParticipant> {
        Some(self)
    }
}

impl MilpParticipant for ShiftableLoadAsset {
    /// Only `asset_id`/`state`/`n`/`cum_s`/`now` apply here — every
    /// EV/heater-specific parameter (shared signature, `assets/asset_trait.rs`)
    /// is ignored, same as `Battery`'s impl already does.
    #[allow(clippy::too_many_arguments)] // trait-mandated signature shared by 4 heterogeneous asset kinds — see trait doc
    fn build_milp_context(
        &self,
        asset_id: &str,
        state: &AssetState,
        n: usize,
        cum_s: &[i64],
        now: DateTime<Utc>,
        _ev_session: Option<&EvSession>,
        _heater_target: Option<&HeaterTarget>,
        _ev_min_charge_kw: f64,
        _v_ev_extra_eur_kwh: f64,
        _v_ev_core_eur_kwh: f64,
        _asap_lateness_eur_kwh_h: f64,
        _v_ev_free_charge_eur_kwh: f64,
        _lambda_sw: f64,
        _c_terminal_eur_kwh: f64,
        _heater_anchor: Vec<Option<f64>>,
        _w_ghg_eur_kg: f64,
    ) -> Box<dyn AssetMilpContext> {
        let AssetState::ShiftableLoad(s) = state else {
            unreachable!("ShiftableLoadAsset/state mismatch")
        };

        // Maps a non-negative offset_s to the latest slot index t where
        // cum_s[t] <= offset_s. Mirrors `inputs.rs`'s pre-Spec-B helper of
        // the same name exactly, for solver parity.
        let time_to_slot = |offset_s: i64| -> usize {
            cum_s
                .partition_point(|&c| c <= offset_s)
                .saturating_sub(1)
                .min(n.saturating_sub(1))
        };

        let (duration_slots, valid_start_slots) = if s.started {
            // Already running: the start decision is made, not a MILP choice
            // — a single fixed "start" at slot 0, for whatever duration
            // remains.
            let remaining_min = (self.duration_min as f64 - s.elapsed_min).max(1.0);
            let remaining_s = (remaining_min * 60.0).round() as i64;
            let duration_slots = (1..=n).find(|&k| cum_s[k] >= remaining_s).unwrap_or(n);
            (duration_slots, vec![0])
        } else {
            let dur_s = (self.duration_min as i64) * 60;
            let duration_slots = (1..=n).find(|&k| cum_s[k] >= dur_s).unwrap_or(n);
            let window_start_s = (self.earliest_start - now).num_seconds().max(0);
            let window_end_s = (self.latest_end - now).num_seconds().max(0);
            let first_slot = time_to_slot(window_start_s);
            let last_valid_s = window_end_s - dur_s;
            let valid_start_slots: Vec<usize> = if last_valid_s < 0 || duration_slots == 0 {
                vec![]
            } else {
                let last_slot = time_to_slot(last_valid_s).min(n.saturating_sub(duration_slots));
                (first_slot..=last_slot)
                    .filter(|&slot| slot + duration_slots <= n)
                    .collect()
            };
            (duration_slots, valid_start_slots)
        };

        Box::new(ShiftableLoadMilpContext {
            asset_id: asset_id.to_string(),
            power_kw: self.power_kw,
            duration_slots,
            valid_start_slots,
        })
    }
}

impl AssetMilpContext for ShiftableLoadMilpContext {
    fn asset_id(&self) -> &str {
        &self.asset_id
    }

    fn asset_kind(&self) -> AssetKind {
        AssetKind::ShiftableLoad
    }

    fn milp_params(&self, _n: usize, _now: DateTime<Utc>) -> AssetMilpParams {
        AssetMilpParams::ShiftableLoad(ShiftableLoadScalars {
            power_kw: self.power_kw,
            duration_slots: self.duration_slots,
            valid_start_slots: self.valid_start_slots.clone(),
        })
    }

    /// Declares one binary per valid start slot and pushes the resulting
    /// `ShiftableLoadMilpVars` into `pool.shiftable` — a `Vec`, not a new
    /// `Option` slot, since (unlike Battery/EV/Heater) a site can have
    /// several shiftable loads. Replaces the pre-Spec-B bespoke pre-loop
    /// construction in `solver_phase1.rs`/`solver_phase2.rs`.
    fn declare_vars_into_pool(
        &self,
        _n: usize,
        _c_startup_eur: f64,
        _c_ramp_eur_kw: f64,
        vars: &mut good_lp::ProblemVariables,
        pool: &mut crate::controller::milp_interactions::MilpVarPool,
    ) {
        use good_lp::variable;
        let y_shift = self
            .valid_start_slots
            .iter()
            .map(|_| vars.add(variable().binary()))
            .collect();
        pool.shiftable.push(
            crate::controller::milp_interactions::ShiftableLoadMilpVars {
                asset_id: self.asset_id.clone(),
                power_kw: self.power_kw,
                duration_slots: self.duration_slots,
                valid_start_slots: self.valid_start_slots.clone(),
                y_shift,
            },
        );
    }

    /// Exactly one start slot must be chosen — the hard-window requirement
    /// (spec.md "Planner never schedules a start outside the valid window").
    /// Replaces the pre-Spec-B bespoke `sum_y == 1` loop in each solver
    /// phase's `add_model_constraints`.
    fn constraints(
        &self,
        pool: &crate::controller::milp_interactions::MilpVarPool,
        _n: usize,
        _dt_h: &[f64],
    ) -> Vec<good_lp::Constraint> {
        use good_lp::{constraint, Expression};
        let Some(sv) = pool
            .shiftable
            .iter()
            .find(|sv| sv.asset_id == self.asset_id)
        else {
            return vec![];
        };
        let mut sum_y = Expression::from(0.0);
        for &y in &sv.y_shift {
            sum_y += y;
        }
        vec![constraint!(sum_y == 1.0)]
    }

    /// No per-instance economic term of its own — the deterministic
    /// earliest-start tie-break (`SHIFT_TIEBREAK_EUR_PER_SLOT`) is already
    /// applied generically over `pool.shiftable` by each solver phase,
    /// unchanged by this migration (it was already per-instance, just called
    /// once over the whole `Vec` rather than once per `AssetMilpContext`).
    fn objective(
        &self,
        _pool: &crate::controller::milp_interactions::MilpVarPool,
        _n: usize,
        _dt_h: &[f64],
        _c_wear_eur_kwh: f64,
        _c_startup_eur: f64,
        _c_ramp_eur_kw: f64,
    ) -> good_lp::Expression {
        good_lp::Expression::from(0.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn load(power_kw: f64, duration_min: u32) -> ShiftableLoadAsset {
        let now = Utc::now();
        ShiftableLoadAsset {
            power_kw,
            duration_min,
            earliest_start: now,
            latest_end: now + Duration::hours(4),
        }
    }

    #[test]
    fn pending_load_draws_no_power_on_zero_setpoint() {
        let l = load(2.0, 60);
        let state = ShiftableLoadAsset::initial_state();
        let (_, kw) = l.step_inner(&state, 0.0, Duration::minutes(1));
        assert_eq!(kw, 0.0);
    }

    #[test]
    fn nonzero_setpoint_starts_the_load_at_rated_power() {
        let l = load(2.0, 60);
        let state = ShiftableLoadAsset::initial_state();
        let (next, kw) = l.step_inner(&state, 2.0, Duration::minutes(1));
        assert_eq!(kw, 2.0);
        assert!(next.started);
        assert!((next.elapsed_min - 1.0).abs() < 1e-9);
    }

    #[test]
    fn once_started_a_later_zero_setpoint_does_not_stop_it() {
        let l = load(2.0, 60);
        let (started, _) = l.step_inner(
            &ShiftableLoadAsset::initial_state(),
            2.0,
            Duration::minutes(1),
        );
        assert!(started.started);
        let (next, kw) = l.step_inner(&started, 0.0, Duration::minutes(1));
        assert_eq!(
            kw, 2.0,
            "non-interruptible: a later zero setpoint must not stop it"
        );
        assert!((next.elapsed_min - 2.0).abs() < 1e-9);
    }

    #[test]
    fn load_never_modulates_below_or_above_rated_power() {
        let l = load(2.0, 60);
        let (started, kw) = l.step_inner(
            &ShiftableLoadAsset::initial_state(),
            99.0,
            Duration::minutes(1),
        );
        assert_eq!(
            kw, 2.0,
            "must draw exactly rated power_kw, not the raw setpoint"
        );
        assert!(started.started);
    }

    #[test]
    fn load_finishes_exactly_at_duration_and_then_draws_nothing() {
        let l = load(2.0, 10);
        let (started, _) = l.step_inner(
            &ShiftableLoadAsset::initial_state(),
            2.0,
            Duration::minutes(10),
        );
        assert!((started.elapsed_min - 10.0).abs() < 1e-9);
        assert!(l.is_finished(&started));
        let (after, kw) = l.step_inner(&started, 2.0, Duration::minutes(1));
        assert_eq!(kw, 0.0, "finished load must stop drawing power");
        assert!(l.is_finished(&after));
    }

    #[test]
    fn capability_offers_on_or_off_while_pending() {
        let l = load(2.0, 60);
        let cap = l.capability_inner(&ShiftableLoadAsset::initial_state());
        assert_eq!(cap.power_steps_kw, vec![0.0, 2.0]);
        assert_eq!(cap.max_import_kw, 2.0);
    }

    #[test]
    fn capability_forces_on_while_running() {
        let l = load(2.0, 60);
        let (started, _) = l.step_inner(
            &ShiftableLoadAsset::initial_state(),
            2.0,
            Duration::minutes(1),
        );
        let cap = l.capability_inner(&started);
        assert_eq!(cap.power_steps_kw, vec![2.0], "no off option once running");
    }

    #[test]
    fn flexibility_floor_is_zero_while_pending_and_rated_power_while_running() {
        let l = load(2.0, 60);
        let pending = ShiftableLoadAsset::initial_state();
        assert_eq!(l.flexibility_floor_inner(&pending).min_import_kw, 0.0);
        let (started, _) = l.step_inner(&pending, 2.0, Duration::minutes(1));
        assert_eq!(l.flexibility_floor_inner(&started).min_import_kw, 2.0);
    }

    #[test]
    fn boxed_asset_dispatches_through_the_trait() {
        let l = load(2.0, 60);
        let boxed: Box<dyn Asset> = Box::new(l);
        let state = AssetState::ShiftableLoad(ShiftableLoadAsset::initial_state());
        let (next, kw) = boxed.step(&state, 2.0, Duration::minutes(1));
        assert_eq!(kw, 2.0);
        assert_eq!(boxed.state_values(&next).get("started"), Some(&1.0));
    }
}
