use std::collections::HashMap;

use chrono::{DateTime, Duration, Utc};

use super::asset_port::AssetMilpParams;
use crate::controller::milp_planner::AssetMilpContext;
use crate::controller::simulator_port::SimSnapshot;
use crate::entities::asset_params::{BaseLoadParams, PvParams};
use crate::entities::capacity::{AlertWindow, OadrCapacityState, SimpleWindow};
use crate::entities::design_vocabulary::AssetHeuristics;
use crate::entities::device_session::{BaselineOverride, ShiftableLoad};
use crate::entities::planner_params::PlannerParams;
use crate::entities::tariff_snapshot::TariffTimeSeries;

use super::types::*;

/// Natural sin-model irradiance [0,1] at time `ts`, without any user offset.
/// Mirrors `PvInverter::natural_irradiance_at()` from `assets/pv.rs`.
fn pv_natural_irradiance(ts: DateTime<Utc>) -> f64 {
    use chrono::Timelike;
    let hour = ts.hour() as f64 + ts.minute() as f64 / 60.0;
    if (6.0..=18.0).contains(&hour) {
        let angle = std::f64::consts::PI * (hour - 6.0) / 12.0;
        angle.sin().max(0.0)
    } else {
        0.0
    }
}

/// Build the full MILP input parameter set from asset contexts and current runtime state.
///
/// Asset-specific parameters (battery, EV, heater) are extracted via the `AssetMilpContext`
/// trait; grid, PV, and baseline parameters are still read from `profile` and `assets`.
#[allow(clippy::too_many_arguments)]
pub(crate) fn build_milp_inputs(
    asset_contexts: &[Box<dyn AssetMilpContext>],
    assets: &SimSnapshot,
    tariffs: &TariffTimeSeries,
    capacity: &OadrCapacityState,
    alert_windows: &[AlertWindow],
    simple_windows: &[SimpleWindow],
    planner: &PlannerParams,
    phys_imp: f64,
    phys_exp: f64,
    pv_cfg: Option<&PvParams>,
    base_load: Option<&BaseLoadParams>,
    now: DateTime<Utc>,
    shiftable_loads: &[ShiftableLoad],
    baseline_override: Option<&BaselineOverride>,
    pv_forecast_override: Option<f64>,
    asset_heuristics: &HashMap<String, AssetHeuristics>,
) -> MilpInputs {
    let n: usize = planner.plan_zones.iter().map(|z| z.slots).sum();
    let mut cum_s: Vec<i64> = Vec::with_capacity(n + 1);
    cum_s.push(0);
    let mut dt_h: Vec<f64> = Vec::with_capacity(n);
    for zone in &planner.plan_zones {
        let step_h = zone.step_s as f64 / 3600.0;
        for _ in 0..zone.slots {
            dt_h.push(step_h);
            cum_s.push(cum_s.last().unwrap() + zone.step_s as i64);
        }
    }
    let zone_a_step_s = planner.plan_zones.first().map(|z| z.step_s).unwrap_or(300);

    // ── Per-step grid arrays ──────────────────────────────────────────────────
    // WP3.3 (§8.10): subscription + reservation form a contracted allowance
    // that binds when tighter than the event limit / physical bound and is
    // inactive when looser. A reservation without a subscription (or vice
    // versa) counts alone.
    let allowance = |sub: Option<f64>, res: Option<f64>| match (sub, res) {
        (None, None) => f64::INFINITY,
        (s, r) => s.unwrap_or(0.0) + r.unwrap_or(0.0),
    };
    let imp_allowance = allowance(
        capacity.import_subscription_kw,
        capacity.import_reservation_kw,
    );
    let exp_allowance = allowance(
        capacity.export_subscription_kw,
        capacity.export_reservation_kw,
    );
    let cont_imp = capacity
        .import_limit_kw
        .unwrap_or(phys_imp)
        .min(imp_allowance);
    let cont_exp = capacity
        .export_limit_kw
        .unwrap_or(phys_exp)
        .min(exp_allowance);
    // Flat fallback (today's exact pre-WP5.2 behavior) used whenever no
    // learned heuristic exists yet for the asset (cold-start, or a VEN that
    // has never had `POST /debug/heuristics/preload` run / accumulated
    // enough history) — see the per-slot loop below for the heuristic path.
    let flat_base_kw = base_load.map(|c| c.baseline_kw).unwrap_or(0.0);
    let flat_residual_kw = assets
        .assets
        .get(crate::controller::residual::SITE_RESIDUAL_ASSET_ID)
        .map(|s| s.power_kw)
        .unwrap_or(0.0);
    // WP5.2 (BL-14): when a learned heuristic exists, the planner samples a
    // per-slot value (`daytime_profile_kw[weekday_bucket][hour] ×
    // seasonal_factor`) instead of repeating a flat scalar across the whole
    // horizon — this is what makes the Controller tab's future-horizon line
    // for base_load/site-residual show real daily structure instead of a
    // flat line once history has been seeded.
    let base_heuristic = asset_heuristics.get(crate::ids::ASSET_BASE_LOAD);
    let residual_heuristic =
        asset_heuristics.get(crate::controller::residual::SITE_RESIDUAL_ASSET_ID);

    // WP4.4 (BL-07): import rates come through the stale-rate policy — covered
    // slots interpolate, slots beyond tariff coverage are filled per policy.
    let slot_starts: Vec<DateTime<Utc>> = cum_s[0..n]
        .iter()
        .map(|&s| now + Duration::seconds(s))
        .collect();
    let stale_outcome = super::stale_rates::apply_stale_rate_policy(
        &planner.stale_rate_policy,
        planner.stale_rate_safe_pctl,
        tariffs,
        &slot_starts,
        0.25,
    );
    let c_imp = stale_outcome.c_imp_eur_kwh;

    let mut c_exp = Vec::with_capacity(n);
    let mut g_co2 = Vec::with_capacity(n);
    let mut p_pv = Vec::with_capacity(n);
    let mut p_base = Vec::with_capacity(n);
    let mut p_residual = Vec::with_capacity(n);
    let mut p_imp_phys = Vec::with_capacity(n);
    let mut p_exp_phys = Vec::with_capacity(n);
    let mut p_imp_cont = Vec::with_capacity(n);
    let mut p_exp_cont = Vec::with_capacity(n);

    for (i, &slot_s) in cum_s[0..n].iter().enumerate() {
        let slot_t = now + Duration::seconds(slot_s);
        let slot_end = now + Duration::seconds(cum_s[i + 1]);
        c_exp.push(
            tariffs
                .export_eur_kwh
                .interpolate_at(slot_t)
                .unwrap_or(0.08),
        );
        // CO₂ stored as g/kWh → MILP uses kgCO₂/kWh
        g_co2.push(tariffs.co2_g_kwh.interpolate_at(slot_t).unwrap_or(300.0) / 1000.0);
        // Use live PvInverter snapshot when available so that irradiance_offset (irradiance
        // slider) and pv_alpha (blend-back speed slider) both project into the
        // forecast. Falls back to the static sin model if no "pv" asset exists.
        // pv_forecast_override pins all horizon slots to a fixed kW,
        // making plans deterministic regardless of time-of-day.
        let pv_kw = if let Some(forced_kw) = pv_forecast_override {
            forced_kw.max(0.0)
        } else if let Some(pv_snap) = assets.assets.get("pv") {
            let natural = pv_natural_irradiance(slot_t);
            let irradiance_offset = pv_snap.val("irradiance_offset").unwrap_or(0.0);
            let pv_alpha = pv_snap.val("pv_alpha").unwrap_or(0.1);
            let rated_kw = pv_snap.val("rated_kw").unwrap_or(0.0);
            // pv_alpha is "fraction removed per zone-A step (zone_a_step_s)".
            // Exponent is the number of zone-A-equivalent steps ahead.
            let steps_ahead = slot_s as f64 / zone_a_step_s as f64;
            let decayed_offset = irradiance_offset * (1.0 - pv_alpha).powf(steps_ahead);
            (natural + decayed_offset).clamp(0.0, 1.0) * rated_kw
        } else {
            pv_cfg.map(|c| c.forecast_kw(slot_t)).unwrap_or(0.0)
        };
        p_pv.push(pv_kw);
        let base_kw_t = base_heuristic
            .map(|h| h.sample_kw(slot_t))
            .unwrap_or(flat_base_kw);
        p_base.push(base_kw_t);
        p_residual.push(
            residual_heuristic
                .map(|h| h.sample_kw(slot_t))
                .unwrap_or(flat_residual_kw),
        );
        p_imp_phys.push(phys_imp);
        p_exp_phys.push(phys_exp);
        // WP3.2: SIMPLE levels clamp the import cap per slot — level 1 to a
        // configurable fraction of the contractual limit, level 2 to the
        // baseline forecast (defer all flexible draw), level 3 to 0. Highest
        // overlapping level wins; combined with the contractual cap via min.
        let simple_level = simple_windows
            .iter()
            .filter(|w| w.start < slot_end && slot_t < w.end)
            .map(|w| w.level)
            .max();
        let simple_cap = match simple_level {
            Some(1) => cont_imp * planner.simple_level1_import_cap_pct,
            Some(2) => base_kw_t.max(0.0),
            Some(l) if l >= 3 => 0.0,
            _ => cont_imp,
        };
        // WP3.1 (BL-04): slots overlapping an active grid-alert window get an
        // import cap of 0 ("minimize electricity use", both alert types). The
        // cap is soft in the solver (slack + violation penalty), so unavoidable
        // base load yields a warned violation, never infeasibility. Export is
        // left untouched — the spec prescribes nothing for it. Alerts override
        // any SIMPLE level.
        let in_alert = alert_windows
            .iter()
            .any(|a| a.start < slot_end && slot_t < a.end);
        p_imp_cont.push(if in_alert {
            0.0
        } else {
            cont_imp.min(simple_cap)
        });
        p_exp_cont.push(cont_exp);
    }

    // ── Asset-context dispatch: battery / EV / heater scalars ────────────────
    let mut e_bat_nom: Option<f64> = None;
    let mut e_bat_init: Option<f64> = None;
    let mut e_bat_min: Option<f64> = None;
    let mut e_bat_max: Option<f64> = None;
    let mut p_bat_ch_max: Option<f64> = None;
    let mut p_bat_dis_max: Option<f64> = None;
    let mut eff_ch: Option<f64> = None;
    let mut eff_dis: Option<f64> = None;

    let mut a_ev = vec![false; n];
    let mut ev_mode = MilpLoadMode::MustNotRun;
    let mut t_ev_dead: Option<usize> = None;
    let mut p_ev_max = 0.0_f64;
    let mut p_ev_min = 0.0_f64;
    let mut e_ev_core = 0.0_f64;
    let mut e_ev_extra = 0.0_f64;
    let mut v_ev_extra = 0.0_f64;
    let mut v_ev_core = 0.0_f64;
    let mut ev_budget_eur: Option<f64> = None;
    let mut soc_ev_init: Option<f64> = None;

    let mut heater_mode = MilpLoadMode::MustNotRun;
    let mut t_heat_dead: Option<usize> = None;
    let mut p_mid = 0.0_f64;
    let mut p_full = 0.0_f64;
    let mut e_heat_init = 0.0_f64;
    let mut e_heat_max = 0.0_f64;
    let mut q_heat_dem = 0.0_f64;
    let mut e_heat_target = 0.0_f64;
    let mut lambda_sw = 0.0_f64;
    let mut heat_iz_mid = 0.0_f64;
    let mut heat_iz_full = 0.0_f64;

    for ctx in asset_contexts {
        match ctx.milp_params(n, now) {
            AssetMilpParams::Battery(b) => {
                e_bat_nom = Some(b.e_nom_kwh);
                e_bat_init = Some(b.e_init_kwh);
                e_bat_min = Some(b.e_min_kwh);
                e_bat_max = Some(b.e_max_kwh);
                p_bat_ch_max = Some(b.p_ch_max_kw);
                p_bat_dis_max = Some(b.p_dis_max_kw);
                eff_ch = Some(b.eff_ch);
                eff_dis = Some(b.eff_dis);
            }
            AssetMilpParams::Ev(e) => {
                a_ev = e.a_ev;
                ev_mode = e.mode;
                t_ev_dead = e.t_dead_step;
                p_ev_max = e.p_max_kw;
                p_ev_min = e.p_min_kw;
                e_ev_core = e.e_core_kwh;
                e_ev_extra = e.e_extra_max_kwh;
                v_ev_extra = e.v_extra_eur_kwh;
                v_ev_core = e.v_core_eur;
                ev_budget_eur = e.budget_eur;
                soc_ev_init = assets.assets.get("ev").and_then(|s| s.val("soc"));
            }
            AssetMilpParams::Heater(h) => {
                heater_mode = h.mode;
                t_heat_dead = h.t_dead_step;
                p_mid = h.p_mid_kw;
                p_full = h.p_full_kw;
                e_heat_init = h.e_init_kwh;
                e_heat_max = h.e_max_kwh;
                q_heat_dem = h.q_dem_kw;
                e_heat_target = h.e_target_kwh;
                lambda_sw = h.lambda_sw_eur;
                heat_iz_mid = h.initial_z_mid;
                heat_iz_full = h.initial_z_full;
            }
            AssetMilpParams::Unknown => {}
        }
    }

    // Maps a non-negative offset_s to the latest slot index t where cum_s[t] <= offset_s.
    let time_to_slot = |offset_s: i64| -> usize {
        cum_s
            .partition_point(|&s| s <= offset_s)
            .saturating_sub(1)
            .min(n.saturating_sub(1))
    };

    // ── Baseline override: additive per-slot kW adjustments ─────────────────
    if let Some(bo) = baseline_override {
        for slot in &bo.slots {
            let offset_s = (slot.slot_start - now).num_seconds();
            if offset_s < 0 {
                continue;
            }
            let idx = time_to_slot(offset_s);
            if idx < n {
                p_base[idx] += slot.add_kw;
            }
        }
    }

    // ── Shiftable loads → ShiftableLoadMilp ──────────────────────────────────
    let milp_loads: Vec<ShiftableLoadMilp> = shiftable_loads.iter().filter_map(|sl| {
        let dur_s = (sl.duration_min as i64) * 60;
        let duration_slots = (1..=n).find(|&k| cum_s[k] >= dur_s).unwrap_or(n);
        if duration_slots == 0 { return None; }

        let window_start_s = (sl.earliest_start - now).num_seconds().max(0);
        let window_end_s = (sl.latest_end - now).num_seconds().max(0);

        let first_slot = time_to_slot(window_start_s);
        // Last valid start: load must finish before latest_end
        let last_valid_s = window_end_s - dur_s;
        if last_valid_s < 0 {
            tracing::warn!(
                asset_id = %sl.asset_id,
                window_end_s,
                dur_s,
                "shiftable load window expired before planner ran — skipped"
            );
            return None;
        }
        let last_slot = time_to_slot(last_valid_s).min(n.saturating_sub(duration_slots));

        let valid_start_slots: Vec<usize> = (first_slot..=last_slot).filter(|&s| s + duration_slots <= n).collect();
        if valid_start_slots.is_empty() {
            tracing::warn!(asset_id = %sl.asset_id, "shiftable load has no valid start slots in horizon — skipped");
            return None;
        }

        Some(ShiftableLoadMilp {
            asset_id: sl.asset_id.clone(),
            power_kw: sl.power_kw,
            duration_slots,
            valid_start_slots,
        })
    }).collect();

    // WP4.1-c: even at the cheapest slot rate the target energy (all "extra"
    // headroom in MAX_COST mode) exceeds the budget → charging will stop
    // early. Stable text — WP4.3's notification dedup keys on it.
    let budget_warning = ev_budget_eur.and_then(|budget_eur| {
        let min_rate = c_imp.iter().cloned().fold(f64::INFINITY, f64::min);
        (e_ev_extra * min_rate > budget_eur).then(|| {
            "EV charging budget too low to reach the session target — \
             charging stops at the budget (MAX_COST)"
                .to_string()
        })
    });

    MilpInputs {
        n,
        dt_h,
        cum_s,
        c_imp_eur_kwh: c_imp,
        rate_stale: stale_outcome.rate_stale,
        stale_rate_warning: stale_outcome.warning,
        budget_warning,
        c_exp_eur_kwh: c_exp,
        g_imp_kgco2_kwh: g_co2,
        p_pv_kw: p_pv,
        p_base_kw: p_base,
        p_residual_kw: p_residual,
        p_imp_max_phys_kw: p_imp_phys,
        p_exp_max_phys_kw: p_exp_phys,
        p_imp_max_cont_kw: p_imp_cont,
        p_exp_max_cont_kw: p_exp_cont,
        pen_imp_eur_kwh: planner.pen_imp_eur_kwh,
        pen_exp_eur_kwh: planner.pen_exp_eur_kwh,
        e_bat_nom_kwh: e_bat_nom,
        e_bat_init_kwh: e_bat_init,
        e_bat_min_kwh: e_bat_min,
        e_bat_max_kwh: e_bat_max,
        p_bat_ch_max_kw: p_bat_ch_max,
        p_bat_dis_max_kw: p_bat_dis_max,
        eff_bat_ch: eff_ch,
        eff_bat_dis: eff_dis,
        a_ev,
        ev_mode,
        t_ev_dead_step: t_ev_dead,
        p_ev_max_kw: p_ev_max,
        p_ev_min_kw: p_ev_min,
        e_ev_core_kwh: e_ev_core,
        e_ev_extra_max_kwh: e_ev_extra,
        v_ev_core_eur: v_ev_core,
        v_ev_extra_eur_kwh: v_ev_extra,
        heater_mode,
        t_heat_dead_step: t_heat_dead,
        p_heat_mid_kw: p_mid,
        p_heat_full_kw: p_full,
        e_heat_init_kwh: e_heat_init,
        e_heat_max_kwh: e_heat_max,
        q_heat_dem_kw: q_heat_dem,
        e_heat_target_kwh: e_heat_target,
        lambda_heat_sw_eur: lambda_sw,
        w_tier_penalty_eur: planner.w_tier_penalty_eur,
        heat_initial_z_mid: heat_iz_mid,
        heat_initial_z_full: heat_iz_full,
        shiftable_loads: milp_loads,
        soc_ev_init,
    }
}
