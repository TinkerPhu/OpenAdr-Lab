//! Tick golden: every planning-mode decision of `build_tick_setpoints`, recorded.
//!
//! `openspec/changes/opportunistic-objective` D0 requires that a planning objective decides
//! exactly as before the opportunistic objective existed. This test replays a fixed set of
//! planned-tick scenarios (plan present / absent / expired, EV overlay, deviation correction,
//! an import limit, a DISPATCH_SETPOINT window, comms loss, an alert, a heater comfort inject)
//! through `build_tick_setpoints` and a seeded `SimState`, on a fixed clock, and compares every
//! asset setpoint and every `ArbiterOutcome` field with `tests/golden/setpoints.txt`.
//!
//! The golden was recorded on the code before any opportunistic change. A diff here means a
//! planning-mode decision changed: fix the code, never re-record to make it pass. Re-record
//! (`UPDATE_TICK_GOLDEN=1`) only for a change the user has accepted as a planning-mode change,
//! and say so in the commit. Re-recorded once so far: R-151, the limit pass holding a lagging
//! lever where its command lands (changed `plan_import_limit` and `plan_alert`).

use std::collections::{BTreeMap, HashMap};
use std::fmt::Write as _;

use chrono::{DateTime, Duration, TimeZone, Utc};
use rand::SeedableRng;

use super::context::TickContext;
use crate::controller::arbiter::ArbiterOutcome;
use crate::controller::comms_loss::CommsLossState;
use crate::entities::capacity::{AlertWindow, CapacitySnapshot, DispatchWindow};
use crate::entities::plan::{AssetAllocation, Plan};
use crate::simulator::{SimState, TickInputs};

const GOLDEN_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/src/tasks/sim_tick/tests/golden/setpoints.txt"
);
const TICKS: usize = 30;

pub(super) fn t0() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 7, 14, 12, 0, 0).unwrap()
}

fn profile_params() -> Vec<crate::entities::asset_params::AssetParams> {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/profiles/test.yaml");
    let profile: crate::profile::Profile =
        serde_yaml::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    profile.asset_params()
}

pub(super) fn sim() -> SimState {
    SimState::from_params_seeded(
        &profile_params(),
        t0(),
        rand::rngs::StdRng::seed_from_u64(7),
    )
}

fn alloc(asset_id: &str, power_kw: f64) -> AssetAllocation {
    AssetAllocation {
        asset_id: asset_id.to_string(),
        power_kw,
        surplus_power_kw: 0.0,
        grid_power_kw: power_kw,
        marginal_value: 0.25,
        cost_eur: 0.0,
        co2_g: 0.0,
    }
}

/// A 1 h plan of 15-minute slots from 5 minutes before `t0`, net import 1 kW planned, with
/// the given allocations in every slot.
pub(super) fn plan_with(allocations: &[(&str, f64)]) -> Plan {
    let mut plan =
        crate::services::test_support::plans::flat_plan(900, 4, t0() - Duration::minutes(5));
    for slot in &mut plan.slots {
        slot.allocations = allocations.iter().map(|(id, kw)| alloc(id, *kw)).collect();
        slot.planned_kw_by_asset = allocations
            .iter()
            .map(|(id, kw)| (id.to_string(), *kw))
            .collect();
        slot.net_import_kw = 1.0;
        slot.pv_forecast_kw = 2.0;
        slot.pv_used_kw = 2.0;
    }
    plan
}

/// A plan whose every slot ended before `t0`.
fn expired_plan() -> Plan {
    let mut plan =
        crate::services::test_support::plans::flat_plan(900, 2, t0() - Duration::hours(2));
    for slot in &mut plan.slots {
        slot.allocations = vec![alloc("battery", 2.0)];
    }
    plan
}

