//! Phase-2 smoothing benches.
//!
//! `bench_ven1_spikes`: reproduction of ven-1's live instance of 2026-10-03T10:40Z
//! (battery + EV + PV), whose plan showed one-slot battery dropouts/spikes at 14:10
//! and 16:55 local. Measures which planner knob removes them.
//!
//! `bench_fleet_startup_penalty`: the same question for every real fleet profile.
//!
//!   wsl bash -lc "cd <VEN> && cargo test -p ven-app --release <bench> -- --ignored --nocapture"

use super::*;
use std::time::Instant;

mod data {
    include!("ven1_spikes_data.rs");
}
#[allow(clippy::approx_constant)] // captured PV forecast: 3.1416 kW is data, not pi
mod frag {
    include!("ven1_ev_frag_data.rs");
}
#[allow(clippy::approx_constant)] // captured PV forecast, not pi
mod frag5 {
    include!("ven5_heater_frag_data.rs");
}
use data::SLOTS;

/// One captured slot: (pv_kw, base_kw, import_eur_kwh, export_eur_kwh, co2_g_kwh).
type Slot = (f64, f64, f64, f64, f64);

fn ven1_now() -> DateTime<Utc> {
    use chrono::TimeZone;
    Utc.with_ymd_and_hms(2026, 10, 3, 10, 40, 0).unwrap()
}

fn ven1_profile() -> Profile {
    use crate::entities::asset_params::{EvUsageDayParams, EvUsageMode, EvUsageSimParams};
    let t = |h, m| chrono::NaiveTime::from_hms_opt(h, m, 0).unwrap();
    // Live: next trip Mon 06:56Z..17:24Z, 22 % drop; no trip on the weekend days.
    let weekday = EvUsageDayParams {
        leave_time: t(6, 56),
        leave_jitter_min: 0.0,
        return_time: t(17, 24),
        return_jitter_min: 0.0,
        leave_probability: 1.0,
        soc_drop_pct_mean: 22.0,
        soc_drop_pct_stddev: 0.0,
    };
    let weekend = EvUsageDayParams {
        leave_probability: 0.0,
        ..weekday.clone()
    };
    Profile {
        assets: vec![
            AssetProfile::Ev(EvParams {
                id: "ev".into(),
                max_charge_kw: 11.0,
                max_discharge_kw: 0.0,
                initial_soc: 0.745,
                battery_kwh: 60.0,
                consumption_kwh_per_km: 0.18,
                soc_target: 0.80,
                default_charge_kw: 0.0,
                min_charge_kw: 1.4,
                response_delay_s: 10.0,
                v2g_capable: false,
                usage_sim: Some(EvUsageSimParams {
                    mode: EvUsageMode::Forecast,
                    engage_charge_planning: true,
                    weekday,
                    weekend,
                    min_soc_after_drop_pct: 5.0,
                }),
            }),
            AssetProfile::Pv(PvParams {
                id: "pv".into(),
                rated_kw: 14.4,
                inverter_max_kw: 12.5,
                co2_g_kwh: 0.0,
            }),
            AssetProfile::Battery(BatteryParams {
                id: "battery".into(),
                capacity_kwh: 10.0,
                max_charge_kw: 5.0,
                max_discharge_kw: 5.0,
                initial_soc: 0.17,
                round_trip_efficiency: 0.92,
                min_soc: 0.10,
                c_terminal_eur_kwh: None,
            }),
            AssetProfile::BaseLoad(BaseLoadParams {
                id: "base_load".into(),
                baseline_kw: 0.4,
                spikes: vec![],
            }),
        ],
        simulator: SimulatorConfig,
        planner: PlannerConfig {
            plan_step_s: 300,
            plan_horizon_h: 48,
            plan_zones: vec![
                crate::entities::plan::PlanZone {
                    step_s: 300,
                    slots: 96,
                },
                crate::entities::plan::PlanZone {
                    step_s: 600,
                    slots: 96,
                },
                crate::entities::plan::PlanZone {
                    step_s: 900,
                    slots: 96,
                },
            ],
            ..PlannerConfig::default()
        },
        grid: GridConfig {
            max_import_kw: 12.5,
            max_export_kw: 10.0,
        },
        packets: vec![],
    }
}

