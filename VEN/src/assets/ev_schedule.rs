//! `EvCharger`'s departure-aware forward projection — split out of `ev.rs` to
//! stay under the file-size cap. `ev-departure-consolidation`: this is the
//! mechanism that makes `Asset::simulate_forward` genuinely aware that the
//! car will leave at `departure_time`, replacing the site-level
//! `capacity_headroom.rs` `ev_session` workaround this phase retires.

use chrono::{DateTime, Duration, Utc};

use super::ev::EvCharger;
use super::{Asset, AssetState, Trajectory, TrajectoryPoint};

impl EvCharger {
    /// Forces `plugged=false` for any trajectory point at or after the live
    /// `departure_time` before delegating to `step()` — the one thing the
    /// trait default's step()-based walk can't express, since `step()` itself
    /// is genuinely setpoint+duration-driven (no timestamp needed for EV's
    /// own physics, unlike PV's `simulate_forward` override).
    /// `capability_inner`/`step_inner` already correctly zero out an
    /// unplugged EV, so no other asset-specific branch is needed anywhere
    /// downstream (unlike PV, whose own `max_effort_setpoint` ignores `state`
    /// entirely — see that method's doc comment).
    ///
    /// `step_inner`'s BL-12 response delay applies the *previous* step's
    /// command. That is one 1 s tick live, but a projection window is 60 s to
    /// 15 min, so each window's command is staged (a zero-length step) before
    /// it is integrated; otherwise every command would land one window late.
    pub(super) fn simulate_forward_inner(
        &self,
        initial: &AssetState,
        setpoints: &[(DateTime<Utc>, f64)],
    ) -> Trajectory {
        let AssetState::Ev(_) = initial else {
            unreachable!("EvCharger/state mismatch")
        };
        let mut state = initial.clone();
        let mut points = Vec::new();
        let force_unplugged = |state: &mut AssetState, ts: DateTime<Utc>| {
            if self.departure_time.is_some_and(|d| ts >= d) {
                if let AssetState::Ev(s) = state {
                    s.plugged = false;
                }
            }
        };
        let stage = |state: &AssetState, sp: f64| self.step(state, sp, Duration::zero()).0;
        for window in setpoints.windows(2) {
            let (ts, sp) = window[0];
            let dt = window[1].0 - ts;
            force_unplugged(&mut state, ts);
            let (next, actual_kw) = self.step(&stage(&state, sp), sp, dt);
            points.push(TrajectoryPoint {
                ts,
                power_kw: actual_kw,
                state: state.clone(),
            });
            state = next;
        }
        if let Some(&(ts, sp)) = setpoints.last() {
            force_unplugged(&mut state, ts);
            let (_, actual_kw) = self.step(&stage(&state, sp), sp, Duration::zero());
            points.push(TrajectoryPoint {
                ts,
                power_kw: actual_kw,
                state,
            });
        }
        Trajectory { points }
    }
}
