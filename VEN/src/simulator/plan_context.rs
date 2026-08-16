//! Plan-cycle helpers that need concrete simulator state.
//!
//! These functions operate on `SimState` (and asset physics types) directly, so they
//! live in the infra ring next to the simulator instead of the application layer —
//! `services/` must only touch the simulator through `SimulatorPort`.

use std::sync::Arc;

use chrono::{DateTime, Utc};
use tracing::{debug, warn};

use crate::assets::{AssetConfig, PvInverter};
use crate::controller::milp_planner::asset_port::AssetMilpContext;
use crate::entities::asset_params::AssetParams;
use crate::entities::device_session::{EvSession, HeaterTarget};
use crate::entities::planner_params::PlannerParams;
use crate::entities::sim_inject::SimInjectState;
use crate::simulator::SimState;

/// Clone the `SimState` under its Mutex, logging when the lock wait was long.
/// The clone releases the Mutex immediately — MILP solving takes 18–60s on
/// Node1 ARM64 and must never hold the sim lock for its duration.
pub async fn clone_sim_snapshot(
    sim: &Arc<tokio::sync::Mutex<SimState>>,
    trigger_reason: &str,
) -> SimState {
    let lock_start = std::time::Instant::now();
    let snap = sim.lock().await.clone();
    let lock_ms = lock_start.elapsed().as_millis();
    if lock_ms > 500 {
        warn!(lock_wait_ms = lock_ms, trigger = %trigger_reason, "planner: sim lock wait was long");
    } else {
        debug!(lock_wait_ms = lock_ms, "planner: sim lock acquired");
    }
    snap
}

/// Apply a pending one-shot PV-irradiance inject to a cloned sim snapshot, if the sim
/// tick hasn't applied it yet. No-op when `inject.pv_irradiance` is `None` — including
/// when the tick already applied and cleared it before this snapshot was cloned.
pub fn apply_pending_pv_inject(
    sim_snap: &mut SimState,
    inject: &SimInjectState,
    now: DateTime<Utc>,
) {
    if let Some(forced) = inject.pv_irradiance {
        let natural = PvInverter::natural_irradiance_at(now);
        if let Some((_, AssetConfig::Pv(pv))) = sim_snap.find_asset_mut(crate::ids::ASSET_PV) {
            pv.irradiance_offset = forced - natural;
            pv.pv_alpha = inject.pv_irradiance_alpha;
        }
    }
}