/// ven-1's live tariffs of 2026-10-03T10:40Z, one snapshot per plan slot.
fn ven1_tariffs(profile: &Profile) -> TariffTimeSeries {
    tariffs_from(profile, ven1_now(), &SLOTS)
}

/// A captured plan's tariffs, one snapshot per plan slot.
fn tariffs_from(profile: &Profile, now: DateTime<Utc>, slots: &[Slot]) -> TariffTimeSeries {
    let steps: Vec<i64> = profile
        .planner
        .plan_zones
        .iter()
        .flat_map(|z| std::iter::repeat_n(z.step_s as i64, z.slots))
        .collect();
    let mut start = now;
    let mut snaps = Vec::new();
    for (i, &(_, _, imp, exp, co2)) in slots.iter().enumerate() {
        let end = start + chrono::Duration::seconds(steps[i]);
        snaps.push(TariffSnapshot {
            interval_start: start,
            interval_end: end,
            import_tariff_eur_kwh: Some(imp),
            export_tariff_eur_kwh: Some(exp),
            co2_g_kwh: Some(co2),
        });
        start = end;
    }
    TariffTimeSeries::from_snapshots(&snaps)
}

struct Instance {
    inputs: MilpInputs,
    ctxs: Vec<Box<dyn crate::controller::milp_planner::AssetMilpContext>>,
    profile: Profile,
}

fn ven1_instance() -> Instance {
    instance_from(ven1_profile(), ven1_now(), &SLOTS)
}

/// A captured plan replayed: the profile's assets and SoCs, the plan's own PV and
/// base-load forecast and tariffs.
fn instance_from(profile: Profile, now: DateTime<Utc>, slots: &[Slot]) -> Instance {
    let sim = make_snap_from_profile(&profile);
    let tariffs = tariffs_from(&profile, now, slots);
    let cap = no_capacity();
    let mut ctxs = build_asset_contexts(&profile, &sim, now, None, None, &tariffs);
    let mut inputs = build_milp_inputs(&ctxs, &tariffs, &cap, &profile, now, &[], None);
    assert_eq!(inputs.n, 288);
    for (i, &(pv, base, _, _, _)) in slots.iter().enumerate() {
        inputs.p_pv_kw[i] = pv;
        inputs.p_base_kw[i] = base;
    }
    for ctx in ctxs.iter_mut() {
        ctx.inject_grid_slots(&inputs.c_imp_eur_kwh, &inputs.p_pv_kw, &inputs.p_base_kw);
    }
    Instance {
        inputs,
        ctxs,
        profile,
    }
}

/// Slots 0..96 are the 5-min zone (8 h), where the spikes sit.
const NEAR: usize = 96;

/// Run starts and one-slot blips of an on/off signal over `slots`.
#[derive(Default, Clone, Copy)]
struct Flips {
    starts: usize,
    isolated: usize,
}

fn flips(on: &dyn Fn(usize) -> bool, slots: usize) -> Flips {
    let mut f = Flips::default();
    for t in 1..slots {
        if on(t) && !on(t - 1) {
            f.starts += 1;
        }
        // A one-slot blip: differs from both neighbours, which agree with each other.
        if t + 1 < slots && on(t - 1) == on(t + 1) && on(t) != on(t - 1) {
            f.isolated += 1;
        }
    }
    f
}

