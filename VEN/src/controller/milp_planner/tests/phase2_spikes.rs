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
use data::SLOTS;

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
    let steps: Vec<i64> = profile
        .planner
        .plan_zones
        .iter()
        .flat_map(|z| std::iter::repeat_n(z.step_s as i64, z.slots))
        .collect();
    let mut start = ven1_now();
    let mut snaps = Vec::new();
    for (i, &(_, _, imp, exp, co2)) in SLOTS.iter().enumerate() {
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
    let now = ven1_now();
    let profile = ven1_profile();
    let sim = make_snap_from_profile(&profile);
    let tariffs = ven1_tariffs(&profile);
    let cap = no_capacity();
    let mut ctxs = build_asset_contexts(&profile, &sim, now, None, None, &tariffs);
    let mut inputs = build_milp_inputs(&ctxs, &tariffs, &cap, &profile, now, &[], None);
    assert_eq!(inputs.n, 288);
    for (i, &(pv, base, _, _, _)) in SLOTS.iter().enumerate() {
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
