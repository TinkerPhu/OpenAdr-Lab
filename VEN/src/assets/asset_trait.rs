use std::collections::HashMap;

use chrono::{DateTime, Duration, Utc};

use super::{
    AssetCapability, AssetConfig, AssetFlexibilityFloor, AssetHistoryBuffer, AssetState,
    ControlDescriptor,
};
use crate::assets::HistoryPoint;
use crate::common::TimeSeries;
use crate::entities::asset::{AssetType, ComfortRate, CompletionPolicy};
use crate::entities::asset_params::PvCurtailmentSource;
use crate::entities::device_session::{EvSession, HeaterTarget};
use crate::entities::timeline::HeaterPlanTrajectory;

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

/// Full Asset trait. Combines the physics interface (Phase A) with the identity and
/// history interface (Phase B/C) needed for `&dyn Asset` trait objects.
///
/// Physics types (`Battery`, `EvCharger`, etc.) implement only `step()` and `capability()`.
/// They inherit the three identity/history methods with panicking defaults — those methods
/// must only be called via `AssetHandle`, which properly implements them.
#[allow(dead_code)]
pub trait Asset: Send + Sync {
    // ── Identity / observability (Phase B/C) ──────────────────────────────────

    /// Unique asset identifier (e.g. "battery", "ev", "grid").
    /// Default panics — call via `AssetHandle`, not a bare physics type.
    fn id(&self) -> &str {
        unimplemented!("Asset::id() must be called via AssetHandle, not a bare physics type")
    }

    /// Current live state snapshot. Positive = import from grid, negative = export.
    /// Default panics — call via `AssetHandle`, not a bare physics type.
    fn current_state(&self) -> AssetState {
        unimplemented!(
            "Asset::current_state() must be called via AssetHandle, not a bare physics type"
        )
    }

    /// Slice of this asset's own ring buffer over [now − window, now].
    /// Default panics — call via `AssetHandle`, not a bare physics type.
    fn history(&self, _window: Duration, _now: DateTime<Utc>) -> Vec<HistoryPoint> {
        unimplemented!("Asset::history() must be called via AssetHandle, not a bare physics type")
    }

    // ── Physics primitives (Phase A) ──────────────────────────────────────────

    /// Pure physics step. Returns (new_state, actual_power_kw).
    /// actual_power_kw may differ from setpoint_kw (e.g. SoC ceiling clamps charge rate).
    /// Sign convention: positive = import/charge, negative = export/discharge.
    fn step(&self, state: &AssetState, setpoint_kw: f64, dt: Duration) -> (AssetState, f64);

    /// Point-in-time feasible power range given current state.
    fn capability(&self, state: &AssetState) -> AssetCapability;

    /// Lowest magnitude the asset could still be forced to right now — see
    /// `AssetFlexibilityFloor`'s doc comment. No default: every asset type must
    /// state its own answer explicitly rather than silently inherit a wrong one.
    fn flexibility_floor(&self, state: &AssetState) -> AssetFlexibilityFloor;

    // ── AssetConfig-scoped methods (Spec A D4) ──────────────────────────────
    //
    // "Universal" per D4's audit means universal across `AssetConfig`'s 5
    // variants specifically (all dispatched via the delegate_asset! macros
    // today, no `_ =>` fallback) — NOT universal across every `Asset`
    // implementor. `Grid` also implements `Asset` but is deliberately outside
    // `AssetConfig` (see `grid.rs`'s own doc comment: "not part of the
    // controllable asset dispatch loop") and has no sim-inject/MILP/forecast
    // concept of its own. So these get panicking defaults, same contract as
    // `id`/`current_state`/`history` above: real physics types override them
    // for real (wired in Spec A's Phase 2a, not this phase), Grid inherits the
    // default and is never called this way in practice.