/// Battery, EV and heater fragmentation over the 8 h 5-min zone, plus the
/// battery's summed |delta net power| [kW].
fn shape(s: &SolveOutput, n: usize) -> (Flips, Flips, Flips, f64) {
    let slots = NEAR.min(n);
    let at = |v: &Vec<f64>, t: usize| v.get(t).copied().unwrap_or(0.0);
    let net = |t: usize| at(&s.p_bat_ch_kw, t) - at(&s.p_bat_dis_kw, t);
    let bat = flips(
        &|t| at(&s.p_bat_ch_kw, t) + at(&s.p_bat_dis_kw, t) > 0.05,
        slots,
    );
    let ev = flips(&|t| at(&s.p_ev_kw, t) > 0.05, slots);
    let htr = flips(&|t| at(&s.y_heat, t) > 0.5, slots);
    let ramp_kw = (1..slots).map(|t| (net(t) - net(t - 1)).abs()).sum();
    (bat, ev, htr, ramp_kw)
}

fn fmt_shape((b, e, h, r): (Flips, Flips, Flips, f64)) -> String {
    format!(
        "bat {:>2}/{:<2} ev {:>2}/{:<2} htr {:>2}/{:<2} ramp {:>5.1}",
        b.starts, b.isolated, e.starts, e.isolated, h.starts, h.isolated, r
    )
}

fn report(label: &str, s: &SolveOutput, n: usize, secs: f64, extra: &str) {
    println!(
        "  {label:<30} {secs:>6.1}s {:>10}  {}  {extra}",
        format!("{:?}", s.status),
        fmt_shape(shape(s, n))
    );
    // Slots 14..54 = 13:50..17:10 local, one column per 5-min slot.
    let row = |name: &str, f: &dyn Fn(usize) -> f64| {
        let cells: String = (14..54).map(|t| format!("{:>4.1}", f(t))).collect();
        println!("      {name:>4} {cells}");
    };
    row("bat", &|t| s.p_bat_ch_kw[t] - s.p_bat_dis_kw[t]);
    row("ev", &|t| s.p_ev_kw[t]);
}

#[test]
#[ignore = "ven-1 spike reproduction: ~10 full solves, run with --ignored --nocapture"]
fn bench_ven1_spikes() {
    let Instance {
        inputs,
        ctxs,
        profile,
    } = ven1_instance();
    let p1w = build_phase1_weights(&profile, PlannerObjective::MinCost);
    let p2w = build_phase2_weights(&inputs, &profile.planner);
    println!("\n-- ven-1 2026-10-03T10:40Z: battery spikes at 14:10 / 16:55 local --\n");

    let t = Instant::now();
    let p1 = solve_phase1(&inputs, &p1w, &ctxs, 60.0).expect("phase 1 feasible");
    report(
        "phase 1 (gap 0.02)",
        &p1,
        inputs.n,
        t.elapsed().as_secs_f64(),
        &format!("obj {:.4}", p1.objective_eur),
    );

    let run_p2 = |label: &str, inp: &MilpInputs, w: &Phase2Weights, eps: f64, budget: f64| {
        let t = Instant::now();
        match solve_phase2(inp, &p1w, w, p1.objective_eur, eps, &p1, &ctxs, budget) {
            Ok((s, fr)) => report(
                label,
                &s,
                inp.n,
                t.elapsed().as_secs_f64(),
                &format!("friction {fr:.4}"),
            ),
            Err(e) => println!("  {label:<30} ERR {e}"),
        }
    };

    run_p2("p2 prod (eps .02, 15 s)", &inputs, &p2w, 0.02, 15.0);
    run_p2("p2 budget 60 s", &inputs, &p2w, 0.02, 60.0);
    run_p2("p2 budget 180 s", &inputs, &p2w, 0.02, 180.0);
    let mut w = p2w.clone();
    w.c_bat_startup_eur *= 10.0;
    w.c_ev_startup_eur *= 10.0;
    run_p2("p2 startup x10, 15 s", &inputs, &w, 0.02, 15.0);
    let mut w = p2w.clone();
    w.c_bat_ramp_eur_kw *= 10.0;
    w.c_ev_ramp_eur_kw *= 10.0;
    run_p2("p2 ramp x10, 15 s", &inputs, &w, 0.02, 15.0);
    let mut loose = inputs.clone();
    loose.mip_gap_target = 0.20;
    run_p2("p2 own gap 0.20, 15 s", &loose, &p2w, 0.02, 15.0);

    // Does a tighter phase 1 leave nothing for phase 2 to clean up?
    for gap in [0.005, 0.001] {
        let mut tight = inputs.clone();
        tight.mip_gap_target = gap;
        let t = Instant::now();
        let p1t = solve_phase1(&tight, &p1w, &ctxs, 60.0).expect("phase 1 feasible");
        report(
            &format!("phase 1 (gap {gap})"),
            &p1t,
            inputs.n,
            t.elapsed().as_secs_f64(),
            &format!("obj {:.4}", p1t.objective_eur),
        );
    }
}

