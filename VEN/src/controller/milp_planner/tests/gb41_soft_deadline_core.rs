//! GB-41's regression harness, restated for `ev-comfort-piecewise-core`.
//!
//! GB-41: four of nine fleet VENs charged nothing at all for 24 h with a valid
//! soft-deadline session, solving OPTIMAL every cycle. Reproduced offline here on
//! 2026-09-27, the mechanism was an all-or-nothing core — one binary
//! (`z_ev_core`) deciding the whole requested block — priced against a comfort
//! bid that the 0.22 €/kWh controllable-import malus pushed below cost. When the
//! bid did not cover the *whole* block, zero was optimal.
//!
//! That binary is gone. The comfort curve now prices every kWh from the current
//! SoC to full as continuous bands, so a bid covering part of the energy buys
//! that part. These tests state the new behaviour and fail if the cliff returns.
//!
//! Every assertion carries a full input/output dump (`dump`), so a failure is
//! diagnosable from the test output alone — the thing the original investigation
//! could not do, because that run's per-slot `p_ev_kw` was never retained.

use super::solver::make_phase1_weights;
use super::*;
use crate::controller::milp_planner::asset_port::EvEnergySegment;

/// ven-11's EV, as GB-41 recorded it: 7.4 kW, 50 kWh, 30 % SoC, 80 % target — so
/// 25 kWh to the target and 10 kWh beyond it. ven-19 (which charged) had the same
/// EV and differed only in site assets; that pair is the controlled experiment.
const P_EV_MAX_KW: f64 = 7.4;
const BATTERY_KWH: f64 = 50.0;
const SOC_START: f64 = 0.30;
const SOC_TARGET: f64 = 0.80;
const TO_TARGET_KWH: f64 = BATTERY_KWH * (SOC_TARGET - SOC_START);
const BEYOND_TARGET_KWH: f64 = BATTERY_KWH * (1.0 - SOC_TARGET);
/// `profile::defaults::default_v_ev_core`.
const DEFAULT_CORE_BID: f64 = 1.0;

struct Site {
    name: &'static str,
    pv_kw: f64,
    battery: bool,
}

const SITES: [Site; 4] = [
    Site {
        name: "bare (ven-11: EV + base load only)",
        pv_kw: 0.0,
        battery: false,
    },
    Site {
        name: "with PV",
        pv_kw: 5.0,
        battery: false,
    },
    Site {
        name: "with battery",
        pv_kw: 0.0,
        battery: true,
    },
    Site {
        name: "with PV + battery (ven-19)",
        pv_kw: 5.0,
        battery: true,
    },
];

/// The bands a session's curve produces: `to_target` at `bid`, the rest of the
/// battery at `bid_beyond`.
fn bands(bid: f64, bid_beyond: f64) -> Vec<EvEnergySegment> {
    vec![
        EvEnergySegment {
            kwh: TO_TARGET_KWH,
            eur_per_kwh: bid,
        },
        EvEnergySegment {
            kwh: BEYOND_TARGET_KWH,
            eur_per_kwh: bid_beyond,
        },
    ]
}