/// Build per-asset MILP contexts from live simulator state, for the current plan cycle.
///
/// Must run before the blocking solve so asset states are captured at this instant, not
/// whenever the solver thread happens to get scheduled.
#[allow(clippy::too_many_arguments)]
pub fn build_asset_contexts(
    sim_snap: &SimState,
    n_slots: usize,
    cum_s: &[i64],
    now: DateTime<Utc>,
    ev_sess: Option<&EvSession>,
    heat_tgt: Option<&HeaterTarget>,
    asset_params: &[AssetParams],
    planner: &PlannerParams,
    lambda_sw: f64,
    heater_c_terminal_eur_kwh: f64,
    battery_c_terminal_eur_kwh: f64,
    heater_anchor: &[Option<f64>],
) -> Vec<Box<dyn AssetMilpContext>> {
    let min_ev_charge_kw = asset_params
        .iter()
        .find_map(|p| match p {
            AssetParams::Ev(e) => Some(e.min_charge_kw),
            _ => None,
        })
        .unwrap_or(0.0);

    sim_snap
        .iter_assets()
        .filter_map(|(entry, cfg)| {
            // Use per-asset c_terminal: heater and battery get their own coefficient;
            // EV gets 0.0 (deadline constraint handles the charging incentive).
            let c_terminal = match cfg {
                AssetConfig::Heater(_) => heater_c_terminal_eur_kwh,
                AssetConfig::Battery(_) => battery_c_terminal_eur_kwh,
                _ => 0.0,
            };
            cfg.build_milp_context(
                &entry.state,
                n_slots,
                cum_s,
                now,
                ev_sess,
                heat_tgt,
                min_ev_charge_kw,
                planner.v_ev_extra_eur_kwh,
                planner.v_ev_core_eur_kwh,
                planner.asap_lateness_eur_kwh_h,
                planner.v_ev_free_charge_eur_kwh,
                lambda_sw,
                c_terminal,
                if matches!(cfg, AssetConfig::Heater(_)) {
                    heater_anchor.to_vec()
                } else {
                    vec![]
                },
                planner.w_ghg,
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::controller::milp_planner::asset_port::{AssetKind, AssetMilpParams};

    #[test]
    fn apply_pending_pv_inject_noop_when_no_pv_asset() {
        // No PV asset present (or no pending inject) — must not panic, must be a no-op.
        let mut sim_snap: SimState = serde_json::from_value(serde_json::json!({
            "asset_configs": [],
            "assets": [],
            "grid": {
                "net_power_w": 0.0, "import_w": 0.0, "export_w": 0.0,
                "voltage_v": 0.0, "import_kwh": 0.0, "export_kwh": 0.0
            },
            "last_tick": Utc::now().to_rfc3339()
        }))
        .expect("minimal SimState must deserialize");
        let inject = SimInjectState::default();
        apply_pending_pv_inject(&mut sim_snap, &inject, Utc::now());
        // No panic is the assertion — nothing else observable without a PV asset.
    }

    #[test]
    fn apply_pending_pv_inject_sets_offset_and_alpha_from_forced_irradiance() {
        use crate::entities::asset_params::{AssetParams, PvParams};

        let now = Utc::now();
        let mut sim_snap = SimState::from_params(&[AssetParams::Pv(PvParams::default())], now);
        let inject = SimInjectState {
            pv_irradiance: Some(0.9),
            pv_irradiance_alpha: 0.25,
            ..Default::default()
        };

        apply_pending_pv_inject(&mut sim_snap, &inject, now);

        let natural = PvInverter::natural_irradiance_at(now);
        let (_, cfg) = sim_snap.find_asset(crate::ids::ASSET_PV).unwrap();
        match cfg {
            AssetConfig::Pv(pv) => {
                assert!(
                    (pv.irradiance_offset - (0.9 - natural)).abs() < 1e-9,
                    "offset must be forced-minus-natural irradiance"
                );
                assert!((pv.pv_alpha - 0.25).abs() < 1e-9);
            }
            other => panic!("expected Pv config, got {other:?}"),
        }
    }

    fn cum_seconds(n: usize, step_s: i64) -> Vec<i64> {
        (0..=n as i64).map(|i| i * step_s).collect()
    }

    #[test]
    fn build_asset_contexts_returns_one_per_milp_capable_asset_and_skips_others() {
        use crate::entities::asset_params::{
            AssetParams, BaseLoadParams, BatteryParams, EvParams, HeaterParams,
        };

        let now = Utc::now();
        let params = vec![
            AssetParams::Battery(BatteryParams {
                id: "battery".into(),
                capacity_kwh: 10.0,
                max_charge_kw: 3.0,
                max_discharge_kw: 3.0,
                initial_soc: 0.5,
                round_trip_efficiency: 0.95,
                min_soc: 0.1,
                c_terminal_eur_kwh: None,
            }),
            AssetParams::Heater(HeaterParams {
                id: "heater".into(),
                ..Default::default()
            }),
            AssetParams::Ev(EvParams {
                id: "ev".into(),
                ..Default::default()
            }),
            AssetParams::BaseLoad(BaseLoadParams {
                id: "base_load".into(),
                baseline_kw: 1.0,
                spikes: vec![],
            }),
        ];
        let sim_snap = SimState::from_params(&params, now);
        let planner = PlannerParams::default();
        let cum_s = cum_seconds(4, 600);

        let contexts = build_asset_contexts(
            &sim_snap,
            4,
            &cum_s,
            now,
            None,
            None,
            &params,
            &planner,
            0.0,
            0.07,
            0.03,
            &[],
        );

        assert_eq!(
            contexts.len(),
            3,
            "base_load is not MILP-capable and must be skipped, not produce a context"
        );
        let mut got: Vec<(String, AssetKind)> = contexts
            .iter()
            .map(|c| (c.asset_id().to_string(), c.asset_kind()))
            .collect();
        got.sort_by(|a, b| a.0.cmp(&b.0));
        assert_eq!(
            got,
            vec![
                ("battery".to_string(), AssetKind::Battery),
                ("ev".to_string(), AssetKind::Ev),
                ("heater".to_string(), AssetKind::Heater),
            ]
        );
    }

    #[test]
    fn build_asset_contexts_selects_c_terminal_by_asset_type_not_a_single_shared_value() {
        use crate::entities::asset_params::{AssetParams, BatteryParams, HeaterParams};

        let now = Utc::now();
        let params = vec![
            AssetParams::Battery(BatteryParams {
                id: "battery".into(),
                capacity_kwh: 10.0,
                max_charge_kw: 3.0,
                max_discharge_kw: 3.0,
                initial_soc: 0.5,
                round_trip_efficiency: 0.95,
                min_soc: 0.1,
                c_terminal_eur_kwh: None,
            }),
            AssetParams::Heater(HeaterParams {
                id: "heater".into(),
                ..Default::default()
            }),
        ];
        let sim_snap = SimState::from_params(&params, now);
        let planner = PlannerParams::default();
        let cum_s = cum_seconds(4, 600);

        // Deliberately distinct values so a bug that used one shared c_terminal
        // for every asset (instead of selecting per AssetConfig variant) would
        // fail this assertion.
        let contexts = build_asset_contexts(
            &sim_snap,
            4,
            &cum_s,
            now,
            None,
            None,
            &params,
            &planner,
            0.0,
            /* heater_c_terminal_eur_kwh */ 0.07,
            /* battery_c_terminal_eur_kwh */ 0.03,
            &[],
        );

        let heater_ctx = contexts
            .iter()
            .find(|c| c.asset_kind() == AssetKind::Heater)
            .expect("heater context must be present");
        match heater_ctx.milp_params(4, now) {
            AssetMilpParams::Heater(scalars) => {
                assert!(
                    (scalars.c_terminal_eur_kwh - 0.07).abs() < 1e-9,
                    "heater must receive heater_c_terminal_eur_kwh, not battery's value"
                );
            }
            other => panic!("expected Heater scalars, got {other:?}"),
        }
    }

    #[test]
    fn build_asset_contexts_extracts_min_ev_charge_kw_from_ev_asset_params() {
        use crate::entities::asset_params::{AssetParams, EvParams};

        let now = Utc::now();
        let params = vec![AssetParams::Ev(EvParams {
            id: "ev".into(),
            min_charge_kw: 2.2,
            ..Default::default()
        })];
        let sim_snap = SimState::from_params(&params, now);
        let planner = PlannerParams::default();
        let cum_s = cum_seconds(4, 600);

        let contexts = build_asset_contexts(
            &sim_snap,
            4,
            &cum_s,
            now,
            None,
            None,
            &params,
            &planner,
            0.0,
            0.0,
            0.0,
            &[],
        );

        let ev_ctx = &contexts[0];
        match ev_ctx.milp_params(4, now) {
            AssetMilpParams::Ev(scalars) => {
                assert!(
                    (scalars.p_min_kw - 2.2).abs() < 1e-9,
                    "min_ev_charge_kw must be scanned out of asset_params and threaded through"
                );
            }
            other => panic!("expected Ev scalars, got {other:?}"),
        }
    }
}