    /// The setpoint a dispatcher should hold this asset at absent any explicit
    /// command (e.g. battery/EV/heater hold at 0.0 — dispatcher-controlled).
    fn default_setpoint(&self) -> f64 {
        unimplemented!("Asset::default_setpoint() only applies to AssetConfig-backed asset kinds")
    }

    /// This asset's runtime-controllable parameters, for `GET /sim/schema`.
    fn control_schema(&self) -> Vec<ControlDescriptor> {
        unimplemented!("Asset::control_schema() only applies to AssetConfig-backed asset kinds")
    }

    /// Apply user-edited config values (e.g. from the sim-inject UI).
    fn update_config(&mut self, _values: HashMap<String, f64>) {
        unimplemented!("Asset::update_config() only applies to AssetConfig-backed asset kinds")
    }

    /// MILP comfort-curve default (fill vs. max marginal price/CO2), used when
    /// no user-set comfort curve exists for this asset's session.
    fn default_comfort_rates(&self) -> Vec<ComfortRate> {
        unimplemented!(
            "Asset::default_comfort_rates() only applies to AssetConfig-backed asset kinds"
        )
    }

    /// What happens once this asset's MILP objective is satisfied (stop vs.
    /// keep optimizing opportunistically).
    fn default_completion_policy(&self) -> CompletionPolicy {
        unimplemented!(
            "Asset::default_completion_policy() only applies to AssetConfig-backed asset kinds"
        )
    }

    /// Comfort bid applied past a missed deadline, if any.
    fn default_post_deadline_comfort_bid(&self) -> Option<f64> {
        unimplemented!(
            "Asset::default_post_deadline_comfort_bid() only applies to AssetConfig-backed asset kinds"
        )
    }

    /// This state's config-relevant values, keyed for the generic sim-inject/
    /// diagnostics UI (e.g. `{"soc": 0.6, "capacity_kwh": 10.0}`).
    fn state_values(&self, _state: &AssetState) -> HashMap<String, f64> {
        unimplemented!("Asset::state_values() only applies to AssetConfig-backed asset kinds")
    }

    /// Overwrite mutable state fields from user-supplied key/value pairs (the
    /// inverse of `state_values`, for the sim-inject UI).
    fn reset(&self, _state: &mut AssetState, _values: HashMap<String, f64>) {
        unimplemented!("Asset::reset() only applies to AssetConfig-backed asset kinds")
    }

    /// Forecast this asset's own power over `timespan` from `now`, holding its
    /// current setpoint (no plan-awareness — see `simulator::forecast` for
    /// the plan-driven equivalent).
    fn forecast(
        &self,
        _state: &AssetState,
        _timespan: Duration,
        _now: DateTime<Utc>,
    ) -> TimeSeries {
        unimplemented!("Asset::forecast() only applies to AssetConfig-backed asset kinds")
    }

    /// Domain-level type classification, for the API/timeline snapshots.
    /// **Not** 1:1 with the 5 `AssetConfig` variants — `BaseLoad` maps to
    /// `AssetType::GenericConsumer`, not its own variant (see `AssetType`'s
    /// own doc comment for the wider consumer/producer catalog this fits
    /// into, spanning asset kinds this codebase hasn't implemented yet).
    fn asset_type(&self) -> AssetType {
        unimplemented!("Asset::asset_type() only applies to AssetConfig-backed asset kinds")
    }