/// A 24-slot hourly horizon with a realistic tariff shape and the site's assets.
fn inputs_for(site: &Site) -> MilpInputs {
    let n = 24;
    let c_imp: Vec<f64> = (0..n)
        .map(|t| if (0..6).contains(&t) { 0.06 } else { 0.25 })
        .collect();
    let p_pv: Vec<f64> = (0..n)
        .map(|t| {
            let x = (t as f64 - 13.0) / 4.0;
            (site.pv_kw * (1.0 - x * x)).max(0.0)
        })
        .collect();

    MilpInputs {
        n,
        dt_h: vec![1.0; n],
        cum_s: (0..=n as i64).map(|i| i * 3600).collect(),
        c_imp_eur_kwh: c_imp,
        rate_stale: vec![false; n],
        stale_rate_warning: None,
        co2_stale_rate_warning: None,
        budget_warning: None,
        c_exp_eur_kwh: vec![0.08; n],
        g_imp_kgco2_kwh: vec![0.30; n],
        p_pv_kw: p_pv,
        p_base_kw: vec![0.5; n],
        p_imp_max_phys_kw: vec![25.0; n],
        p_exp_max_phys_kw: vec![10.0; n],
        p_imp_max_cont_kw: vec![25.0; n],
        p_exp_max_cont_kw: vec![10.0; n],
        pen_imp_eur_kwh: 0.0,
        pen_exp_eur_kwh: 0.0,
        mip_gap_target: 0.02,
        penalty_rules: vec![],
        e_bat_nom_kwh: site.battery.then_some(10.0),
        e_bat_init_kwh: site.battery.then_some(5.0),
        e_bat_min_kwh: site.battery.then_some(1.0),
        e_bat_max_kwh: site.battery.then_some(9.0),
        p_bat_ch_max_kw: site.battery.then_some(5.0),
        p_bat_dis_max_kw: site.battery.then_some(5.0),
        eff_bat_ch: site.battery.then_some(0.96),
        eff_bat_dis: site.battery.then_some(0.96),
        a_ev: vec![true; n],
        // Soft deadline: nothing guaranteed, the bids decide.
        ev_mode: MilpLoadMode::MayRun,
        t_ev_dead_step: Some(n - 1),
        p_ev_max_kw: P_EV_MAX_KW,
        p_ev_min_kw: 1.4,
        e_ev_required_kwh: 0.0,
        ev_segments: bands(DEFAULT_CORE_BID, 0.10),
        e_ev_extra_max_kwh: 0.0,
        v_ev_extra_eur_kwh: 0.0,
        heater_mode: MilpLoadMode::MustNotRun,
        t_heat_dead_step: None,
        p_heat_step_kw: 0.0,
        heat_n_stages: 0,
        e_heat_init_kwh: 0.0,
        e_heat_max_kwh: 0.0,
        q_heat_dem_kw: 0.0,
        e_heat_target_kwh: 0.0,
        lambda_heat_sw_eur: 0.0,
        w_tier_penalty_eur: 0.0,
        heat_initial_y: 0.0,
        shiftable_loads: vec![],
        soc_ev_init: Some(SOC_START),
        ev_soc_drops: None,
    }
}

fn delivered_kwh(inputs: &MilpInputs, out: &SolveOutput) -> f64 {
    out.p_ev_kw
        .iter()
        .zip(inputs.dt_h.iter())
        .map(|(p, d)| p * d)
        .sum()
}

/// Everything needed to diagnose a failure without re-running anything.
fn dump(site: &Site, inputs: &MilpInputs, out: &SolveOutput) -> String {
    let ev_energy = delivered_kwh(inputs, out);
    let cheapest = inputs
        .c_imp_eur_kwh
        .iter()
        .cloned()
        .fold(f64::INFINITY, f64::min);
    let mut s = String::new();
    s.push_str(&format!("\n── GB-41 probe: {} ──\n", site.name));
    s.push_str(&format!("status        {:?}\n", out.status));
    s.push_str(&format!("objective     {:.4} EUR\n", out.objective_eur));
    s.push_str(&format!("delivered     {ev_energy:.2} kWh EV\n"));
    s.push_str(&format!(
        "required      {:.2} kWh (0 = soft, the bids decide)\n",
        inputs.e_ev_required_kwh
    ));
    s.push_str("bands         ");
    for b in &inputs.ev_segments {
        s.push_str(&format!(
            "[{:.1} kWh @ {:.2} EUR/kWh] ",
            b.kwh, b.eur_per_kwh
        ));
    }
    s.push_str(&format!(
        "\ncheapest rate {cheapest:.3} EUR/kWh (+0.22 ctrl-import malus when priced)\n"
    ));
    s.push_str("slot  rate   pv    p_ev   p_imp\n");
    for t in 0..inputs.n {
        s.push_str(&format!(
            "{t:>4}  {:.2}  {:4.1}  {:5.2}  {:5.2}\n",
            inputs.c_imp_eur_kwh[t], inputs.p_pv_kw[t], out.p_ev_kw[t], out.p_imp_kw[t],
        ));
    }
    s
}

