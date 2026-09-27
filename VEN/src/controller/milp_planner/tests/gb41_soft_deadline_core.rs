//! GB-41: does a soft-deadline (`MayRun`) EV session charge on a site with no PV
//! and no battery?
//!
//! Four of nine fleet VENs charged nothing at all for 24 h with a valid
//! `BY_DEADLINE` session and `soft_deadline: true`. The solver returned OPTIMAL
//! and set `z_ev_core = 0` — it *decided* not to charge. Live probing narrowed
//! the discriminator to PV/battery presence and ruled out price, CO2 weighting,
//! the comfort reward, session creation, `plugged`, and solver cost.
//!
//! The fleet no longer exercises this path: `ev-usage-forecast`'s
//! `engage_charge_planning` sets `MustRun`, whose constraint is an equality with
//! no `z_ev_core` binary at all. So the symptom is gone from the fleet while the
//! decision that produced it is untouched — any user-created soft-deadline
//! request still goes through `MayRun`. These tests settle "solved or merely
//! bypassed" offline, which is what GB-41 itself proposes as the next step.
//!
//! Every assertion carries a full input/output dump (`dump`), so a failure is
//! diagnosable from the test output alone without re-running a 24 h fleet
//! scenario — the thing the original investigation could not do, because the
//! run's per-slot `p_ev_kw` was never retained.

use super::*;
// The same weights the other solver tests use: w_energy = 1, everything else
// off. Deliberately the case most favourable to charging -- no CO2 term, no
// grid/import malus, no wear. If the solver still declines here, the cause is a
// constraint, not a price.
use super::solver::make_phase1_weights;

/// ven-11's EV, as GB-41 recorded it: 7.4 kW, 50 kWh, 30 % -> 80 % target, so
/// 25 kWh of core energy. ven-19 (which charged) has the same EV and differs
/// only in site assets — that pair is the controlled experiment.
const P_EV_MAX_KW: f64 = 7.4;
const BATTERY_KWH: f64 = 50.0;
const SOC_START: f64 = 0.30;
const SOC_TARGET: f64 = 0.80;
const CORE_KWH: f64 = BATTERY_KWH * (SOC_TARGET - SOC_START);
/// `profile::defaults::default_v_ev_core` — the reward is `core_kwh * this`.
const V_EV_CORE_EUR_KWH: f64 = 1.0;

/// One site variant to solve. `pv_kw` and the battery are what GB-41 narrowed
/// the difference down to.
struct Site {
    name: &'static str,
    pv_kw: f64,
    battery: bool,
}

const SITES: [Site; 4] = [
    // ven-11 / ven-16-shaped: nothing but the EV and the house.
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
    // ven-19-shaped: the site that charged.
    Site {
        name: "with PV + battery (ven-19)",
        pv_kw: 5.0,
        battery: true,
    },
];