    /// Lowercase snake_case label for the same purpose as `asset_type`, but
    /// for contexts that want the literal `AssetConfig` variant name (e.g.
    /// `"base_load"`, not `"generic_consumer"`) rather than the wider
    /// domain classification. Deliberately a separate method, not derived
    /// from `asset_type()` — the two diverge for `BaseLoad`, and inverting
    /// `AssetType::GenericConsumer` back to a specific string wouldn't be
    /// safe once other consumer kinds share that same variant.
    fn asset_type_str(&self) -> &'static str {
        unimplemented!("Asset::asset_type_str() only applies to AssetConfig-backed asset kinds")
    }

    /// Recover this asset's concrete type from a `&dyn Asset`/`Box<dyn Asset>`
    /// — for the handful of call sites that need one specific asset kind
    /// (e.g. a PV-only sim-inject handler), not a generic `Asset` operation.
    /// Deliberately narrow: prefer a real trait method for anything more
    /// than a one-off, narrowly-scoped need.
    ///
    /// No default body: `self` inside a default method is `Self`-generic and
    /// unsized from the trait's own point of view, so coercing it to
    /// `&dyn Any` would require a `Self: Sized` bound — which would make this
    /// method uncallable through `dyn Asset` at all, defeating the purpose.
    /// Each concrete type's `impl Asset` provides the one-line body instead
    /// (trivially `self`, since every type is already `Sized`).
    fn as_any(&self) -> &dyn std::any::Any;

    /// Mutable counterpart of `as_any` — see its doc comment.
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any;

    // ── Optional capabilities (Spec A D4) ───────────────────────────────────
    //
    // Unlike the universal methods above, these three have a safe, meaningful
    // default: `None`, "this asset kind doesn't do that." Each is implemented
    // only by the asset kinds D4's audit found already have real (non-`_ =>
    // None`) behavior for it. A type simply doesn't implement the trait if it
    // has no such capability, rather than implementing a stub that returns
    // `None` — see design.md Decision D4's rationale.

    /// This asset's MILP planning context, if it participates in MILP
    /// scheduling at all (Battery/EV/Heater today; PV/BaseLoad do not).
    fn as_milp_participant(&self) -> Option<&dyn MilpParticipant> {
        None
    }

    /// Whether a user can issue a direct request against this asset (target
    /// SoC, surplus absorption) — storage-shaped assets only (Battery/EV
    /// today).
    fn as_request_resolvable(&self) -> Option<&dyn RequestResolvable> {
        None
    }

    /// Whether this asset has thermostat-shaped behavior (Heater only today).
    fn as_thermostat(&self) -> Option<&dyn Thermostat> {
        None
    }

    /// Whether this asset accepts tick-time environment/Behaviour-C overrides
    /// (irradiance, ambient temp, plugged-state, etc.) from `SimState::tick()`
    /// — Pv/Heater/BaseLoad/Ev today, not Battery (design.md Decision D5).
    /// `&mut self` (unlike the other three capability accessors) because
    /// every implementor mutates.
    fn as_tick_overridable(&mut self) -> Option<&mut dyn TickOverridable> {
        None
    }

    /// Project state forward over an explicit setpoint schedule (default impl).
    /// `setpoints` is a list of (slot_start, setpoint_kw) pairs in ascending time order.
    fn simulate_forward(
        &self,
        initial: &AssetState,
        setpoints: &[(DateTime<Utc>, f64)],
    ) -> Trajectory {
        let mut state = initial.clone();
        let mut points = Vec::new();
        for window in setpoints.windows(2) {
            let (ts, sp) = window[0];
            let dt = window[1].0 - ts;
            let (next, actual_kw) = self.step(&state, sp, dt);
            points.push(TrajectoryPoint {
                ts,
                power_kw: actual_kw,
                state: state.clone(),
            });
            state = next;
        }
        if let Some(&(ts, sp)) = setpoints.last() {
            let (_, actual_kw) = self.step(&state, sp, Duration::seconds(0));
            points.push(TrajectoryPoint {
                ts,
                power_kw: actual_kw,
                state,
            });
        }
        Trajectory { points }
    }
}