fn solve(inputs: &MilpInputs, weights: &Phase1Weights, site: &Site) -> SolveOutput {
    solve_phase1(inputs, weights, &contexts_from_inputs(inputs), 60.0)
        .unwrap_or_else(|e| panic!("{} failed to solve: {e:?}", site.name))
}

/// The fleet's real weights: the 0.22 €/kWh controllable-import malus is the term
/// that pushed a comfort bid below cost in GB-41.
fn realistic_weights() -> Phase1Weights {
    Phase1Weights {
        w_energy: 1.0,
        w_ghg: 0.0001,
        w_grid: 0.0,
        w_import: 0.0,
        w_viol: 1.0,
        c_bat_wear_eur_kwh: 0.03,
        c_bat_ev_coexist_eur_kwh: 0.5,
        c_ctrl_imp_malus_eur_kwh: 0.22,
        w_services: 1.0,
    }
}

// ── The behaviour this change exists for ─────────────────────────────────────

/// A bid that clears the cost buys the energy — on the bare site, which is the
/// case GB-41 never charged on.
#[test]
fn a_worthwhile_bid_charges_on_a_bare_site() {
    let site = &SITES[0];
    let inputs = inputs_for(site);
    let out = solve(&inputs, &realistic_weights(), site);
    let kwh = delivered_kwh(&inputs, &out);
    assert!(
        kwh >= TO_TARGET_KWH - 0.5,
        "a 1.00 EUR/kWh bid must buy the 25 kWh to target even at tariff + malus{}",
        dump(site, &inputs, &out)
    );
}

/// **The heart of the change.** A bid below the cost of most energy buys the part
/// it does cover instead of nothing. Under the old all-or-nothing core this
/// delivered exactly 0 — which is what GB-41 observed on the fleet.
#[test]
fn a_bid_that_covers_only_part_of_the_energy_buys_that_part() {
    let site = &SITES[0];
    let mut inputs = inputs_for(site);
    // Only two cheap hours, so the charger can draw at most 2 x 7.4 = 14.8 kWh
    // of the 25 kWh at a price this bid covers; the rest would have to come from
    // 0.25-rate slots costing 0.47 with the malus.
    inputs.c_imp_eur_kwh = (0..inputs.n)
        .map(|t| if t < 2 { 0.06 } else { 0.25 })
        .collect();
    // 0.284 clears a cheap slot (0.06 + 0.22 malus = 0.28) but not a dear one.
    inputs.ev_segments = bands(0.284, 0.0);
    let out = solve(&inputs, &realistic_weights(), site);
    let kwh = delivered_kwh(&inputs, &out);
    let report = dump(site, &inputs, &out);
    assert!(
        kwh > 0.5,
        "the covered part must be bought, not refused wholesale{report}"
    );
    assert!(
        kwh < TO_TARGET_KWH - 0.5,
        "and only the covered part — the dear slots are not worth this bid{report}"
    );
}

/// The same low bid on a site with surplus buys strictly more, because surplus
/// costs neither tariff nor malus. GB-41 read this as "PV sites charge, bare
/// sites don't"; it is now a difference of degree, not of kind.
#[test]
fn surplus_buys_more_of_the_same_low_bid_than_a_bare_site_does() {
    let bare = &SITES[0];
    let sunny = Site {
        name: "ample PV",
        pv_kw: 14.0,
        battery: false,
    };
    let mut a = inputs_for(bare);
    a.ev_segments = bands(0.10, 0.0);
    let mut b = inputs_for(&sunny);
    b.ev_segments = bands(0.10, 0.0);
    let out_a = solve(&a, &realistic_weights(), bare);
    let out_b = solve(&b, &realistic_weights(), &sunny);
    let (kwh_a, kwh_b) = (delivered_kwh(&a, &out_a), delivered_kwh(&b, &out_b));
    assert!(
        kwh_b > kwh_a + 0.5,
        "surplus must buy more of the same bid: bare {kwh_a:.2} kWh vs sunny {kwh_b:.2} kWh{}{}",
        dump(bare, &a, &out_a),
        dump(&sunny, &b, &out_b)
    );
}