pub(super) struct Scenario {
    pub(super) name: &'static str,
    pub(super) plan: Option<Plan>,
    pub(super) deviation: bool,
    pub(super) overlay: bool,
    pub(super) import_limit_kw: Option<f64>,
    pub(super) dispatch_kw: Option<f64>,
    pub(super) comms_loss: bool,
    pub(super) alert: bool,
    pub(super) heater_setpoint_c: Option<f64>,
    /// Applied as the production tick applies it: one-shot state injections before the first
    /// tick, the PV override in every tick's inputs.
    pub(super) inject: crate::entities::sim_inject::SimInjectState,
}

impl Scenario {
    pub(super) fn new(name: &'static str, plan: Option<Plan>) -> Self {
        Self {
            name,
            plan,
            deviation: false,
            overlay: true,
            import_limit_kw: None,
            dispatch_kw: None,
            comms_loss: false,
            alert: false,
            heater_setpoint_c: None,
            inject: Default::default(),
        }
    }
}

fn scenarios() -> Vec<Scenario> {
    let planned = || plan_with(&[("battery", 2.0), ("ev", 3.0), ("heater", 1.5)]);
    let no_ev = || plan_with(&[("battery", -1.0)]);
    vec![
        Scenario::new("plan_overlay_off_arbiter_off", Some(planned())),
        Scenario {
            deviation: true,
            ..Scenario::new("plan_arbiter_on", Some(planned()))
        },
        Scenario::new("plan_without_ev_allocation_overlay_on", Some(no_ev())),
        Scenario {
            overlay: false,
            ..Scenario::new("plan_without_ev_allocation_overlay_off", Some(no_ev()))
        },
        Scenario {
            deviation: true,
            ..Scenario::new("plan_without_ev_allocation_arbiter_on", Some(no_ev()))
        },
        Scenario::new("no_plan_arbiter_off", None),
        Scenario {
            deviation: true,
            ..Scenario::new("no_plan_arbiter_on", None)
        },
        Scenario {
            overlay: false,
            ..Scenario::new("no_plan_overlay_off", None)
        },
        Scenario::new("expired_plan", Some(expired_plan())),
        Scenario {
            import_limit_kw: Some(2.0),
            ..Scenario::new("plan_import_limit", Some(planned()))
        },
        Scenario {
            import_limit_kw: Some(2.0),
            deviation: true,
            ..Scenario::new("plan_import_limit_arbiter_on", Some(planned()))
        },
        plan_import_limit_no_bridge(),
        Scenario {
            import_limit_kw: Some(1.0),
            ..Scenario::new("no_plan_import_limit", None)
        },
        Scenario {
            dispatch_kw: Some(-2.0),
            ..Scenario::new("plan_dispatch_window", Some(planned()))
        },
        Scenario {
            comms_loss: true,
            ..Scenario::new("plan_comms_loss", Some(planned()))
        },
        Scenario {
            alert: true,
            ..Scenario::new("plan_alert", Some(planned()))
        },
        Scenario {
            alert: true,
            deviation: true,
            ..Scenario::new("plan_alert_arbiter_on", Some(planned()))
        },
        Scenario {
            heater_setpoint_c: Some(21.5),
            ..Scenario::new("plan_heater_inject", Some(no_ev()))
        },
        Scenario {
            heater_setpoint_c: Some(21.5),
            ..Scenario::new("no_plan_heater_inject", None)
        },
    ]
}

