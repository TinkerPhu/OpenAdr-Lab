//! Plan-cycle helpers that need concrete simulator state.
//!
//! These functions operate on `SimState` (and asset physics types) directly, so they
//! live in the infra ring next to the simulator instead of the application layer —
//! `services/` must only touch the simulator through its ports (`SimRosterPort`, `SimReadPort`,
//! `HeadroomPort`, via `SimHandle`).

use std::sync::Arc;

use chrono::{DateTime, Utc};
use tracing::{debug, warn};

use crate::assets::PvInverter;
use crate::controller::milp_planner::asset_port::AssetMilpContext;
use crate::entities::device_session::HeaterTarget;
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
        if let Some((_, cfg)) = sim_snap.find_asset_mut(crate::ids::ASSET_PV) {
            if let Some(pv) = cfg.as_any_mut().downcast_mut::<PvInverter>() {
                pv.irradiance_offset = forced - natural;
                pv.tau_s = inject.pv_tau_s;
            }
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
    // Each EV's queued sessions and each heater's target, by asset id (W4): an asset is handed
    // only its own.
    ev_sessions: &std::collections::BTreeMap<
        String,
        crate::entities::device_session::EvSessionQueue,
    >,
    heater_targets: &std::collections::BTreeMap<String, HeaterTarget>,
    planner: &PlannerParams,
    // Per-asset terminal value of stored energy, keyed by asset id — built
    // by `services::planning::build_plan_cycle_inputs` from each asset's own
    // params. Looked up, not branched on: which kinds have one is declared
    // in `AssetParams::terminal_value_eur_kwh`, not decided here.
    c_terminal_eur_kwh_by_asset: &std::collections::HashMap<String, f64>,
    // Each heater's anchor, by asset id (`services::planning::build_heater_anchor`).
    heater_anchor: &std::collections::BTreeMap<String, Vec<Option<f64>>>,
    // User comfort-curve overrides, by asset id (`AppState::comfort_overrides_map`).
    // Each asset is priced by its override when it has one, its built-in default
    // otherwise — the same resolution the API and session creation use.
    comfort_overrides: &std::collections::HashMap<String, Vec<crate::entities::asset::ComfortRate>>,
    // The PV forecast pin (sim inject `pv_plan_kw`); the PV applies it to its own forecast.
    pv_forecast_override: Option<f64>,
) -> Vec<Box<dyn AssetMilpContext>> {
    sim_snap
        .iter_assets()
        .filter_map(|(entry, cfg)| {
            let comfort_rates = crate::entities::comfort::effective_comfort_rates(
                comfort_overrides,
                &entry.id,
                cfg.default_comfort_rates(),
            );
            let inputs = crate::assets::MilpBuildInputs {
                asset_id: &entry.id,
                state: &entry.state,
                n: n_slots,
                cum_s,
                now,
                ev_sessions: ev_sessions
                    .get(&entry.id)
                    .map(|q| q.as_slice())
                    .unwrap_or(&[]),
                heater_target: heater_targets.get(&entry.id),
                comfort_rates: &comfort_rates,
                planner,
                c_terminal_eur_kwh: c_terminal_eur_kwh_by_asset
                    .get(&entry.id)
                    .copied()
                    .unwrap_or(0.0),
                heater_anchor: if cfg.accepts_dispatch_anchor() {
                    heater_anchor.get(&entry.id).cloned().unwrap_or_default()
                } else {
                    vec![]
                },
                pv_forecast_override,
            };
            Some(cfg.as_milp_participant()?.build_milp_context(&inputs))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::controller::milp_planner::asset_port::{
        BatteryMilpContext, EvMilpContext, HeaterMilpContext,
    };

    /// Which kind of context each built context is, by asset id: what a test reads to see
    /// that a roster entry became the right planner participant.
    fn kind_of(ctx: &dyn crate::controller::milp_planner::AssetMilpContext) -> &'static str {
        let any = ctx.as_any();
        if any.is::<BatteryMilpContext>() {
            "battery"
        } else if any.is::<EvMilpContext>() {
            "ev"
        } else if any.is::<HeaterMilpContext>() {
            "heater"
        } else if any.is::<crate::controller::milp_planner::ForecastMilpContext>() {
            "forecast"
        } else {
            "other"
        }
    }
    use chrono::TimeZone;

    /// Distinct per-asset terminal values, so a test can tell which one an
    /// asset actually received. Deliberately *not* built via
    /// `AssetParams::terminal_value_eur_kwh`: these tests check that
    /// `build_asset_contexts` routes each asset its own entry, which a map
    /// derived from the same rule under test could not show.
    fn terminal_values() -> std::collections::HashMap<String, f64> {
        std::collections::HashMap::from([
            (crate::ids::ASSET_HEATER.to_string(), 0.07),
            (crate::ids::ASSET_BATTERY.to_string(), 0.03),
        ])
    }

    #[test]
    fn apply_pending_pv_inject_noop_when_no_pv_asset() {
        // No PV asset present (or no pending inject) — must not panic, must be a no-op.
        let mut sim_snap: SimState = serde_json::from_value(serde_json::json!({
            "asset_configs": [],
            "assets": [],
            "grid": {
                "net_power_kw": 0.0, "import_kw": 0.0, "export_kw": 0.0,
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
            pv_tau_s: 1200.0,
            ..Default::default()
        };

        apply_pending_pv_inject(&mut sim_snap, &inject, now);

        let natural = PvInverter::natural_irradiance_at(now);
        let (_, cfg) = sim_snap.find_asset(crate::ids::ASSET_PV).unwrap();
        let pv = cfg
            .as_any()
            .downcast_ref::<PvInverter>()
            .expect("expected Pv config");
        assert!(
            (pv.irradiance_offset - (0.9 - natural)).abs() < 1e-9,
            "offset must be forced-minus-natural irradiance"
        );
        assert!((pv.tau_s - 1200.0).abs() < 1e-9);
    }

    fn cum_seconds(n: usize, step_s: i64) -> Vec<i64> {
        (0..=n as i64).map(|i| i * step_s).collect()
    }

    /// The forecast participant a live asset built, by its id.
    fn forecast_context<'a>(
        contexts: &'a [Box<dyn AssetMilpContext>],
        id: &str,
    ) -> &'a crate::controller::milp_planner::ForecastMilpContext {
        contexts
            .iter()
            .find(|c| c.asset_id() == id)
            .and_then(|c| c.as_any().downcast_ref())
            .expect("a forecast participant")
    }

    fn contexts_for(
        sim_snap: &SimState,
        n_slots: usize,
        cum_s: &[i64],
        now: DateTime<Utc>,
        pv_forecast_override: Option<f64>,
    ) -> Vec<Box<dyn AssetMilpContext>> {
        build_asset_contexts(
            sim_snap,
            n_slots,
            cum_s,
            now,
            &std::collections::BTreeMap::new(),
            &std::collections::BTreeMap::new(),
            &PlannerParams::default(),
            &terminal_values(),
            &std::collections::BTreeMap::new(),
            &std::collections::HashMap::new(),
            pv_forecast_override,
        )
    }

    /// W3: the live PV plans with its own forecast, one value per slot, the same
    /// `uncurtailed_power_kw_at` its dispatch schedule uses (what `resolve_pv_forecast_kw`
    /// used to compute outside it).
    #[test]
    fn the_pv_plans_with_its_own_uncurtailed_forecast() {
        use crate::entities::asset_params::{AssetParams, PvParams};

        let now = Utc.with_ymd_and_hms(2026, 7, 20, 6, 0, 0).unwrap(); // sunrise-ish, varies over horizon
        let params = vec![AssetParams::Pv(PvParams {
            id: crate::ids::ASSET_PV.to_string(),
            ..Default::default()
        })];
        let sim_snap = SimState::from_params(&params, now);
        let (_, cfg) = sim_snap.find_asset(crate::ids::ASSET_PV).unwrap();
        let pv = cfg.as_any().downcast_ref::<PvInverter>().unwrap().clone();
        let n_slots = 4;
        let cum_s = cum_seconds(n_slots, 3600); // hourly steps, spans sunrise to noon

        let contexts = contexts_for(&sim_snap, n_slots, &cum_s, now, None);
        let got = &forecast_context(&contexts, crate::ids::ASSET_PV).forecast_kw;
        assert_eq!(got.len(), n_slots);
        for (i, &kw) in got.iter().enumerate() {
            let s = cum_s[i];
            let expected =
                -pv.uncurtailed_power_kw_at(now + chrono::Duration::seconds(s), s as f64);
            assert!(
                (kw - expected).abs() < 1e-9,
                "slot {i}: got {kw}, expected {expected}"
            );
        }
        // Sanity: the horizon crosses sunrise->noon, so it must not be a flat series —
        // otherwise this test wouldn't actually be exercising uncurtailed_power_kw_at's
        // time-varying behavior.
        assert!(
            got.iter().any(|&v| (v - got[0]).abs() > 1e-6),
            "expected genuine time-of-day variation across the horizon, got {got:?}"
        );
    }

    /// The test pin (sim inject `pv_plan_kw`) wins over the PV's own forecast, floored at 0.
    #[test]
    fn the_pv_forecast_pin_wins_and_is_floored_at_zero() {
        use crate::entities::asset_params::{AssetParams, PvParams};
        let now = Utc.with_ymd_and_hms(2026, 7, 20, 12, 0, 0).unwrap();
        let params = vec![AssetParams::Pv(PvParams {
            id: crate::ids::ASSET_PV.to_string(),
            ..Default::default()
        })];
        let sim_snap = SimState::from_params(&params, now);
        let cum_s = cum_seconds(3, 600);
        let pinned = contexts_for(&sim_snap, 3, &cum_s, now, Some(2.5));
        assert_eq!(
            forecast_context(&pinned, crate::ids::ASSET_PV).forecast_kw,
            vec![2.5; 3]
        );
        let negative = contexts_for(&sim_snap, 3, &cum_s, now, Some(-1.0));
        assert_eq!(
            forecast_context(&negative, crate::ids::ASSET_PV).forecast_kw,
            vec![0.0; 3]
        );
    }

    /// A site without PV or base load has no forecast participant at all.
    #[test]
    fn no_live_pv_or_base_load_means_no_forecast_participant() {
        let now = Utc::now();
        let sim_snap = SimState::from_params(&[], now);
        let cum_s = cum_seconds(4, 600);
        assert!(contexts_for(&sim_snap, 4, &cum_s, now, None).is_empty());
    }

    /// W3: the live base load plans with its own heuristic-aware forecast
    /// (`forecast_kw_at`), one value per slot.
    #[test]
    fn the_base_load_plans_with_its_own_forecast() {
        use crate::assets::BaseLoad;
        use crate::entities::asset_params::{AssetParams, BaseLoadParams};
        use crate::entities::design_vocabulary::AssetHeuristics;

        let now = Utc.with_ymd_and_hms(2026, 7, 20, 8, 0, 0).unwrap(); // Monday 08:00
        let params = vec![AssetParams::BaseLoad(BaseLoadParams {
            id: crate::ids::ASSET_BASE_LOAD.to_string(),
            baseline_kw: 0.5,
            spikes: vec![],
        })];
        let mut sim_snap = SimState::from_params(&params, now);
        let mut monday = vec![0.0; 24];
        monday[8] = 1.0;
        monday[9] = 3.0;
        let daytime_profile_kw: [Vec<f64>; 7] = std::array::from_fn(|i| {
            if i == 0 {
                monday.clone()
            } else {
                vec![0.0; 24]
            }
        });
        let heuristic = AssetHeuristics {
            asset_id: crate::ids::ASSET_BASE_LOAD.to_string(),
            daytime_profile_kw,
            seasonal_factor: 1.0,
            last_updated: None,
            recent_mean_abs_error_kw: None,
        };
        {
            let (_, cfg) = sim_snap
                .find_asset_mut(crate::ids::ASSET_BASE_LOAD)
                .unwrap();
            let bl = cfg.as_any_mut().downcast_mut::<BaseLoad>().unwrap();
            bl.heuristic = Some(heuristic.clone());
        }
        let n_slots = 2;
        let cum_s = cum_seconds(n_slots, 3600); // hour 8, then hour 9

        let contexts = contexts_for(&sim_snap, n_slots, &cum_s, now, None);
        assert_eq!(
            forecast_context(&contexts, crate::ids::ASSET_BASE_LOAD).forecast_kw,
            vec![1.0, 3.0]
        );
    }

    #[test]
    fn build_asset_contexts_returns_one_per_participating_asset() {
        // W3: the base load is a forecast participant now, so it builds a context too.
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
                initial_soc_frac: 0.5,
                round_trip_efficiency: 0.95,
                min_soc_frac: 0.1,
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
            &std::collections::BTreeMap::new(),
            &std::collections::BTreeMap::new(),
            &planner,
            &terminal_values(),
            &std::collections::BTreeMap::new(),
            &std::collections::HashMap::new(),
            None,
        );

        assert_eq!(
            contexts.len(),
            4,
            "every asset takes part, the base load as a forecast"
        );
        let mut got: Vec<(String, &str)> = contexts
            .iter()
            .map(|c| (c.asset_id().to_string(), kind_of(c.as_ref())))
            .collect();
        got.sort_by(|a, b| a.0.cmp(&b.0));
        assert_eq!(
            got,
            vec![
                ("base_load".to_string(), "forecast"),
                ("battery".to_string(), "battery"),
                ("ev".to_string(), "ev"),
                ("heater".to_string(), "heater"),
            ]
        );
    }

    /// W4: each EV is handed only its own queue, so a second EV does not plan for the first
    /// EV's session.
    #[test]
    fn build_asset_contexts_hands_each_ev_only_its_own_sessions() {
        use crate::controller::milp_planner::asset_port::EvMilpContext;
        use crate::entities::asset_params::{AssetParams, EvParams};
        use crate::entities::device_session::{EvSession, EvSessionQueue};

        let now = Utc.with_ymd_and_hms(2026, 7, 20, 6, 0, 0).unwrap();
        let params: Vec<_> = ["ev-a", "ev-b"]
            .iter()
            .map(|id| {
                AssetParams::Ev(EvParams {
                    id: id.to_string(),
                    ..Default::default()
                })
            })
            .collect();
        let sim_snap = SimState::from_params(&params, now);
        let mut queue = EvSessionQueue::default();
        queue
            .insert(EvSession::simulated(
                "ev-a",
                0.8,
                now,
                now + chrono::Duration::hours(3),
                now,
            ))
            .unwrap();
        let sessions = std::collections::BTreeMap::from([("ev-a".to_string(), queue)]);
        let cum_s = cum_seconds(4, 3600);
        let contexts = build_asset_contexts(
            &sim_snap,
            4,
            &cum_s,
            now,
            &sessions,
            &std::collections::BTreeMap::new(),
            &PlannerParams::default(),
            &terminal_values(),
            &std::collections::BTreeMap::new(),
            &std::collections::HashMap::new(),
            None,
        );
        let departure = |id: &str| {
            contexts
                .iter()
                .find(|c| c.asset_id() == id)
                .and_then(|c| c.as_any().downcast_ref::<EvMilpContext>())
                .expect("an EV context")
                .session_departure
        };
        assert_eq!(departure("ev-a"), Some(now + chrono::Duration::hours(3)));
        assert_eq!(
            departure("ev-b"),
            None,
            "the other EV's session is not ev-b's"
        );
    }

    /// R-108 moved the override-versus-default rule out of `services`; this pins what the move
    /// could silently break: a user's comfort curve reaches the planner's EV context, and
    /// without one the EV's own built-in curve does.
    #[test]
    fn build_asset_contexts_prices_the_ev_by_the_users_comfort_override() {
        use crate::entities::asset::ComfortRate;
        use crate::entities::asset_params::{
            AssetParams, EvParams, EvUsageDayParams, EvUsageMode, EvUsageSimParams,
        };

        // A forecast-driven charge (no session): the path where the curve is consulted.
        let now = Utc.with_ymd_and_hms(2026, 7, 20, 0, 0, 0).unwrap();
        let day = EvUsageDayParams {
            leave_time: chrono::NaiveTime::from_hms_opt(8, 0, 0).unwrap(),
            leave_jitter_min: 0.0,
            return_time: chrono::NaiveTime::from_hms_opt(17, 0, 0).unwrap(),
            return_jitter_min: 0.0,
            leave_probability: 1.0,
            soc_drop_pct_mean: 10.0,
            soc_drop_pct_stddev: 0.0,
        };
        let params = vec![AssetParams::Ev(EvParams {
            id: "ev".into(),
            initial_soc_frac: 0.3,
            usage_sim: Some(EvUsageSimParams {
                mode: EvUsageMode::Forecast,
                engage_charge_planning: true,
                weekday: day.clone(),
                weekend: day,
                min_soc_after_drop_pct: 5.0,
            }),
            ..Default::default()
        })];
        let sim_snap = SimState::from_params(&params, now);
        let planner = PlannerParams::default();
        let cum_s = cum_seconds(24, 3600);
        let segments_with = |overrides: &std::collections::HashMap<String, Vec<ComfortRate>>| {
            let contexts = build_asset_contexts(
                &sim_snap,
                24,
                &cum_s,
                now,
                &std::collections::BTreeMap::new(),
                &std::collections::BTreeMap::new(),
                &planner,
                &terminal_values(),
                &std::collections::BTreeMap::new(),
                overrides,
                None,
            );
            let ev = contexts
                .iter()
                .find_map(|c| c.as_any().downcast_ref::<EvMilpContext>())
                .expect("an EV context");
            ev.segments
                .iter()
                .map(|s| s.eur_per_kwh)
                .collect::<Vec<_>>()
        };

        let by_default = segments_with(&std::collections::HashMap::new());
        let override_curve = vec![ComfortRate {
            fill: 1.0,
            max_marginal_price_eur_kwh: 0.77,
            max_marginal_co2: 0.0,
        }];
        let overrides = std::collections::HashMap::from([("ev".to_string(), override_curve)]);
        let by_override = segments_with(&overrides);

        assert!(
            !by_override.is_empty(),
            "the override prices at least one band"
        );
        assert_ne!(
            by_override, by_default,
            "the override changes what the EV is bid"
        );
        assert!(
            by_override.iter().all(|bid| (bid - 0.77).abs() < 1e-6),
            "every band is bid at the override's price, got {by_override:?}"
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
                initial_soc_frac: 0.5,
                round_trip_efficiency: 0.95,
                min_soc_frac: 0.1,
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
            &std::collections::BTreeMap::new(),
            &std::collections::BTreeMap::new(),
            &planner,
            &terminal_values(),
            &std::collections::BTreeMap::new(),
            &std::collections::HashMap::new(),
            None,
        );

        let heater_ctx = contexts
            .iter()
            .find_map(|c| c.as_any().downcast_ref::<HeaterMilpContext>())
            .expect("heater context must be present");
        assert!(
            (heater_ctx.c_terminal_eur_kwh - 0.07).abs() < 1e-9,
            "heater must receive heater_c_terminal_eur_kwh, not battery's value"
        );
    }

    #[test]
    fn build_asset_contexts_reports_the_roster_id_each_context_was_built_for() {
        use crate::entities::asset_params::{AssetParams, BatteryParams, EvParams};

        let now = Utc::now();
        let params = vec![
            AssetParams::Battery(BatteryParams {
                id: "bat-a".into(),
                ..Default::default()
            }),
            AssetParams::Ev(EvParams {
                id: "car-1".into(),
                ..Default::default()
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
            &std::collections::BTreeMap::new(),
            &std::collections::BTreeMap::new(),
            &planner,
            &std::collections::HashMap::new(),
            &std::collections::BTreeMap::new(),
            &std::collections::HashMap::new(),
            None,
        );

        let ids: Vec<&str> = contexts.iter().map(|c| c.asset_id()).collect();
        assert_eq!(ids, vec!["bat-a", "car-1"]);
    }

    /// The EV's minimum charge power is its own, not whichever EV the roster lists first.
    #[test]
    fn build_asset_contexts_gives_the_ev_its_own_min_charge_kw() {
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
            &std::collections::BTreeMap::new(),
            &std::collections::BTreeMap::new(),
            &planner,
            &std::collections::HashMap::new(),
            &std::collections::BTreeMap::new(),
            &std::collections::HashMap::new(),
            None,
        );

        let ev_ctx = contexts[0]
            .as_any()
            .downcast_ref::<EvMilpContext>()
            .expect("an EV context");
        assert!(
            (ev_ctx.p_min_kw - 2.2).abs() < 1e-9,
            "the EV context must carry the EV's own min_charge_kw"
        );
    }
}
