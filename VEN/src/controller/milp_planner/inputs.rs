use chrono::{DateTime, Duration, Utc};

use super::asset_port::{BatteryMilpContext, EvMilpContext, EvMilpMode, HeaterMilpContext, HeaterMilpMode};
use crate::controller::simulator_port::SimSnapshot;
use crate::entities::capacity::OadrCapacityState;
use crate::entities::device_session::{BaselineOverride, ShiftableLoad};
use crate::entities::tariff_snapshot::TariffTimeSeries;
use crate::profile::Profile;

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

/// Build the full MILP input parameter set from the current runtime state.
///
/// All transformations — CO₂ unit conversion (g→kg), √RTE efficiency split,
/// live battery SoC, EV horizon mask, and LoadMode translation — happen here.
/// The resulting `MilpInputs` is ready to pass directly to `solve_milp_two_phase()`.
pub(crate) fn build_milp_inputs(
    assets: &SimSnapshot,
    tariffs: &TariffTimeSeries,
    capacity: &OadrCapacityState,
    profile: &Profile,
    now: DateTime<Utc>,
    ev_session: Option<&crate::entities::device_session::EvSession>,
    heater_target: Option<&crate::entities::device_session::HeaterTarget>,
    shiftable_loads: &[ShiftableLoad],
    baseline_override: Option<&BaselineOverride>,
) -> MilpInputs {
    let step_s = profile.planner.plan_step_s;
    let n = ((profile.planner.plan_horizon_h as f64 * 3600.0) / step_s as f64) as usize;
    let dt_h = step_s as f64 / 3600.0;

    // ── Per-step grid arrays ──────────────────────────────────────────────────
    let phys_imp = profile.grid.max_import_kw;
    let phys_exp = profile.grid.max_export_kw;
    let cont_imp = capacity.import_limit_kw.unwrap_or(phys_imp);
    let cont_exp = capacity.export_limit_kw.unwrap_or(phys_exp);
    let pv_cfg = profile.pv_config();
    let base_kw = profile.base_load_kw();

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
        let pv_kw = if let Some(pv_snap) = assets.assets.get("pv") {
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

    // ── Battery ───────────────────────────────────────────────────────────────
    let (e_bat_nom, e_bat_init, e_bat_min, e_bat_max, p_bat_ch_max, p_bat_dis_max, eff_ch, eff_dis) =
        if let Some(bat_cfg) = profile.battery_config() {
            let ctx = if let Some(bat_snap) = assets.assets.get("battery") {
                let soc = bat_snap.val("soc").unwrap_or(bat_cfg.initial_soc);
                let cap = bat_cfg.capacity_kwh;
                let eff = bat_cfg.round_trip_efficiency.sqrt();
                BatteryMilpContext {
                    e_nom_kwh: cap,
                    e_init_kwh: soc * cap,
                    e_min_kwh: bat_cfg.min_soc * cap,
                    e_max_kwh: cap,
                    p_ch_max_kw: bat_cfg.max_charge_kw,
                    p_dis_max_kw: bat_cfg.max_discharge_kw,
                    eff_ch: eff,
                    eff_dis: eff,
                }
            } else {
                // No live battery in sim (e.g. cleared in test): fall back to profile initial_soc
                let cap = bat_cfg.capacity_kwh;
                let eff = bat_cfg.round_trip_efficiency.sqrt();
                BatteryMilpContext {
                    e_nom_kwh: cap,
                    e_init_kwh: bat_cfg.initial_soc * cap,
                    e_min_kwh: bat_cfg.min_soc * cap,
                    e_max_kwh: cap,
                    p_ch_max_kw: bat_cfg.max_charge_kw,
                    p_dis_max_kw: bat_cfg.max_discharge_kw,
                    eff_ch: eff,
                    eff_dis: eff,
                }
            };
            (
                Some(ctx.e_nom_kwh),
                Some(ctx.e_init_kwh),
                Some(ctx.e_min_kwh),
                Some(ctx.e_max_kwh),
                Some(ctx.p_ch_max_kw),
                Some(ctx.p_dis_max_kw),
                Some(ctx.eff_ch),
                Some(ctx.eff_dis),
            )
        } else {
            (None, None, None, None, None, None, None, None)
        };

    // ── EV ────────────────────────────────────────────────────────────────────
    let (
        a_ev,
        ev_mode,
        t_ev_dead,
        p_ev_max,
        p_ev_min,
        e_ev_core,
        e_ev_extra,
        v_ev_extra,
        soc_ev_init,
    ) = if let Some(ev_cfg) = profile.ev_config() {
        let (ctx, soc_init) = if let Some(ev_snap) = assets.assets.get("ev") {
            let plugged = ev_snap.val("plugged").map(|v| v > 0.5).unwrap_or(false);
            let soc = ev_snap.val("soc").unwrap_or(ev_cfg.initial_soc);
            (
                EvMilpContext::from_live(
                    plugged,
                    soc,
                    ev_cfg.max_charge_kw,
                    ev_cfg.battery_kwh,
                    ev_cfg.soc_target,
                    ev_cfg.min_charge_kw,
                    profile.planner.v_ev_extra_eur_kwh,
                    n,
                    step_s,
                    now,
                    ev_session,
                ),
                Some(soc),
            )
        } else {
            (
                EvMilpContext {
                    mode: EvMilpMode::MustNotRun,
                    a_ev: vec![false; n],
                    t_dead_step: None,
                    p_max_kw: ev_cfg.max_charge_kw,
                    p_min_kw: ev_cfg.min_charge_kw,
                    e_core_kwh: 0.0,
                    e_extra_max_kwh: ev_cfg.battery_kwh * (1.0 - ev_cfg.soc_target),
                    v_extra_eur_kwh: profile.planner.v_ev_extra_eur_kwh,
                },
                None,
            )
        };
        let ev_mode = match ctx.mode {
            EvMilpMode::MustRun => MilpLoadMode::MustRun,
            EvMilpMode::MayRun => MilpLoadMode::MayRun,
            EvMilpMode::MustNotRun => MilpLoadMode::MustNotRun,
        };
        (
            ctx.a_ev,
            ev_mode,
            ctx.t_dead_step,
            ctx.p_max_kw,
            ctx.p_min_kw,
            ctx.e_core_kwh,
            ctx.e_extra_max_kwh,
            ctx.v_extra_eur_kwh,
            soc_init,
        )
    } else {
        // No EV asset in profile
        (
            vec![false; n],
            MilpLoadMode::MustNotRun,
            None,
            0.0,
            0.0,
            0.0,
            0.0,
            0.0,
            None,
        )
    };

    // ── Heater ────────────────────────────────────────────────────────────────
    let (
        heater_mode,
        t_heat_dead,
        p_mid,
        p_full,
        e_heat_init,
        e_heat_max,
        q_heat_dem,
        e_heat_target,
        lambda_sw,
        heat_iz_mid,
        heat_iz_full,
    ) = if let Some(heat_cfg) = profile.heater_config() {
        let lambda = heat_cfg.effective_switching_penalty();
        let ctx = if let Some(h_snap) = assets.assets.get("heater") {
            let temp_c = h_snap.val("temp_c")
                .unwrap_or((heat_cfg.temp_min_c + heat_cfg.temp_max_c) / 2.0);
            let thermal_mass = heat_cfg.effective_thermal_mass();
            let t_mid = (heat_cfg.temp_min_c + heat_cfg.temp_max_c) / 2.0;
            let ambient = 10.0;
            let q_dem = (heat_cfg.effective_draw_kw()
                + heat_cfg.effective_k_loss() * (t_mid - ambient))
                .max(0.0);
            HeaterMilpContext::from_live(
                temp_c,
                h_snap.power_kw,
                heat_cfg.temp_min_c,
                heat_cfg.temp_max_c,
                heat_cfg.mid_kw.unwrap_or(0.0),
                heat_cfg.max_kw,
                thermal_mass,
                q_dem,
                lambda,
                n,
                step_s,
                now,
                heater_target,
            )
        } else {
            // No live heater in sim: reconstruct from profile config
                let thermal_mass = heat_cfg.effective_thermal_mass();
                let ambient = 10.0;
                let live_t_min = heat_cfg.temp_min_c;
                let live_t_max = heat_cfg.temp_max_c;
                let live_max_kw = heat_cfg.max_kw;
                let live_mid_kw = heat_cfg.mid_kw.unwrap_or(live_max_kw / 2.0);
                let current_temp = heat_cfg.temp_initial_c;
                let e_init = (current_temp - live_t_min) * thermal_mass;
                let e_max = ((live_t_max - live_t_min) * thermal_mass).max(0.0);
                let t_mid = (live_t_min + live_t_max) / 2.0;
                let q_dem = (heat_cfg.effective_draw_kw()
                    + heat_cfg.effective_k_loss() * (t_mid - ambient))
                    .max(0.0);
                if let Some(target) = heater_target {
                    let e_target =
                        ((target.target_temp_c - live_t_min) * thermal_mass).clamp(0.0, e_max);
                    let secs = (target.ready_by - now).num_seconds();
                    let t_dead =
                        (secs / step_s as i64).clamp(0, (n.saturating_sub(1)) as i64) as usize;
                    HeaterMilpContext {
                        mode: HeaterMilpMode::MustRun,
                        t_dead_step: Some(t_dead),
                        p_mid_kw: live_mid_kw,
                        p_full_kw: live_max_kw,
                        e_init_kwh: e_init,
                        e_max_kwh: e_max,
                        q_dem_kw: q_dem,
                        e_target_kwh: e_target,
                        lambda_sw_eur: lambda,
                        initial_z_mid: 0.0,
                        initial_z_full: 0.0,
                    }
                } else {
                    HeaterMilpContext {
                        mode: HeaterMilpMode::MayRun,
                        t_dead_step: None,
                        p_mid_kw: live_mid_kw,
                        p_full_kw: live_max_kw,
                        e_init_kwh: e_init,
                        e_max_kwh: e_max,
                        q_dem_kw: q_dem,
                        e_target_kwh: e_max,
                        lambda_sw_eur: lambda,
                        initial_z_mid: 0.0,
                        initial_z_full: 0.0,
                    }
                }
        };
        let heater_mode = match ctx.mode {
            HeaterMilpMode::MustRun => MilpLoadMode::MustRun,
            HeaterMilpMode::MayRun => MilpLoadMode::MayRun,
            HeaterMilpMode::MustNotRun => MilpLoadMode::MustNotRun,
        };
        (
            heater_mode,
            ctx.t_dead_step,
            ctx.p_mid_kw,
            ctx.p_full_kw,
            ctx.e_init_kwh,
            ctx.e_max_kwh,
            ctx.q_dem_kw,
            ctx.e_target_kwh,
            ctx.lambda_sw_eur,
            ctx.initial_z_mid,
            ctx.initial_z_full,
        )
    } else {
        (
            MilpLoadMode::MustNotRun,
            None,
            0.0,
            0.0,
            0.0,
            0.0,
            0.0,
            0.0,
            0.0,
            0.0,
            0.0,
        )
    };

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
        pen_imp_eur_kwh: profile.planner.pen_imp_eur_kwh,
        pen_exp_eur_kwh: profile.planner.pen_exp_eur_kwh,
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
        w_tier_penalty_eur: profile.planner.w_tier_penalty_eur,
        heat_initial_z_mid: heat_iz_mid,
        heat_initial_z_full: heat_iz_full,
        shiftable_loads: milp_loads,
        soc_ev_init,
    }
}