/// A 24-slot hourly horizon with a realistic tariff shape, a soft-deadline EV
/// session due at the end, and the site's own assets.
fn inputs_for(site: &Site) -> MilpInputs {
    let n = 24;
    // Cheap overnight, expensive daytime — the shape the fleet actually saw.
    let c_imp: Vec<f64> = (0..n)
        .map(|t| if (0..6).contains(&t) { 0.06 } else { 0.25 })
        .collect();
    let p_pv: Vec<f64> = (0..n)
        .map(|t| {
            // A daylight bell, zero at night.
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
        // The session: soft deadline at the end of the horizon, so MayRun.
        a_ev: vec![true; n],
        ev_mode: MilpLoadMode::MayRun,
        t_ev_dead_step: Some(n - 1),
        p_ev_max_kw: P_EV_MAX_KW,
        p_ev_min_kw: 1.4, // semi-continuous floor, as the fleet profiles set
        e_ev_core_kwh: CORE_KWH,
        e_ev_extra_max_kwh: BATTERY_KWH * (1.0 - SOC_TARGET),
        v_ev_core_eur: CORE_KWH * V_EV_CORE_EUR_KWH,
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
        ev_core_unmet_warning: None,
    }
}

/// Everything needed to diagnose a failure without re-running anything: the
/// decision, the money behind it, and the per-slot plan.
fn dump(site: &Site, inputs: &MilpInputs, out: &SolveOutput) -> String {
    let ev_energy: f64 = out
        .p_ev_kw
        .iter()
        .zip(inputs.dt_h.iter())
        .map(|(p, d)| p * d)
        .sum();
    let import_cost: f64 = out
        .p_imp_kw
        .iter()
        .zip(inputs.c_imp_eur_kwh.iter())
        .zip(inputs.dt_h.iter())
        .map(|((p, c), d)| p * c * d)
        .sum();
    let cheapest = inputs
        .c_imp_eur_kwh
        .iter()
        .cloned()
        .fold(f64::INFINITY, f64::min);
    let mut s = String::new();
    s.push_str(&format!("\n── GB-41 probe: {} ──\n", site.name));
    s.push_str(&format!(
        "decision      z_ev_core = {:.3}   (0 = solver declined the core energy)\n",
        out.z_ev_core
    ));
    s.push_str(&format!("status        {:?}\n", out.status));
    s.push_str(&format!("objective     {:.4} EUR\n", out.objective_eur));
    s.push_str(&format!(
        "core          {CORE_KWH:.1} kWh wanted, reward {:.2} EUR ({V_EV_CORE_EUR_KWH:.2} EUR/kWh)\n",
        inputs.v_ev_core_eur
    ));
    s.push_str(&format!(
        "cheapest rate {cheapest:.3} EUR/kWh -> core at best price would cost {:.2} EUR\n",
        CORE_KWH * cheapest
    ));
    s.push_str(&format!(
        "delivered     {ev_energy:.2} kWh EV,  site import cost {import_cost:.2} EUR\n"
    ));
    s.push_str(&format!(
        "ev p_min/max  {:.1} / {:.1} kW,  import cap {:.1} kW,  base {:.1} kW\n",
        inputs.p_ev_min_kw, inputs.p_ev_max_kw, inputs.p_imp_max_cont_kw[0], inputs.p_base_kw[0]
    ));
    s.push_str("slot  rate   pv    p_ev   p_imp  bat_ch bat_dis\n");
    for t in 0..inputs.n {
        s.push_str(&format!(
            "{t:>4}  {:.2}  {:4.1}  {:5.2}  {:5.2}  {:5.2}  {:5.2}\n",
            inputs.c_imp_eur_kwh[t],
            inputs.p_pv_kw[t],
            out.p_ev_kw[t],
            out.p_imp_kw[t],
            out.p_bat_ch_kw.get(t).copied().unwrap_or(0.0),
            out.p_bat_dis_kw.get(t).copied().unwrap_or(0.0),
        ));
    }
    s
}

fn solve_site(site: &Site) -> (MilpInputs, SolveOutput) {
    let inputs = inputs_for(site);
    let out = solve_phase1(
        &inputs,
        &make_phase1_weights(),
        &contexts_from_inputs(&inputs),
        60.0,
    )
    .unwrap_or_else(|e| panic!("{} failed to solve: {e:?}", site.name));
    (inputs, out)
}

/// The question GB-41 asks: on a site with no PV and no battery, does a
/// soft-deadline session get its core energy?
#[test]
fn gb41_soft_deadline_core_is_taken_on_a_bare_site() {
    let site = &SITES[0];
    let (inputs, out) = solve_site(site);
    let report = dump(site, &inputs, &out);
    assert!(
        out.z_ev_core >= 0.5,
        "GB-41 reproduces: the solver declined the core energy on a site with no PV \
         and no battery, exactly as four fleet VENs did for 24 h.{report}"
    );
    let ev_energy: f64 = out
        .p_ev_kw
        .iter()
        .zip(inputs.dt_h.iter())
        .map(|(p, d)| p * d)
        .sum();
    assert!(
        ev_energy >= CORE_KWH - 0.5,
        "core energy was committed (z_ev_core=1) but not delivered.{report}"
    );
}

/// The controlled pair from GB-41: same EV, same session, same prices — only
/// the site assets differ. If the bare site declines and these charge, the
/// discriminator really is PV/battery presence and the defect is live.
#[test]
fn gb41_every_site_variant_reaches_the_same_decision() {
    let mut declined: Vec<&str> = Vec::new();
    let mut reports = String::new();
    for site in SITES.iter() {
        let (inputs, out) = solve_site(site);
        reports.push_str(&dump(site, &inputs, &out));
        if out.z_ev_core < 0.5 {
            declined.push(site.name);
        }
    }
    assert!(
        declined.is_empty(),
        "the same soft-deadline session is taken on some sites and declined on others, \
         which is GB-41's discriminator. Declined: {declined:?}{reports}"
    );
}

/// GB-41 proposes this as the decisive follow-up: if the same site charges once
/// the deadline is firm, the soft-deadline core value is priced below what the
/// solver would rather avoid, and the fix belongs in that valuation.
#[test]
fn gb41_a_firm_deadline_charges_the_same_bare_site() {
    let site = &SITES[0];
    let mut inputs = inputs_for(site);
    inputs.ev_mode = MilpLoadMode::MustRun;
    inputs.v_ev_core_eur = 0.0; // firm deadlines carry no comfort reward
    let out = solve_phase1(
        &inputs,
        &make_phase1_weights(),
        &contexts_from_inputs(&inputs),
        60.0,
    )
    .unwrap_or_else(|e| panic!("firm-deadline solve failed: {e:?}"));
    let ev_energy: f64 = out
        .p_ev_kw
        .iter()
        .zip(inputs.dt_h.iter())
        .map(|(p, d)| p * d)
        .sum();
    assert!(
        ev_energy >= CORE_KWH - 0.5,
        "a firm deadline must deliver the core energy on any site.{}",
        dump(site, &inputs, &out)
    );
}

/// The fleet's real weights, from `profile::defaults`: the controllable-import
/// malus (0.22 EUR/kWh on top of the tariff) is the one that could plausibly
/// outweigh a comfort reward, and a PV site can dodge it by charging from
/// surplus instead of import — which is exactly the shape of GB-41's
/// PV-vs-no-PV discriminator.
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

/// One axis of the sweep below: what the solve is priced and bounded by.
struct Variant {
    name: &'static str,
    weights: fn() -> Phase1Weights,
    /// Site import ceiling [kW] — GB-41 lists "site import limit vs p_max_kw +
    /// base load" as a candidate constraint.
    import_cap_kw: f64,
}

/// Sweeps every site against every pricing/bounding variant and reports the
/// decision for each. This is the experiment GB-41 asks for: if the bare site
/// declines under some variant while the PV site takes it, that variant is the
/// cause, and the dump names it.
#[test]
fn gb41_sweep_sites_against_pricing_and_limits() {
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

    let mut table = String::from(
        "
── GB-41 sweep: z_ev_core per site x variant (0 = declined) ──
",
    );
    let mut declined: Vec<String> = Vec::new();
    let mut detail = String::new();

    for variant in variants.iter() {
        table.push_str(&format!(
            "
{}
",
            variant.name
        ));
        for site in SITES.iter() {
            let mut inputs = inputs_for(site);
            inputs.p_imp_max_cont_kw = vec![variant.import_cap_kw; inputs.n];
            inputs.p_imp_max_phys_kw = vec![variant.import_cap_kw; inputs.n];
            let out = solve_phase1(
                &inputs,
                &(variant.weights)(),
                &contexts_from_inputs(&inputs),
                60.0,
            )
            .unwrap_or_else(|e| panic!("{} / {} failed to solve: {e:?}", variant.name, site.name));
            let ev_kwh: f64 = out
                .p_ev_kw
                .iter()
                .zip(inputs.dt_h.iter())
                .map(|(p, d)| p * d)
                .sum();
            table.push_str(&format!(
                "  {:<34} z_ev_core={:.2}  delivered={:6.2} kWh  obj={:8.3}
",
                site.name, out.z_ev_core, ev_kwh, out.objective_eur
            ));
            if out.z_ev_core < 0.5 {
                declined.push(format!("{} / {}", variant.name, site.name));
                detail.push_str(&dump(site, &inputs, &out));
            }
        }
    }

    // A site with PV taking the core while the bare site declines IS GB-41.
    // Any decline at all is worth failing on: the session asked for energy and
    // the solver said no.
    assert!(
        declined.is_empty(),
        "the soft-deadline core was declined in {} combination(s): {declined:#?}{table}{detail}",
        declined.len()
    );
    // Not an assertion, but the table is the point of the test — print it so a
    // passing run still leaves the evidence behind (`cargo test -- --nocapture`).
    println!("{table}");
}

/// **The mechanism behind GB-41, reproduced.**
///
/// `UserRequestMode::ByDeadlineFree` / `AsapFree` / `Opportunistic` set
/// `free_only`, and `EvMilpContext::inject_grid_slots` then caps EV power per
/// slot at `(pv - base).max(0)` — PV surplus over the house load. Combined with
/// `MayRun`'s **all-or-nothing** core (`ev_energy == e_core * z_ev_core`), a
/// site that cannot cover the *entire* core from surplus charges **nothing at
/// all** — not "as much as it can".
///
/// That covers everything GB-41 called unexplained, including the counterexample
/// that broke its own PV rule (ven-18 *has* PV and still charged nothing): what
/// matters is not whether a site has PV, but whether its surplus covers the
/// whole core. It also explains OPTIMAL-with-no-charging and the price
/// independence — nothing here is a price decision.
///
/// What this does NOT establish is which session mode the 2026-08/09 campaign
/// actually created. If those sessions were `BY_DEADLINE` (not `*_FREE`), this
/// mechanism was not active and GB-41 stays open on a different cause.
fn free_energy_ctx(
    inputs: &MilpInputs,
) -> Box<dyn crate::controller::milp_planner::AssetMilpContext> {
    use crate::controller::milp_planner::asset_port::{EvMilpContext, EvMilpMode};
    use crate::services::test_support::milp_mocks::MockEvCtx;
    let mut ctx = EvMilpContext {
        mode: EvMilpMode::MayRun,
        soc_init: SOC_START,
        a_ev: vec![true; inputs.n],
        soc_drops: None,
        core_unmet_warning: None,
        t_dead_step: Some(inputs.n - 1),
        p_max_kw: P_EV_MAX_KW,
        p_min_kw: 0.0, // surplus rarely reaches a semi-continuous floor
        e_core_kwh: CORE_KWH,
        e_extra_max_kwh: BATTERY_KWH * (1.0 - SOC_TARGET),
        v_extra_eur_kwh: 0.0,
        v_core_eur: CORE_KWH * V_EV_CORE_EUR_KWH,
        asap_lateness_eur_kwh_h: 0.0,
        free_only: true,
        p_free_cap_kw: None,
        reward_per_slot: false,
        free_early_bias: false,
        budget_eur: None,
        c_imp_eur_kwh: None,
        v_extra_co2_eur_kwh: 0.0,
        v_core_co2_eur: 0.0,
    };
    ctx.inject_grid_slots(&inputs.c_imp_eur_kwh, &inputs.p_pv_kw, &inputs.p_base_kw);
    Box::new(MockEvCtx { ctx })
}

/// Solves `site` with a free-energy-gated EV session; returns (surplus offered,
/// core taken?, kWh delivered, report).
fn solve_free_energy(site: &Site) -> (f64, bool, f64, String) {
    let mut inputs = inputs_for(site);
    inputs.p_ev_min_kw = 0.0;
    let mut contexts = contexts_from_inputs(&inputs);
    contexts.retain(|c| c.asset_id() != "ev");
    let surplus: f64 = inputs
        .p_pv_kw
        .iter()
        .zip(inputs.p_base_kw.iter())
        .zip(inputs.dt_h.iter())
        .map(|((pv, base), dt)| (pv - base).max(0.0) * dt)
        .sum();
    contexts.push(free_energy_ctx(&inputs));
    let out = solve_phase1(&inputs, &realistic_weights(), &contexts, 60.0)
        .unwrap_or_else(|e| panic!("{} free-energy solve failed: {e:?}", site.name));
    let ev_kwh: f64 = out
        .p_ev_kw
        .iter()
        .zip(inputs.dt_h.iter())
        .map(|(p, d)| p * d)
        .sum();
    let report = format!(
        "
surplus offered over the horizon: {surplus:.2} kWh vs core {CORE_KWH:.1} kWh{}",
        dump(site, &inputs, &out)
    );
    (surplus, out.z_ev_core >= 0.5, ev_kwh, report)
}

#[test]
fn gb41_free_energy_without_pv_can_never_charge() {
    let (surplus, took_core, kwh, report) = solve_free_energy(&SITES[0]);
    assert!(
        surplus < 0.01,
        "a site with no PV offers no surplus{report}"
    );
    assert!(
        !took_core && kwh < 0.01,
        "expected zero charging when no surplus exists at all{report}"
    );
}

/// The finding that explains GB-41's own broken PV rule: partial surplus buys
/// **nothing**, because the core is all-or-nothing. ven-18 has PV and charged
/// nothing; this is why.
#[test]
fn gb41_free_energy_with_partial_surplus_charges_nothing_rather_than_partially() {
    let (surplus, took_core, kwh, report) = solve_free_energy(&SITES[1]); // 5 kW PV
    assert!(
        surplus > 1.0 && surplus < CORE_KWH,
        "this site must offer some surplus, but less than the core{report}"
    );
    assert!(
        !took_core && kwh < 0.01,
        "MayRun's core is all-or-nothing, so partial surplus delivers zero —          if this now charges partially, the model changed and GB-41's mechanism          is gone{report}"
    );
}

/// The control: give the same site enough surplus to cover the whole core and
/// it commits, proving the gate is the surplus volume and not the site type.
#[test]
fn gb41_free_energy_with_ample_surplus_delivers_the_core() {
    let ample = Site {
        name: "ample PV (surplus exceeds the core)",
        pv_kw: 14.0,
        battery: false,
    };
    let (surplus, took_core, kwh, report) = solve_free_energy(&ample);
    assert!(
        surplus > CORE_KWH,
        "fixture must offer more surplus than the core{report}"
    );
    assert!(
        took_core && kwh >= CORE_KWH - 0.5,
        "with surplus covering the core, a free-energy session must deliver it{report}"
    );
}

/// **The likely root cause of GB-41**, in two parts that only bite together.
///
/// Part 1 — valuation. `assets::ev_comfort::resolve_ev_comfort_reward` REPLACES
/// the profile's `v_ev_core_eur_kwh` (default 1.0 EUR/kWh) with the session's own
/// comfort rate at 0 % fill whenever the session carries `comfort_rates`; the
/// fleet's envelopes ran 0.05-0.35 EUR/kWh. Grid charging costs the tariff PLUS
/// the 0.22 EUR/kWh controllable-import malus, so at 0.10 EUR/kWh a 25 kWh core is
/// worth 2.50 EUR against ~7-12 EUR of import: declining is simply optimal, and
/// the solver says so with OPTIMAL and `EV_CORE_ENERGY_UNMET`.
///
/// Part 2 — the discriminator. A site whose PV surplus covers the core pays
/// neither tariff nor malus for it, so the same cheap session is still worth
/// taking there. Sites without enough surplus take nothing, because `MayRun`'s
/// core is all-or-nothing.
///
/// Together these predict GB-41's pattern *including* the counterexample that
/// broke its own "has PV" rule (ven-18 has PV and charged nothing): what matters
/// is whether surplus covers the whole core, not whether PV exists.
#[test]
fn gb41_a_low_comfort_rate_declines_grid_charging_but_not_free_charging() {
    let comfort_rate_eur_kwh = 0.10;
    let ample_pv = Site {
        name: "ample PV (surplus covers the core)",
        pv_kw: 14.0,
        battery: false,
    };
    let mut outcome: Vec<(&str, bool)> = Vec::new();
    let mut report = String::new();

    for site in [&SITES[0], &ample_pv] {
        let mut inputs = inputs_for(site);
        inputs.v_ev_core_eur = CORE_KWH * comfort_rate_eur_kwh;
        let out = solve_phase1(
            &inputs,
            &realistic_weights(),
            &contexts_from_inputs(&inputs),
            60.0,
        )
        .unwrap_or_else(|e| panic!("{} failed to solve: {e:?}", site.name));
        report.push_str(&dump(site, &inputs, &out));
        outcome.push((site.name, out.z_ev_core >= 0.5));
    }

    assert!(
        !outcome[0].1,
        "a {comfort_rate_eur_kwh} EUR/kWh core must not be worth importing at          tariff + 0.22 malus{report}"
    );
    assert!(
        outcome[1].1,
        "the same cheap core must still be taken where surplus covers it — that          asymmetry is GB-41's discriminator{report}"
    );
}

/// The same site and session, with the profile's own default reward instead of
/// a low comfort rate: it charges. This is what makes the line above a
/// valuation problem rather than a constraint problem.
#[test]
fn gb41_the_profile_default_reward_would_have_charged_the_same_site() {
    let site = &SITES[0];
    let inputs = inputs_for(site); // v_ev_core_eur = CORE * 1.0 EUR/kWh
    let out = solve_phase1(
        &inputs,
        &realistic_weights(),
        &contexts_from_inputs(&inputs),
        60.0,
    )
    .unwrap();
    assert!(
        out.z_ev_core >= 0.5,
        "at the profile default the bare site must charge{}",
        dump(site, &inputs, &out)
    );
}