/// Load a real fleet profile through the same translation `main.rs` uses.
fn fleet_profile(name: &str) -> Profile {
    let yaml = std::fs::read_to_string(format!("profiles/{name}.yaml")).expect("profile");
    let real: crate::profile::Profile = serde_yaml::from_str(&yaml).expect("parse");
    let (_, planner, assets) = crate::domain_params::build_domain_params(&real);
    Profile {
        assets,
        simulator: SimulatorConfig,
        planner,
        grid: GridConfig {
            max_import_kw: real.grid.max_import_kw,
            max_export_kw: real.grid.max_export_kw,
        },
        packets: vec![],
    }
}

/// Is a 10x phase-2 startup penalty (0.10 EUR) safe and useful for every VEN, or
/// only for some asset mixes? Each real profile, ven-1's live tariffs, the
/// production 15 s phase-2 budget, default 0.01 vs 0.10 on both battery and EV.
/// `FLEET_ONLY=ven-1,ven-5` restricts the sweep.
#[test]
#[ignore = "fleet sweep: 20 profiles x (phase 1 + 2 phase-2 solves), ~25 min"]
fn bench_fleet_startup_penalty() {
    let now = ven1_now();
    println!("\n-- phase-2 startup penalty 0.01 vs 0.10, every fleet profile (15 s budget) --");
    println!("   first 8 h: starts/one-slot blips per asset; ramp = battery sum |d net| kW\n");
    let only = std::env::var("FLEET_ONLY").ok();
    for i in 1..=20 {
        let name = format!("ven-{i}");
        if let Some(o) = &only {
            if !o.split(',').any(|x| x == name) {
                continue;
            }
        }
        let mut profile = fleet_profile(&name);
        // Measure against the fleet default, whatever the profile now says.
        profile.planner.c_bat_startup_eur = 0.01;
        profile.planner.c_ev_startup_eur = 0.01;
        let kinds: Vec<&str> = profile
            .assets
            .iter()
            .filter_map(|a| match a {
                AssetProfile::Battery(_) => Some("bat"),
                AssetProfile::Ev(_) => Some("ev"),
                AssetProfile::Heater(_) => Some("htr"),
                AssetProfile::Pv(_) => Some("pv"),
                _ => None,
            })
            .collect();
        let kinds = kinds.join("+");
        let sim = make_snap_from_profile(&profile);
        let tariffs = ven1_tariffs(&profile);
        let cap = no_capacity();
        let mut ctxs = build_asset_contexts(&profile, &sim, now, None, None, &tariffs);
        let inputs = build_milp_inputs(&ctxs, &tariffs, &cap, &profile, now, &[], None);
        for ctx in ctxs.iter_mut() {
            ctx.inject_grid_slots(&inputs.c_imp_eur_kwh, &inputs.p_pv_kw, &inputs.p_base_kw);
        }
        let p1w = build_phase1_weights(&profile, PlannerObjective::MinCost);
        let eps = profile.planner.phase2_epsilon_eur;
        let gap = profile.planner.mip_gap_target;
        let t = Instant::now();
        let p1 = match solve_phase1(&inputs, &p1w, &ctxs, 60.0) {
            Ok(p) => p,
            Err(e) => {
                println!("  {name:<6} [{kinds}] phase 1 ERR {e}");
                continue;
            }
        };
        let p1_s = t.elapsed().as_secs_f64();
        println!(
            "  {name:<6} [{kinds}] gap {gap} eps {eps} | p1 {p1_s:>5.1}s {:?}",
            p1.status
        );
        println!("      {:<10}{}", "p1", fmt_shape(shape(&p1, inputs.n)));
        let mut rec = vec![];
        for startup in [0.01, 0.10] {
            let mut planner = profile.planner.clone();
            planner.c_bat_startup_eur = startup;
            planner.c_ev_startup_eur = startup;
            let p2w = build_phase2_weights(&inputs, &planner);
            let t = Instant::now();
            let r = solve_phase2(&inputs, &p1w, &p2w, p1.objective_eur, eps, &p1, &ctxs, 15.0);
            let secs = t.elapsed().as_secs_f64();
            let (s, fr) = match r {
                Ok(v) => v,
                Err(e) => {
                    println!("      p2 {startup}  ERR {e}");
                    continue;
                }
            };
            let sh = shape(&s, inputs.n);
            // First 25 min = what reaches the hardware before the next replan.
            let at = |v: &Vec<f64>, t: usize| v.get(t).copied().unwrap_or(0.0);
            let bat25: f64 = (0..5)
                .map(|t| (at(&s.p_bat_ch_kw, t) - at(&s.p_bat_dis_kw, t)) * inputs.dt_h[t])
                .sum();
            let ev25: f64 = (0..5).map(|t| at(&s.p_ev_kw, t) * inputs.dt_h[t]).sum();
            let status = format!("{:?}", s.status);
            println!(
                "      {:<10}{}  {secs:>5.1}s {status:<9} friction {fr:>8.4} | 25min bat {bat25:>6.3} ev {ev25:>6.3} kWh",
                format!("p2 {startup}"),
                fmt_shape(sh),
            );
            rec.push(format!(
                "\"s{}\":{{\"secs\":{secs:.2},\"status\":\"{status}\",\"bat_starts\":{},\"bat_isolated\":{},\"ev_starts\":{},\"ev_isolated\":{},\"htr_starts\":{},\"htr_isolated\":{},\"bat_ramp_kw\":{:.2},\"bat25_kwh\":{bat25:.4},\"ev25_kwh\":{ev25:.4}}}",
                (startup * 100.0_f64).round() as i64,
                sh.0.starts,
                sh.0.isolated,
                sh.1.starts,
                sh.1.isolated,
                sh.2.starts,
                sh.2.isolated,
                sh.3
            ));
        }
        println!(
            "@@RESULT {{\"bench\":\"bench_fleet_startup_penalty\",\"params\":{{\"ven\":\"{name}\",\"assets\":\"{kinds}\",\"mip_gap\":{gap},\"epsilon\":{eps},\"phase2_budget_s\":15}},\"results\":{{\"p1_s\":{p1_s:.2},{}}}}}",
            rec.join(",")
        );
    }
}

