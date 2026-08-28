use chrono::{DateTime, Duration, Utc};

use super::{AssetCapability, AssetConfig, AssetFlexibilityFloor, AssetHistoryBuffer, AssetState};
use crate::assets::HistoryPoint;

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