pub(super) fn context(s: &Scenario, sim: &SimState) -> TickContext {
    let window_end = t0() + Duration::hours(1);
    let inject = crate::entities::sim_inject::SimInjectState {
        heater_setpoint_c: s.heater_setpoint_c,
        ..s.inject.clone()
    };
    let overlay = sim
        .iter_assets()
        .map(|(entry, _)| (entry.id.clone(), s.overlay))
        .collect::<HashMap<_, _>>();
    TickContext {
        inject,
        pv_clear: false,
        base_clear: false,
        feeds: Default::default(),
        grid_max_import_kw: 100.0,
        grid_max_export_kw: 100.0,
        plan_snap: s.plan.clone(),
        capacity_schedule: s
            .import_limit_kw
            .map(|kw| CapacitySnapshot {
                interval_start: t0() - Duration::hours(1),
                interval_end: window_end,
                import_limit_kw: Some(kw),
                export_limit_kw: None,
                import_limit_event_id: Some("evt-limit".into()),
                export_limit_event_id: None,
                ..Default::default()
            })
            .into_iter()
            .collect(),
        dispatch_windows: s
            .dispatch_kw
            .map(|kw| DispatchWindow {
                setpoint_kw: kw,
                start: t0() - Duration::minutes(1),
                end: window_end,
                event_id: "evt-dispatch".into(),
            })
            .into_iter()
            .collect(),
        alert_windows: if s.alert {
            vec![AlertWindow {
                alert_type: "ALERT_GRID_EMERGENCY".into(),
                start: t0() - Duration::minutes(1),
                end: window_end,
                event_id: "evt-alert".into(),
                message: String::new(),
            }]
        } else {
            vec![]
        },
        rates_snap: vec![],
        overlay_enabled: crate::controller::arbiter::OverlayEnabled::from_map(overlay),
        deviation_arbiter_enabled: s.deviation,
        incumbent_lever: None,
        bridge_kw_by_asset: HashMap::new(),
        limit_enforcement_enabled: true,
        limit_incumbent_lever: None,
        ev_departure_by_asset: HashMap::new(),
        opportunistic: None,
        comms_loss: s.comms_loss.then_some(CommsLossState {
            active: true,
            max_power_pct: 0.7,
        }),
    }
}

/// A float at 1e-9: the arbiter sums power over `HashMap`s, whose iteration order varies
/// per process, so the last bit of a sum is not reproducible between runs. A decision is.
fn f(x: f64) -> String {
    let r = format!("{x:.9}");
    if r.trim_start_matches('-')
        .chars()
        .all(|c| c == '0' || c == '.')
    {
        "0.000000000".to_string()
    } else {
        r
    }
}

fn of(x: Option<f64>) -> String {
    x.map_or("None".to_string(), f)
}

fn sorted_kw(m: &HashMap<String, f64>) -> BTreeMap<&str, String> {
    m.iter().map(|(k, v)| (k.as_str(), f(*v))).collect()
}

fn sorted<V: std::fmt::Debug>(m: &HashMap<String, V>) -> BTreeMap<&str, String> {
    m.iter()
        .map(|(k, v)| (k.as_str(), format!("{v:?}")))
        .collect()
}

fn dump(out: &mut String, tick: usize, o: &ArbiterOutcome) {
    let _ = writeln!(out, "  tick {tick}");
    let _ = writeln!(out, "    setpoints {:?}", sorted_kw(&o.setpoints));
    let _ = writeln!(
        out,
        "    heater_emergency_mode {:?}",
        sorted(&o.heater_emergency_mode)
    );
    let _ = writeln!(
        out,
        "    pv_tighten {}",
        of(o.pv_generation_limit_tighten_kw)
    );
    let _ = writeln!(
        out,
        "    displaced {:?}",
        sorted_kw(&o.displaced_kw_by_asset)
    );
    let _ = writeln!(out, "    active_lever {:?}", o.active_lever);
    let _ = writeln!(
        out,
        "    net {} dev {} dev_without {} unresolved {}",
        of(o.net_kw),
        of(o.dev_kw),
        of(o.dev_without_correction_kw),
        f(o.unresolved_kw)
    );
    let _ = writeln!(out, "    bridge {:?}", sorted_kw(&o.bridge_kw_by_asset));
    match &o.limit {
        None => {
            let _ = writeln!(out, "    limit None");
        }
        Some(l) => {
            let _ = writeln!(
                out,
                "    limit target {} excess {} unresolved {} lever {:?} modes {:?} adjusted {:?} bridge {:?}",
                f(l.target_kw),
                f(l.excess_kw),
                f(l.unresolved_kw),
                l.active_lever,
                sorted(&l.heater_emergency_mode),
                sorted_kw(&l.adjusted_kw_by_asset),
                sorted_kw(&l.bridge_kw_by_asset),
            );
        }
    }
}