/// ven-1's plan of 2026-10-05T19:40Z: EV charging scattered into one-slot 1.4 kW runs,
/// the battery discharging straight into them at night, phase 2 at its 15 s limit
/// every cycle since 051 (multi-trip usage forecast) went live. The real profile,
/// with the plan's SoCs.
fn ven1_frag_instance() -> Instance {
    use chrono::TimeZone;
    let now = Utc.with_ymd_and_hms(2026, 10, 5, 19, 40, 0).unwrap();
    captured_instance(
        "ven-1",
        now,
        &LiveState {
            battery_soc: 0.8776,
            ev_soc: 0.5788,
            heater_temp_c: None,
        },
        &frag::SLOTS,
    )
}

/// ven-5's plan of 2026-10-06T07:25Z: heater + battery + EV (away) + PV, phase-1 gap
/// 0.30. The heater ran single 5-minute slots through the PV hours and the battery
/// mirrored each one; phase 2 at its 15 s limit, friction 4.74 EUR.
fn ven5_frag_instance() -> Instance {
    use chrono::TimeZone;
    let now = Utc.with_ymd_and_hms(2026, 10, 6, 7, 25, 0).unwrap();
    captured_instance(
        "ven-5",
        now,
        &LiveState {
            battery_soc: 0.1343,
            ev_soc: 0.7859,
            heater_temp_c: Some(47.76),
        },
        &frag5::SLOTS,
    )
}