/// Capability: this asset participates in MILP planning. Implemented by
/// Battery/EV/Heater — exactly `AssetMilpContext`'s existing `AssetKind` scope
/// (`VEN/src/controller/asset_milp_port.rs`). See design.md Decision D4's
/// correction note for why `default_comfort_rates`/`default_completion_policy`/
/// `default_post_deadline_comfort_bid` are NOT here despite sounding
/// MILP-specific — all 5 asset kinds already implement them for real, so
/// they're universal `Asset` methods instead.
#[allow(dead_code)] // implemented starting Spec A Phase 2a (asset-dispatch-trait-objects tasks.md sec. 4); no implementor yet
pub trait MilpParticipant {
    /// Build the MILP context for this asset. Signature carries the full
    /// per-planning-cycle context every implementor needs (session/target
    /// state, reward weights) even though most parameters apply to only one
    /// or two asset kinds — see `AssetConfig::build_milp_context`'s existing
    /// doc history for why this wasn't split further.
    #[allow(clippy::too_many_arguments)] // one entry point for 3 heterogeneous asset kinds' MILP setup — see trait doc
    fn build_milp_context(
        &self,
        state: &AssetState,
        n: usize,
        cum_s: &[i64],
        now: DateTime<Utc>,
        ev_session: Option<&EvSession>,
        heater_target: Option<&HeaterTarget>,
        ev_min_charge_kw: f64,
        v_ev_extra_eur_kwh: f64,
        v_ev_core_eur_kwh: f64,
        asap_lateness_eur_kwh_h: f64,
        v_ev_free_charge_eur_kwh: f64,
        lambda_sw: f64,
        c_terminal_eur_kwh: f64,
        heater_anchor: Vec<Option<f64>>,
        w_ghg_eur_kg: f64,
    ) -> Box<dyn crate::controller::milp_planner::AssetMilpContext>;
}

/// Capability: a user can issue a direct request against this asset (a target
/// SoC/power, or opportunistic surplus absorption). Implemented by the two
/// storage-shaped assets, Battery and EV.
#[allow(dead_code)] // implemented starting Spec A Phase 2a (asset-dispatch-trait-objects tasks.md sec. 4); no implementor yet
pub trait RequestResolvable {
    /// Resolve a user request into `(energy_kwh, power_kw)`, or `None` if the
    /// request implies no meaningful action (e.g. already at/above target).
    fn resolve_request_target(
        &self,
        state: &AssetState,
        target_soc: Option<f64>,
        desired_power_kw: Option<f64>,
    ) -> Option<(f64, f64)>;

    /// `(discharge_kwh, charge_kwh)` currently available, or `None` if the
    /// asset can't participate right now (e.g. an unplugged EV).
    fn available_storage_kwh(&self, state: &AssetState) -> Option<(f64, f64)>;

    /// How much of `surplus_kw` this asset could opportunistically absorb
    /// right now, or `None` if it can't absorb surplus at all (e.g. EV
    /// already at its charge target, or unplugged).
    fn surplus_charge_kw(&self, state: &AssetState, surplus_kw: f64) -> Option<f64>;
}

/// Capability: this asset has thermostat-shaped behavior (a target
/// temperature driving an on/off or discrete-stage setpoint). Implemented by
/// Heater only, today.
#[allow(dead_code)] // implemented starting Spec A Phase 2a (asset-dispatch-trait-objects tasks.md sec. 4); no implementor yet
pub trait Thermostat {
    /// A stateful trajectory computer seeded from the live state, for
    /// recomputing the plan's own thermal trajectory — `None` if the current
    /// state gives no basis to project from.
    fn plan_trajectory(&self, live_state: &AssetState) -> Option<HeaterPlanTrajectory>;

    /// The on/off (or discrete-stage) setpoint \[kW\] that drives temperature
    /// toward `target_c` from the current state.
    fn thermostat_setpoint_kw(&self, state: &AssetState, target_c: f64) -> f64;
}

// ─── AssetHandle ──────────────────────────────────────────────────────────────

/// Wraps individual fields from a `(AssetConfig, AssetEntry)` pair to implement
/// the full `Asset` trait, including `id()`, `current_state()`, and `history()`.
///
/// Takes individual field references instead of `&AssetEntry` to avoid a circular
/// dependency (`AssetEntry` lives in `simulator`, which imports from `assets`).
///
/// Usage:
/// ```ignore
/// let handle = AssetHandle {
///     config: &entry_config,
///     id: &entry.id,
///     state: &entry.state,
///     history: &entry.history,
/// };
/// ```
// AssetHandle is used in tests and serves as the intended path for dyn Asset dispatch.
#[allow(dead_code)]
pub struct AssetHandle<'a> {
    pub config: &'a AssetConfig,
    pub id: &'a str,
    pub state: &'a AssetState,
    pub history: &'a AssetHistoryBuffer,
}

