use chrono::{DateTime, Duration, Utc};

use super::asset_port::AssetMilpParams;
use crate::assets::{base_load::BaseLoadParams, pv::PvParams};
use crate::controller::milp_planner::AssetMilpContext;
use crate::controller::simulator_port::SimSnapshot;
use crate::entities::capacity::OadrCapacityState;
use crate::entities::device_session::{BaselineOverride, ShiftableLoad};
use crate::entities::planner_params::PlannerParams;
use crate::entities::tariff_snapshot::TariffTimeSeries;

use super::types::*;

/// Natural sin-model irradiance [0,1] at time `ts`, without any user offset.
/// Mirrors `PvInverter::natural_irradiance_at()` from `assets/pv.rs`.
fn pv_natural_irradiance(ts: DateTime<Utc>) -> f64 {
    use chrono::Timelike;
    let hour = ts.hour() as f64 + ts.minute() as f64 / 60.0;
    if hour >= 6.0 && hour <= 18.0 {
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
pub(crate) fn build_milp_inputs(
    asset_contexts: &[Box<dyn AssetMilpContext>],
    assets: &SimSnapshot,
    tariffs: &TariffTimeSeries,
    capacity: &OadrCapacityState,
    planner: &PlannerParams,
    phys_imp: f64,
    phys_exp: f64,
    pv_cfg: Option<&PvParams>,
    base_load: Option<&BaseLoadParams>,
    now: DateTime<Utc>,
    shiftable_loads: &[ShiftableLoad],
    baseline_override: Option<&BaselineOverride>,
    pv_forecast_override: Option<f64>,
) -> MilpInputs {
    let step_s = planner.plan_step_s;
    let n = ((planner.plan_horizon_h as f64 * 3600.0) / step_s as f64) as usize;
    let dt_h = step_s as f64 / 3600.0;

    // ── Per-step grid arrays ──────────────────────────────────────────────────
    let cont_imp = capacity.import_limit_kw.unwrap_or(phys_imp);
    let cont_exp = capacity.export_limit_kw.unwrap_or(phys_exp);
    let base_kw = base_load.map(|c| c.baseline_kw).unwrap_or(0.0);

    let mut c_imp = Vec::with_capacity(n);
    let mut c_exp = Vec::with_capacity(n);
    let mut g_co2 = Vec::with_capacity(n);
    let mut p_pv = Vec::with_capacity(n);
    let mut p_base = Vec::with_capacity(n);
    let mut p_imp_phys = Vec::with_capacity(n);
    let mut p_exp_phys = Vec::with_capacity(n);
    let mut p_imp_cont = Vec::with_capacity(n);
    let mut p_exp_cont = Vec::with_capacity(n);

    for t in 0..n {
        let slot_t = now + Duration::seconds(t as i64 * step_s as i64);
        c_imp.push(
            tariffs
                .import_eur_kwh
                .interpolate_at(slot_t)
                .unwrap_or(0.25),
        );
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
            // pv_alpha is "fraction removed per plan step (300 s)".
            // Exponent is the number of plan steps ahead, not raw seconds.
            let decayed_offset = irradiance_offset * (1.0 - pv_alpha).powf(t as f64);
            (natural + decayed_offset).clamp(0.0, 1.0) * rated_kw
        } else {
            pv_cfg.map(|c| c.forecast_kw(slot_t)).unwrap_or(0.0)
        };
        p_pv.push(pv_kw);
        p_base.push(base_kw);
        p_imp_phys.push(phys_imp);
        p_exp_phys.push(phys_exp);
        p_imp_cont.push(cont_imp);
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
        match ctx.milp_params(n, step_s, now) {
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

    // ── Baseline override: additive per-slot kW adjustments ─────────────────
    if let Some(bo) = baseline_override {
        for slot in &bo.slots {
            let offset_s = (slot.slot_start - now).num_seconds();
            if offset_s < 0 {
                continue;
            }
            let idx = (offset_s as f64 / (dt_h * 3600.0)).floor() as usize;
            if idx < n {
                p_base[idx] += slot.add_kw;
            }
        }
    }

    // ── Shiftable loads → ShiftableLoadMilp ──────────────────────────────────
    let milp_loads: Vec<ShiftableLoadMilp> = shiftable_loads.iter().filter_map(|sl| {
        let dur_s = (sl.duration_min as f64) * 60.0;
        let duration_slots = (dur_s / (dt_h * 3600.0)).ceil() as usize;
        if duration_slots == 0 { return None; }

        let window_start_s = (sl.earliest_start - now).num_seconds().max(0) as f64;
        let window_end_s = (sl.latest_end - now).num_seconds().max(0) as f64;

        let first_slot = (window_start_s / (dt_h * 3600.0)).floor() as usize;
        // Last valid start: load must finish before latest_end
        let last_valid_s = window_end_s - dur_s;
        if last_valid_s < 0.0 {
            tracing::warn!(
                asset_id = %sl.asset_id,
                window_end_s,
                dur_s,
                "shiftable load window expired before planner ran — skipped"
            );
            return None;
        }
        let last_slot = ((last_valid_s / (dt_h * 3600.0)).floor() as usize).min(n.saturating_sub(duration_slots));

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

    MilpInputs {
        n,
        dt_h,
        c_imp_eur_kwh: c_imp,
        c_exp_eur_kwh: c_exp,
        g_imp_kgco2_kwh: g_co2,
        p_pv_kw: p_pv,
        p_base_kw: p_base,
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
        v_ev_core_eur: 0.0,
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