/// The state a captured plan started from.
struct LiveState {
    battery_soc: f64,
    ev_soc: f64,
    heater_temp_c: Option<f64>,
}

/// A live plan replayed: the real profile, the plan's starting state, its own PV and
/// base-load forecast and tariffs.
fn captured_instance(
    name: &str,
    now: DateTime<Utc>,
    state: &LiveState,
    slots: &[Slot],
) -> Instance {
    let mut profile = fleet_profile(name);
    for asset in profile.assets.iter_mut() {
        match asset {
            AssetProfile::Battery(b) => b.initial_soc = state.battery_soc,
            AssetProfile::Ev(e) => e.initial_soc = state.ev_soc,
            AssetProfile::Heater(h) => {
                if let Some(t) = state.heater_temp_c {
                    h.temp_initial_c = t;
                }
            }
            _ => {}
        }
    }
    instance_from(profile, now, slots)
}

/// EV runs over the whole horizon (the live plan's fragmentation spans all of it).
fn ev_runs(s: &SolveOutput, n: usize) -> Flips {
    flips(&|t| s.p_ev_kw.get(t).copied().unwrap_or(0.0) > 0.05, n)
}

/// Each EV run as `start-slot+len@kW(bat kW)`, so a blip can be located and its
/// battery partner seen.
fn ev_run_list(s: &SolveOutput, n: usize) -> String {
    let on = |t: usize| s.p_ev_kw.get(t).copied().unwrap_or(0.0) > 0.05;
    let mut out = Vec::new();
    let mut t = 0;
    while t < n {
        if on(t) {
            let start = t;
            while t < n && on(t) {
                t += 1;
            }
            let bat = s.p_bat_ch_kw[start] - s.p_bat_dis_kw[start];
            out.push(format!(
                "{start}+{}@{:.1}({bat:.1})",
                t - start,
                s.p_ev_kw[start]
            ));
        } else {
            t += 1;
        }
    }
    out.join(" ")
}

/// One planner setting to measure on a captured instance.
struct Variant {
    name: &'static str,
    malus_eur_kwh: Option<f64>,
    startup_eur: Option<f64>,
    budget_s: f64,
}

/// ven-5's heater + battery fragmentation (R-97): the heater pulses single slots
/// through the PV hours and the battery mirrors each pulse. Unlike ven-1's EV this is
/// not a cost tie: a heater tie-breaker (1e-4, 1e-3) and a tight phase-1 gap left it at
/// ~30 runs; phase 2 halves it only with a 2 EUR cost allowance and 60 s, at +0.7 EUR.
/// Knobs: phase-1 gap, phase-2 budget, `BENCH_EPS` for the cost allowance.
/// `BENCH_ONLY=a,b` restricts.
#[test]
#[ignore = "ven-5 heater fragmentation matrix, ~3 min"]
fn bench_ven5_heater_fragmentation() {
    let variants: [(&str, Option<f64>, f64); 3] = [
        ("prod", None, 15.0),
        ("prod 60s", None, 60.0),
        ("gap .02", Some(0.02), 15.0),
    ];
    let only = std::env::var("BENCH_ONLY").ok();
    let Instance {
        inputs,
        ctxs,
        profile,
    } = ven5_frag_instance();
    let mismatched = (0..inputs.n)
        .filter(|&t| inputs.a_ev[t] != frag5::PLUGGED[t])
        .count();
    println!(
        "
-- ven-5 2026-10-06T07:25Z | plugged mask differs from live in {mismatched} slots"
    );
    for (name, gap, budget) in variants {
        if let Some(o) = &only {
            if !o.split(',').any(|x| x.trim() == name) {
                continue;
            }
        }
        let mut inp = inputs.clone();
        if let Some(g) = gap {
            inp.mip_gap_target = g;
        }
        let p1w = build_phase1_weights(&profile, PlannerObjective::MinCost);
        let p2w = build_phase2_weights(&inp, &profile.planner);
        let t = Instant::now();
        let p1 = match solve_phase1(&inp, &p1w, &ctxs, 60.0) {
            Ok(p) => p,
            Err(e) => {
                println!("  {name:<22} phase 1 ERR {e}");
                continue;
            }
        };
        println!(
            "  {name:<22} p1 {:>5.1}s {:<10} {}",
            t.elapsed().as_secs_f64(),
            format!("{:?}", p1.status),
            variant_row(&inp, &p1)
        );
        let t = Instant::now();
        let eps = std::env::var("BENCH_EPS")
            .ok()
            .and_then(|e| e.parse().ok())
            .unwrap_or(profile.planner.phase2_epsilon_eur);
        match solve_phase2(&inp, &p1w, &p2w, p1.objective_eur, eps, &p1, &ctxs, budget) {
            Ok((s, fr)) => println!(
                "  {:<22} p2 eps {eps} {:>5.1}s {:<10} {}  friction {fr:.3}",
                "",
                t.elapsed().as_secs_f64(),
                format!("{:?}", s.status),
                variant_row(&inp, &s)
            ),
            Err(e) => println!("  {:<22} p2 ERR {e}", ""),
        }
    }
}