impl<'a> Asset for AssetHandle<'a> {
    fn id(&self) -> &str {
        self.id
    }

    fn current_state(&self) -> AssetState {
        self.state.clone()
    }

    fn history(&self, window: Duration, now: DateTime<Utc>) -> Vec<HistoryPoint> {
        self.history.slice(window, now)
    }

    fn capability(&self, state: &AssetState) -> AssetCapability {
        self.config.capability(state)
    }

    fn flexibility_floor(&self, state: &AssetState) -> AssetFlexibilityFloor {
        self.config.flexibility_floor(state)
    }

    fn step(&self, state: &AssetState, setpoint_kw: f64, dt: Duration) -> (AssetState, f64) {
        self.config.step(state, setpoint_kw, dt)
    }

    // simulate_forward: default impl inherited from Asset

    /// `AssetHandle<'a>` isn't `'static` (it borrows), so it can never
    /// actually be represented as `dyn Any` — downcast the concrete
    /// `AssetConfig` variant it wraps instead. Never called in practice:
    /// `to_boxed_asset()` (the trait-object construction path) hands out the
    /// owned concrete type directly, not an `AssetHandle`.
    fn as_any(&self) -> &dyn std::any::Any {
        unimplemented!(
            "AssetHandle::as_any() is not supported — downcast the concrete asset type instead"
        )
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        unimplemented!(
            "AssetHandle::as_any_mut() is not supported — downcast the concrete asset type instead"
        )
    }
}

/// Capability: this asset accepts tick-time environment/Behaviour-C overrides
/// from `SimState::tick()`. Implemented by Pv/Heater/BaseLoad/Ev — Battery has
/// no arm in `tick()`'s current match, so it declines.
///
/// Deferred from Spec A's Phase 0 (design.md Decision D5's addendum): the
/// self-contained `&mut self` shape only works once each implementor's
/// cross-cutting tick-level state (PV's/BaseLoad's smoothing) is resolved
/// *before* this is called, not inside it — see `SimState::tick()`, which
/// resolves `TickOverrides`' fields ahead of the per-asset loop.
pub trait TickOverridable {
    /// `state` is threaded through (unlike the other three capability traits)
    /// because EV's plugged-state override writes to `AssetState`, not just
    /// its own config — every other implementor ignores it.
    fn apply_tick_overrides(&mut self, state: &mut AssetState, overrides: &TickOverrides);
}

/// Bundles the per-tick override inputs `TickOverridable` implementors need,
/// pre-resolved where resolution requires cross-asset state (`pv_irradiance`/
/// `pv_irradiance_offset`, `base_load_baseline_kw` — see `SimState::tick()`'s
/// pre-loop resolution). One flat struct shared by 4 heterogeneous asset
/// kinds, each reading only its own fields — same shape as
/// `MilpParticipant::build_milp_context`'s signature, shared by 3 kinds.
pub struct TickOverrides {
    // PV
    pub pv_irradiance: f64,
    pub pv_irradiance_offset: f64,
    pub pv_alpha: f64,
    pub pv_generation_limit_kw: Option<f64>,
    pub pv_curtailment_source: PvCurtailmentSource,
    pub pv_weather_power_kw: Option<f64>,
    pub pv_measured_power_kw: Option<f64>,
    pub pv_irradiance_forced: bool,

    // Heater
    pub heater_ambient_temp_c_override: Option<f64>,
    pub heater_temp_min_override: Option<f64>,
    pub heater_temp_max_override: Option<f64>,
    pub heater_emergency_curtail_override: Option<bool>,
    pub heater_emergency_absorb_override: Option<bool>,