/// A firm deadline is a guarantee: it delivers its floor even when the bid is far
/// below the cost of the energy. The "I need 60 % to reach the destination" case
/// belongs here, not in a rewarded binary that can decline.
#[test]
fn a_firm_deadline_delivers_its_floor_however_low_the_bid() {
    let site = &SITES[0];
    let mut inputs = inputs_for(site);
    inputs.ev_mode = MilpLoadMode::MustRun;
    inputs.e_ev_required_kwh = TO_TARGET_KWH;
    inputs.ev_segments = bands(0.0, 0.0); // no comfort value at all
    let out = solve(&inputs, &realistic_weights(), site);
    let kwh = delivered_kwh(&inputs, &out);
    assert!(
        kwh >= TO_TARGET_KWH - 0.5,
        "a firm requirement must be met regardless of the bid{}",
        dump(site, &inputs, &out)
    );
}

/// A guarantee the window cannot physically hold: the floor is capped to what
/// the available slots can deliver (otherwise the solve is infeasible, since the
/// EV's energy balance has no slack), the plan charges that much, and the gap is
/// reported — the one remaining meaning of `EV_CORE_ENERGY_UNMET`.
#[test]
fn a_firm_requirement_beyond_the_window_charges_all_it_can_and_reports_the_gap() {
    let site = &SITES[0];
    let mut inputs = inputs_for(site);
    inputs.ev_mode = MilpLoadMode::MustRun;
    // Only the first two slots are available, so the window holds at most
    // 2 x 0.5 h x P_EV_MAX_KW — far less than the full requirement.
    inputs.a_ev = (0..inputs.n).map(|t| t < 2).collect();
    inputs.t_ev_dead_step = Some(1);
    inputs.e_ev_required_kwh = TO_TARGET_KWH;
    inputs.ev_segments = bands(0.0, 0.0); // the guarantee, not a bid, is on trial

    let out = solve(&inputs, &realistic_weights(), site);
    let reachable_kwh = 2.0 * inputs.dt_h[0] * P_EV_MAX_KW;
    let kwh = delivered_kwh(&inputs, &out);
    assert!(
        (kwh - reachable_kwh).abs() < 0.5,
        "must charge everything the window allows ({reachable_kwh:.2} kWh){}",
        dump(site, &inputs, &out)
    );

    let warnings = super::super::ev_diagnostics::ev_warnings(&inputs, &out);
    let w = warnings
        .iter()
        .find(|w| w.kind == crate::entities::plan::WarningKind::EvCoreEnergyUnmet)
        .unwrap_or_else(|| {
            panic!(
                "an unmet guarantee must be reported, got {warnings:?}{}",
                dump(site, &inputs, &out)
            )
        });
    assert!(
        w.message.contains(&format!("{TO_TARGET_KWH:.1} kWh"))
            && w.message.contains(&format!("{kwh:.1} kWh")),
        "the warning must name both required and delivered energy: {}",
        w.message
    );
}