/// What a plan costs in money over the horizon, ignoring every modelling term:
/// grid import minus export at the tariffs.
fn grid_eur(inputs: &MilpInputs, s: &SolveOutput) -> f64 {
    (0..inputs.n)
        .map(|t| {
            (s.p_imp_kw[t] * inputs.c_imp_eur_kwh[t] - s.p_exp_kw[t] * inputs.c_exp_eur_kwh[t])
                * inputs.dt_h[t]
        })
        .sum()
}

fn variant_row(inputs: &MilpInputs, s: &SolveOutput) -> String {
    let n = inputs.n;
    let ev = ev_runs(s, n);
    let bat = flips(&|t| s.p_bat_ch_kw[t] + s.p_bat_dis_kw[t] > 0.05, n);
    let htr = flips(&|t| s.y_heat.get(t).copied().unwrap_or(0.0) > 0.5, n);
    let kwh = |v: &Vec<f64>| -> f64 { (0..n).map(|t| v[t] * inputs.dt_h[t]).sum() };
    format!(
        "ev {:>2}/{:<2} bat {:>2}/{:<2} htr {:>2}/{:<2} | grid {:>7.3} EUR imp {:>5.1} exp {:>5.1} kWh  ev {:>5.1} kWh  bat end {:>4.1} kWh",
        ev.starts,
        ev.isolated,
        bat.starts,
        bat.isolated,
        htr.starts,
        htr.isolated,
        grid_eur(inputs, s),
        kwh(&s.p_imp_kw),
        kwh(&s.p_exp_kw),
        kwh(&s.p_ev_kw),
        s.e_bat_kwh.last().copied().unwrap_or(0.0),
    )
}

/// The live regression: on ven-1's plan of 2026-10-05T19:40Z the battery fed the EV
/// at its 1.4 kW minimum in 13 scattered runs, 14 of them single slots. Routing PV
/// through the battery into the EV is what the import malus asks for; *when* it
/// happens was a cost tie HiGHS broke arbitrarily, and phase 2 had no time to merge
/// it. Phase 1 must already deliver runs, not single slots — whatever phase 2 manages.
#[test]
fn ven1_live_instance_charges_the_ev_in_runs_not_single_slots() {
    let Instance {
        inputs,
        ctxs,
        profile,
    } = ven1_frag_instance();
    let p1w = build_phase1_weights(&profile, PlannerObjective::MinCost);
    let p1 = solve_phase1(&inputs, &p1w, &ctxs, 60.0).expect("phase 1 feasible");
    let runs = ev_runs(&p1, inputs.n);
    assert_eq!(
        runs.isolated,
        0,
        "single-slot EV charging in phase 1: {}",
        ev_run_list(&p1, inputs.n)
    );
    assert!(
        runs.starts <= 5,
        "EV charging split into {} runs: {}",
        runs.starts,
        ev_run_list(&p1, inputs.n)
    );
}