    // BaseLoad — `base_load_baseline_kw` is the pre-resolved
    // `self.base_load_smoothing.update(...)` result, `None` if no BaseLoad
    // asset is configured (see `SimState::tick()`).
    pub base_load_measured_kw: Option<f64>,
    pub base_load_baseline_kw: Option<f64>,

    // EV
    pub ev_plugged_override: Option<bool>,
    pub ev_soc_target_override: Option<f64>,
}

#[cfg(test)]
mod handle_tests {
    use super::*;
    use crate::assets::battery::{Battery, BatteryState};

    fn make_battery_state(soc: f64, power_kw: f64) -> AssetState {
        AssetState::Battery(BatteryState {
            soc,
            actual_power_kw: power_kw,
        })
    }

    fn make_battery_config(capacity_kwh: f64, max_kw: f64) -> AssetConfig {
        AssetConfig::Battery(Battery {
            capacity_kwh,
            max_charge_kw: max_kw,
            max_discharge_kw: max_kw,
            round_trip_efficiency: 1.0,
            min_soc: 0.1,
        })
    }

    #[test]
    fn handle_id_returns_given_id() {
        let state = make_battery_state(0.5, 0.0);
        let config = make_battery_config(10.0, 5.0);
        let history = AssetHistoryBuffer::new(3600);
        let handle = AssetHandle {
            config: &config,
            id: "bat-01",
            state: &state,
            history: &history,
        };
        assert_eq!(handle.id(), "bat-01");
    }

    #[test]
    fn handle_current_state_returns_state() {
        let state = make_battery_state(0.7, 2.0);
        let config = make_battery_config(10.0, 5.0);
        let history = AssetHistoryBuffer::new(3600);
        let handle = AssetHandle {
            config: &config,
            id: "bat",
            state: &state,
            history: &history,
        };
        match handle.current_state() {
            AssetState::Battery(s) => {
                assert!((s.soc - 0.7).abs() < 1e-9);
                assert!((s.actual_power_kw - 2.0).abs() < 1e-9);
            }
            _ => panic!("expected Battery state"),
        }
    }

    #[test]
    fn handle_history_delegates_to_buffer() {
        let state = make_battery_state(0.5, 0.0);
        let config = make_battery_config(10.0, 5.0);
        let mut history = AssetHistoryBuffer::new(3600);
        let now = Utc::now();
        history.push(HistoryPoint {
            ts: now,
            power_kw: 3.0,
            state: make_battery_state(0.5, 3.0),
        });
        let handle = AssetHandle {
            config: &config,
            id: "bat",
            state: &state,
            history: &history,
        };
        let hist = handle.history(Duration::seconds(60), now);
        assert_eq!(hist.len(), 1);
        assert!((hist[0].power_kw - 3.0).abs() < 1e-9);
    }

    #[test]
    fn handle_capability_delegates_to_config() {
        let state = make_battery_state(0.5, 0.0);
        let config = make_battery_config(10.0, 5.0);
        let history = AssetHistoryBuffer::new(3600);
        let handle = AssetHandle {
            config: &config,
            id: "bat",
            state: &state,
            history: &history,
        };
        let cap = handle.capability(&state);
        // mid-SoC battery (soc=0.5, min_soc=0.1): can charge up to 5 kW and discharge up to 5 kW
        assert!((cap.max_import_kw - 5.0).abs() < 1e-9);
        assert!((cap.max_export_kw + 5.0).abs() < 1e-9); // -5.0
    }

