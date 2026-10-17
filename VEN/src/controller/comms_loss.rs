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
use crate::entities::capacity::{effective_capacity_limit, CapacitySnapshot};
use crate::entities::capacity_curve::CommitmentDirection;
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
    /// The site PV ceiling this state imposes [kW, signed as the resolver expects],
    /// or `None` when it imposes none (or the site has no inverter). The share of every
    /// array's own `ac_ceiling_kw`, summed: the generation limit is a site limit.
    pub fn pv_ceiling_kw(&self, sim: &SimSnapshot) -> Option<f64> {
        if !self.active {
            return None;
        }
        sim.assets
            .values()
            .filter_map(|s| s.ac_ceiling_kw)
            .reduce(|a, b| a + b)
            .map(|max_kw| self.max_power_pct * max_kw)
    }
}

/// The PV generation limit in force this tick, across every source that can
/// impose one: the VTN's export capacity limit (with any sim-injected stand-in),
/// the adopted plan, the deviation arbiter, a manual operator cap, and
/// comms-loss curtailment.
///
/// A thin composition over `dispatcher::resolve_pv_generation_limit_kw`,
/// which owns the precedence rules; this adds only the two inputs that have
/// to be derived first. The export limit is read from the capacity schedule at
/// `now` (GB-48), so it changes at its interval boundary, not at the next poll.
pub fn pv_generation_limit(
    sim: &SimSnapshot,
    plan: Option<&Plan>,
    capacity_schedule: &[CapacitySnapshot],
    inject: &SimInjectState,
    now: DateTime<Utc>,
    arbiter_tighten_kw: Option<f64>,
    comms_loss: Option<CommsLossState>,
) -> ResolvedPvGenerationLimit {
    crate::controller::dispatcher::resolve_pv_generation_limit_kw(
        plan,
        effective_capacity_limit(
            capacity_schedule,
            inject,
            CommitmentDirection::Export,
            now,
            now,
        )
        .map(|l| l.limit_kw),
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
    use crate::entities::capacity::CapacitySnapshot;
    use crate::services::test_support::asset_snapshots::snapshot_from_asset;

    fn empty_sim() -> SimSnapshot {
        SimSnapshot {
            ts: Utc::now(),
            grid: crate::controller::sim_snapshot::GridSnapshot {
                net_power_kw: 0.0,
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
        sim_with_arrays(&[(crate::ids::ASSET_PV, inverter_max_kw)])
    }

    fn sim_with_arrays(arrays: &[(&str, f64)]) -> SimSnapshot {
        let mut snap = empty_sim();
        for &(id, inverter_max_kw) in arrays {
            let params = PvParams {
                id: id.to_string(),
                rated_kw: inverter_max_kw,
                inverter_max_kw,
                co2_g_kwh: 0.0,
            };
            let pv = PvInverter::from_params(&params);
            let state = crate::assets::AssetState::Pv(PvInverter::initial_state(&params));
            snap.assets
                .insert(id.to_string(), snapshot_from_asset(&pv, state, 0.0, 0.0));
        }
        snap
    }

    /// W5b: the site ceiling is the share of every array's rating, whatever their ids.
    #[test]
    fn two_arrays_give_the_share_of_their_summed_rating() {
        let sim = sim_with_arrays(&[("pv-east", 6.0), ("pv-west", 4.0)]);
        let ceiling = state(true).pv_ceiling_kw(&sim).unwrap();
        assert!((ceiling - 3.0).abs() < 1e-9, "30 % of 10 kW, got {ceiling}");
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

    // GB-48 B': the export limit behind the PV generation limit is read from the
    // capacity schedule at the tick's own `now`, never from a value stored at the
    // last event poll.
    fn export_limit_segment(from: DateTime<Utc>, to: DateTime<Utc>, kw: f64) -> CapacitySnapshot {
        CapacitySnapshot {
            interval_start: from,
            interval_end: to,
            export_limit_kw: Some(kw),
            export_limit_event_id: Some("vtn-export".to_string()),
            ..CapacitySnapshot::default()
        }
    }

    #[test]
    fn the_pv_limit_follows_the_export_limit_schedule_at_the_ticks_own_time() {
        use chrono::{Duration, TimeZone};
        let start = Utc.with_ymd_and_hms(2026, 9, 15, 12, 0, 0).unwrap();
        let schedule = [export_limit_segment(start, start + Duration::hours(1), 2.0)];
        let sim = sim_with_pv(10.0);
        let limit_at = |now| {
            pv_generation_limit(
                &sim,
                None,
                &schedule,
                &SimInjectState::default(),
                now,
                None,
                None,
            )
            .limit_kw
        };
        // The same schedule, three ticks, no poll between them.
        assert_eq!(
            limit_at(start - Duration::seconds(1)),
            None,
            "not in force yet"
        );
        assert_eq!(limit_at(start), Some(-2.0), "in force from its start");
        assert_eq!(
            limit_at(start + Duration::hours(1)),
            None,
            "ended at its end"
        );
    }

    #[test]
    fn an_injected_export_limit_stands_in_only_where_no_vtn_limit_applies() {
        use chrono::{Duration, TimeZone};
        let start = Utc.with_ymd_and_hms(2026, 9, 15, 12, 0, 0).unwrap();
        let schedule = [export_limit_segment(start, start + Duration::hours(1), 2.0)];
        let inject = SimInjectState {
            grid_export_limit_kw: Some(7.0),
            ..SimInjectState::default()
        };
        let sim = sim_with_pv(10.0);
        let limit_at =
            |now| pv_generation_limit(&sim, None, &schedule, &inject, now, None, None).limit_kw;
        assert_eq!(
            limit_at(start + Duration::minutes(5)),
            Some(-2.0),
            "a VTN limit wins"
        );
        assert_eq!(
            limit_at(start - Duration::minutes(5)),
            Some(-7.0),
            "the stand-in fills the gap"
        );
    }
}