/// What each planner knob does on ven-1's live instance of 2026-10-05T19:40Z: runs,
/// money, and the import the malus exists to reduce. `BENCH_ONLY=a,b` restricts.
#[test]
#[ignore = "ven-1 EV fragmentation matrix: 7 variants, ~4 min"]
fn bench_ven1_ev_fragmentation() {
    let variants = [
        Variant {
            name: "prod",
            malus_eur_kwh: None,
            startup_eur: None,
            budget_s: 15.0,
        },
        Variant {
            name: "prod 45s",
            malus_eur_kwh: None,
            startup_eur: None,
            budget_s: 45.0,
        },
        Variant {
            name: "prod 60s",
            malus_eur_kwh: None,
            startup_eur: None,
            budget_s: 60.0,
        },
        Variant {
            name: "startup .01",
            malus_eur_kwh: None,
            startup_eur: Some(0.01),
            budget_s: 15.0,
        },
        Variant {
            name: "startup .30",
            malus_eur_kwh: None,
            startup_eur: Some(0.30),
            budget_s: 15.0,
        },
        Variant {
            name: "malus 0.10",
            malus_eur_kwh: Some(0.10),
            startup_eur: None,
            budget_s: 15.0,
        },
        Variant {
            name: "malus off",
            malus_eur_kwh: Some(0.0),
            startup_eur: None,
            budget_s: 15.0,
        },
    ];
    let only = std::env::var("BENCH_ONLY").ok();
    let Instance {
        inputs,
        ctxs,
        profile,
    } = ven1_frag_instance();
    let mismatched = (0..inputs.n)
        .filter(|&t| inputs.a_ev[t] != frag::PLUGGED[t])
        .count();
    println!(
        "
-- ven-1 2026-10-05T19:40Z | obligations {} | plugged mask differs from live in {mismatched} slots",
        inputs.ev_obligations.len()
    );
    println!(
        "   runs/one-slot blips over 48 h; grid = import - export at the tariffs
"
    );
    for v in &variants {
        if let Some(o) = &only {
            if !o.split(',').any(|x| x.trim() == v.name) {
                continue;
            }
        }
        let mut prof = profile.clone();
        if let Some(m) = v.malus_eur_kwh {
            prof.planner.c_ctrl_imp_malus_eur_kwh = m;
        }
        if let Some(c) = v.startup_eur {
            prof.planner.c_bat_startup_eur = c;
            prof.planner.c_ev_startup_eur = c;
        }
        let p1w = build_phase1_weights(&prof, PlannerObjective::MinCost);
        let p2w = build_phase2_weights(&inputs, &prof.planner);
        let t = Instant::now();
        let p1 = match solve_phase1(&inputs, &p1w, &ctxs, 60.0) {
            Ok(p) => p,
            Err(e) => {
                println!("  {:<14} phase 1 ERR {e}", v.name);
                continue;
            }
        };
        println!(
            "  {:<14} p1 {:>5.1}s {:<10} {}",
            v.name,
            t.elapsed().as_secs_f64(),
            format!("{:?}", p1.status),
            variant_row(&inputs, &p1)
        );
        let t = Instant::now();
        let eps = prof.planner.phase2_epsilon_eur;
        match solve_phase2(
            &inputs,
            &p1w,
            &p2w,
            p1.objective_eur,
            eps,
            &p1,
            &ctxs,
            v.budget_s,
        ) {
            Ok((s, fr)) => {
                println!(
                    "  {:<14} p2 {:>5.1}s {:<10} {}  friction {fr:.3}",
                    "",
                    t.elapsed().as_secs_f64(),
                    format!("{:?}", s.status),
                    variant_row(&inputs, &s)
                );
                println!("  {:<14}    runs {}", "", ev_run_list(&s, inputs.n));
            }
            Err(e) => println!("  {:<14} p2 ERR {e}", ""),
        }
    }
}
