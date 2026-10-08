//! R-59: what the VEN does while it cannot reach its VTN.
//!
//! `CommsLossState` is the resolved verdict for one tick, and
//! `pv_generation_limit` is how the PV resolver reads it. Both lived in
//! `tasks/sim_tick/` — the adapter ring, whose job is to run things on a
//! timer — although deciding how a site behaves when the VTN is unreachable
//! is a control decision. Moving them here is also what gives
//! `tasks/sim_tick` room under the `tasks/` size cap, instead of a further
//! split of a file already named `helpers`.

use chrono::{DateTime, Utc};

use crate::controller::dispatcher::ResolvedPvGenerationLimit;
use crate::controller::SimSnapshot;
use crate::entities::capacity::OadrCapacityState;
use crate::entities::plan::Plan;
use crate::entities::sim_inject::SimInjectState;

/// Resolved comms-loss curtailment state for one tick. `active` is the
/// debounced "the VTN has been unreachable long enough" verdict, resolved
/// once per tick so the PV resolver and the EV/heater/battery clamp cannot
/// read different answers. `None` overall — not merely `active: false` —
/// means the profile has no `comms_loss:` section at all (opt-out fast path).
#[derive(Debug, Clone, Copy)]
pub struct CommsLossState {
    pub active: bool,
    pub max_power_pct: f64,
}

impl CommsLossState {
    /// The PV ceiling this state imposes [kW, signed as the resolver expects],
    /// or `None` when it imposes none. Reads the PV asset's own
    /// `inverter_max_kw` rather than assuming a rating.
    pub fn pv_ceiling_kw(&self, sim: &SimSnapshot) -> Option<f64> {
        if !self.active {
            return None;
        }
        sim.assets
            .get(crate::ids::ASSET_PV)
            .and_then(|s| s.ac_ceiling_kw)
            .map(|max_kw| self.max_power_pct * max_kw)
    }
}

/// The PV generation limit in force this tick, across every source that can
/// impose one: the VTN's capacity state (with any sim-injected stand-in), the
/// adopted plan, the deviation arbiter, a manual operator cap, and
/// comms-loss curtailment.
///
/// A thin composition over `dispatcher::resolve_pv_generation_limit_kw`,
/// which owns the precedence rules; this adds only the two inputs that have
/// to be derived first.
pub fn pv_generation_limit(
    sim: &SimSnapshot,
    plan: Option<&Plan>,
    capacity: &OadrCapacityState,
    inject: &SimInjectState,
    now: DateTime<Utc>,
    arbiter_tighten_kw: Option<f64>,
    comms_loss: Option<CommsLossState>,
) -> ResolvedPvGenerationLimit {
    crate::controller::dispatcher::resolve_pv_generation_limit_kw(
        plan,
        &capacity.with_sim_injected_limits(inject),
        now,
        arbiter_tighten_kw,
        inject.pv_generation_limit_kw,
        comms_loss.and_then(|c| c.pv_ceiling_kw(sim)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assets::pv::PvInverter;
    use crate::entities::asset_params::PvParams;
    use crate::services::test_support::asset_snapshots::snapshot_from_asset;

    fn empty_sim() -> SimSnapshot {
        SimSnapshot {
            ts: Utc::now(),
            grid: crate::controller::simulator_port::GridSnapshot {
                net_power_w: 0.0,
                voltage_v: 230.0,
                import_kwh: 0.0,
                export_kwh: 0.0,
                // Unbounded, i.e. no VTN capacity event — see GridSnapshot's
                // own doc comments; 0.0 would read as "no import allowed".
                import_limit_kw: f64::MAX,
                export_limit_kw: -f64::MAX,
            },
            assets: Default::default(),
        }
    }

    /// Built from a real `PvInverter`, so `inverter_max_kw` is whatever the
    /// asset itself reports rather than a hand-written fixture value.
    fn sim_with_pv(inverter_max_kw: f64) -> SimSnapshot {
        let params = PvParams {
            id: crate::ids::ASSET_PV.to_string(),
            rated_kw: inverter_max_kw,
            inverter_max_kw,
            co2_g_kwh: 0.0,
        };
        let pv = PvInverter::from_params(&params);
        let state = crate::assets::AssetState::Pv(PvInverter::initial_state(&params));
        let mut snap = empty_sim();
        snap.assets.insert(
            crate::ids::ASSET_PV.to_string(),
            snapshot_from_asset(&pv, state, crate::ids::ASSET_PV, 0.0, 0.0),
        );
        snap
    }

    fn state(active: bool) -> CommsLossState {
        CommsLossState {
            active,
            max_power_pct: 0.3,
        }
    }

    #[test]
    fn an_inactive_state_imposes_no_ceiling() {
        assert_eq!(state(false).pv_ceiling_kw(&sim_with_pv(10.0)), None);
    }

    #[test]
    fn an_active_state_scales_the_pv_inverter_rating() {
        let ceiling = state(true).pv_ceiling_kw(&sim_with_pv(10.0)).unwrap();
        assert!(
            (ceiling - 3.0).abs() < 1e-9,
            "30 % of a 10 kW inverter, got {ceiling}"
        );
    }

    #[test]
    fn a_site_with_no_pv_asset_gets_no_ceiling() {
        assert_eq!(state(true).pv_ceiling_kw(&empty_sim()), None);
    }
}