/// One scenario, `TICKS` ticks: the tick's own sequence (snapshot, live previews, thermostat
/// setpoints, `build_tick_setpoints`), then physics with the chosen setpoints, then the
/// carried arbiter state the production glue would store. `each` sees every tick's outcome and
/// the simulator after that tick's physics.
pub(super) fn replay(s: &Scenario, mut each: impl FnMut(usize, &ArbiterOutcome, &SimState)) {
    let mut sim = sim();
    let mut ctx = context(s, &sim);
    crate::simulator::inject::apply_state_injections(&ctx.inject, &mut sim);
    for tick in 0..TICKS {
        let now = t0() + Duration::seconds(tick as i64);
        let snap = sim.to_sim_snapshot();
        let (live, released) = super::feeds::live_previews(&sim, &ctx, &snap, now, 1.0);
        let thermostat = sim.thermostat_setpoints_kw(ctx.inject.heater_setpoint_c);
        let o = super::setpoints::build_tick_setpoints(
            &ctx,
            &snap,
            &thermostat,
            now,
            (&live, &released),
        );
        sim.tick(TickInputs {
            pv_irradiance_override: ctx.inject.pv_irradiance,
            ..TickInputs::new(1.0, now, o.setpoints.clone())
        });
        each(tick, &o, &sim);
        ctx.incumbent_lever = o.active_lever.clone();
        ctx.limit_incumbent_lever = o.limit.as_ref().and_then(|l| l.active_lever.clone());
        ctx.bridge_kw_by_asset = super::arbiter_glue::both_passes_bridge_kw(&o);
    }
}

fn run(s: &Scenario) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "scenario {}", s.name);
    replay(s, |tick, o, _| dump(&mut out, tick, o));
    out
}

/// The planned EV command alone is above the 2 kW import limit, and nothing can cover the tick
/// a lagging command takes to land: no PV, the battery at its floor (R-151).
pub(super) fn plan_import_limit_no_bridge() -> Scenario {
    Scenario {
        import_limit_kw: Some(2.0),
        inject: crate::entities::sim_inject::SimInjectState {
            pv_irradiance: Some(0.0),
            battery_soc: Some(0.1),
            ..Default::default()
        },
        ..Scenario::new(
            "plan_import_limit_no_bridge",
            Some(plan_with(&[("ev", 3.0), ("heater", 1.5)])),
        )
    }
}

#[test]
fn planning_mode_tick_decisions_match_the_golden() {
    let actual: String = scenarios().iter().map(run).collect();
    if std::env::var("UPDATE_TICK_GOLDEN").is_ok() {
        std::fs::create_dir_all(std::path::Path::new(GOLDEN_PATH).parent().unwrap()).unwrap();
        std::fs::write(GOLDEN_PATH, &actual).unwrap();
        return;
    }
    let expected = std::fs::read_to_string(GOLDEN_PATH)
        .expect("golden missing: record it with UPDATE_TICK_GOLDEN=1 on unmodified code");
    let expected = expected.replace("\r\n", "\n");
    if actual != expected {
        let first = actual
            .lines()
            .zip(expected.lines())
            .enumerate()
            .find(|(_, (a, e))| a != e);
        panic!(
            "planning-mode tick decisions changed (openspec opportunistic-objective D0); first \
             difference at line {:?}",
            first.map(|(i, (a, e))| format!("{}: got `{a}`, golden `{e}`", i + 1))
        );
    }
}

/// The golden must exercise what it claims to: each named regime has to have acted at least
/// once, or a scenario silently stopped covering it.
#[test]
fn tick_golden_scenarios_exercise_each_regime() {
    let all: String = scenarios().iter().map(run).collect();
    for needle in ["limit target", "active_lever Some(", "lever Some("] {
        assert!(all.contains(needle), "no scenario produced `{needle}`");
    }
    assert!(
        all.lines().any(|l| l.starts_with("    limit target")),
        "the import-limit scenarios never ran the limit pass"
    );
}