    #[test]
    fn handle_step_delegates_to_config() {
        let state = make_battery_state(0.5, 0.0);
        let config = make_battery_config(10.0, 5.0);
        let history = AssetHistoryBuffer::new(3600);
        let handle = AssetHandle {
            config: &config,
            id: "bat",
            state: &state,
            history: &history,
        };
        let (new_state, actual_kw) = handle.step(&state, 5.0, Duration::seconds(3600));
        // 1 hour at 5 kW on 10 kWh battery → SoC goes from 0.5 to 1.0 (full)
        match new_state {
            AssetState::Battery(s) => assert!((s.soc - 1.0).abs() < 1e-6),
            _ => panic!("expected Battery state"),
        }
        assert!(actual_kw > 0.0);
    }

    fn soc_of(state: &AssetState) -> f64 {
        match state {
            AssetState::Battery(s) => s.soc,
            other => panic!("expected Battery state, got {other:?}"),
        }
    }

    // ── Asset trait defaults: simulate_forward ──
    //
    // AssetHandle doesn't override these -- they're the trait's own default
    // implementations (real accumulation/projection logic used by lookahead
    // precompute), exercised here via a battery-backed handle rather than tested
    // in isolation, since they only make sense in terms of a concrete Asset.

    #[test]
    fn simulate_forward_reports_pre_step_state_paired_with_post_step_actual_power() {
        // capacity=10kWh, max_charge=5kW, RTE=1.0, min_soc=0.1, initial soc=0.2.
        // Three setpoints (two 1h windows + a trailing zero-duration point) at a
        // constant 3 kW charge -- deliberately picks a non-obvious contract: each
        // TrajectoryPoint pairs the state *before* its window's step with the
        // *actual* (possibly clamped) power achieved *during* that step, not the
        // state after. Getting this backwards would silently corrupt every lookahead
        // precompute consumer.
        let config = make_battery_config(10.0, 5.0);
        let t0 = Utc::now();
        let initial = make_battery_state(0.2, 0.0);
        let history = AssetHistoryBuffer::new(3600);
        let handle = AssetHandle {
            config: &config,
            id: "bat",
            state: &initial,
            history: &history,
        };

        let setpoints = [
            (t0, 3.0),
            (t0 + Duration::hours(1), 3.0),
            (t0 + Duration::hours(2), 3.0),
        ];
        let traj = handle.simulate_forward(&initial, &setpoints);

        assert_eq!(traj.points.len(), 3);

        assert_eq!(traj.points[0].ts, t0);
        assert!((traj.points[0].power_kw - 3.0).abs() < 1e-9);
        assert!((soc_of(&traj.points[0].state) - 0.2).abs() < 1e-9); // pre-step SoC

        assert_eq!(traj.points[1].ts, t0 + Duration::hours(1));
        assert!((traj.points[1].power_kw - 3.0).abs() < 1e-9);
        assert!((soc_of(&traj.points[1].state) - 0.5).abs() < 1e-9); // 0.2 + 3kWh/10kWh

        assert_eq!(traj.points[2].ts, t0 + Duration::hours(2));
        assert!((soc_of(&traj.points[2].state) - 0.8).abs() < 1e-9); // 0.5 + 3kWh/10kWh
    }

    #[test]
    fn simulate_forward_reports_clamped_actual_power_not_the_requested_setpoint() {
        // Requesting 20 kW on a battery whose max_charge_kw is 5.0 must show up in
        // the trajectory as the clamped 5.0, not the raw (infeasible) request --
        // this is the whole reason simulate_forward reports "actual", not "requested".
        let config = make_battery_config(10.0, 5.0);
        let t0 = Utc::now();
        let initial = make_battery_state(0.2, 0.0);
        let history = AssetHistoryBuffer::new(3600);
        let handle = AssetHandle {
            config: &config,
            id: "bat",
            state: &initial,
            history: &history,
        };

        let setpoints = [(t0, 20.0), (t0 + Duration::hours(1), 20.0)];
        let traj = handle.simulate_forward(&initial, &setpoints);

        assert_eq!(traj.points.len(), 2);
        assert!(
            (traj.points[0].power_kw - 5.0).abs() < 1e-9,
            "power must be clamped to max_charge_kw, got {}",
            traj.points[0].power_kw
        );
    }
}