/// The mirror of the test above: a **soft** request that charges less than its
/// target reports nothing, because it promised nothing.
#[test]
fn a_soft_request_that_charges_partially_reports_no_unmet_obligation() {
    let site = &SITES[0];
    let mut inputs = inputs_for(site);
    // Same construction as `a_bid_that_covers_only_part_of_the_energy_buys_that
    // _part`: two cheap hours the bid clears, the rest dear enough that it does not.
    inputs.c_imp_eur_kwh = (0..inputs.n)
        .map(|t| if t < 2 { 0.06 } else { 0.25 })
        .collect();
    inputs.ev_segments = bands(0.284, 0.0);
    let out = solve(&inputs, &realistic_weights(), site);
    let kwh = delivered_kwh(&inputs, &out);
    assert!(
        kwh > 0.5 && kwh < TO_TARGET_KWH - 0.5,
        "this probe needs a genuinely partial charge, got {kwh:.2} kWh{}",
        dump(site, &inputs, &out)
    );
    assert!(
        super::super::ev_diagnostics::ev_warnings(&inputs, &out).is_empty(),
        "a soft request owes nothing, so a partial charge is not a shortfall{}",
        dump(site, &inputs, &out)
    );
}

/// Nothing is bought when no kWh is worth its cost — the one case where charging
/// nothing is still the right answer.
#[test]
fn a_bid_below_every_cost_buys_nothing() {
    let site = &SITES[0];
    let mut inputs = inputs_for(site);
    inputs.ev_segments = bands(0.01, 0.0);
    let out = solve(&inputs, &realistic_weights(), site);
    let kwh = delivered_kwh(&inputs, &out);
    assert!(
        kwh < 0.5,
        "0.01 EUR/kWh cannot justify energy costing at least 0.28{}",
        dump(site, &inputs, &out)
    );
}

/// Bands are bought most-valuable-first, so the energy toward the target is taken
/// before the energy beyond it.
#[test]
fn the_valuable_band_is_bought_before_the_cheap_one() {
    let site = &SITES[0];
    let mut inputs = inputs_for(site);
    inputs.ev_segments = bands(1.00, 0.29);
    let out = solve(&inputs, &realistic_weights(), site);
    let kwh = delivered_kwh(&inputs, &out);
    assert!(
        kwh >= TO_TARGET_KWH - 0.5,
        "the high-value band must be filled first{}",
        dump(site, &inputs, &out)
    );
}

/// The sweep GB-41 asked for, restated: every site delivers energy at a
/// worthwhile bid. Prints the table for the record.
#[test]
fn sweep_sites_against_pricing_and_limits() {
    struct Variant {
        name: &'static str,
        weights: fn() -> Phase1Weights,
        import_cap_kw: f64,
    }
    let variants = [
        Variant {
            name: "minimal weights, ample import",
            weights: make_phase1_weights,
            import_cap_kw: 25.0,
        },
        Variant {
            name: "fleet weights (0.22 ctrl-import malus), ample import",
            weights: realistic_weights,
            import_cap_kw: 25.0,
        },
        Variant {
            name: "fleet weights, import capped just above base load",
            weights: realistic_weights,
            import_cap_kw: 6.0,
        },
    ];

    let mut table = String::from("\n── delivered kWh per site x variant ──\n");
    let mut starved: Vec<String> = Vec::new();
    let mut detail = String::new();

    for variant in variants.iter() {
        table.push_str(&format!("\n{}\n", variant.name));
        for site in SITES.iter() {
            let mut inputs = inputs_for(site);
            inputs.p_imp_max_cont_kw = vec![variant.import_cap_kw; inputs.n];
            inputs.p_imp_max_phys_kw = vec![variant.import_cap_kw; inputs.n];
            let out = solve(&inputs, &(variant.weights)(), site);
            let kwh = delivered_kwh(&inputs, &out);
            table.push_str(&format!(
                "  {:<34} delivered={kwh:6.2} kWh  obj={:8.3}\n",
                site.name, out.objective_eur
            ));
            if kwh < 0.5 {
                starved.push(format!("{} / {}", variant.name, site.name));
                detail.push_str(&dump(site, &inputs, &out));
            }
        }
    }

    assert!(
        starved.is_empty(),
        "a 1.00 EUR/kWh bid bought nothing in {} combination(s): {starved:#?}{table}{detail}",
        starved.len()
    );
    println!("{table}");
}
