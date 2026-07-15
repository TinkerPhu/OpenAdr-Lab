use super::schema::{GridConfig, HeaterConfig, PlannerConfig, SimulatorConfig};
use crate::entities::PlannerObjective;

pub(super) fn default_asset_id_ev() -> String {
    crate::ids::ASSET_EV.to_string()
}

pub(super) fn default_ev_max_charge() -> f64 {
    7.4
}
pub(super) fn default_ev_max_discharge() -> f64 {
    0.0
}
pub(super) fn default_ev_soc() -> f64 {
    0.5
}
pub(super) fn default_ev_battery() -> f64 {
    60.0
}
pub(super) fn default_ev_soc_target() -> f64 {
    0.8
}
pub(super) fn default_ev_min_charge() -> f64 {
    1.4
}
pub(super) fn default_ev_response_delay() -> f64 {
    10.0
}

pub(super) fn default_history_enabled() -> bool {
    true
}
pub(super) fn default_history_retention_days() -> u32 {
    90
}

impl HeaterConfig {
    /// Effective thermal mass (kWh/°C).
    /// Priority: `volume_l` → `thermal_mass_kwh_per_c` → 2.0 (legacy default).
    pub fn effective_thermal_mass(&self) -> f64 {
        if let Some(v) = self.volume_l {
            v * 4.186 / 3600.0
        } else {
            self.thermal_mass_kwh_per_c.unwrap_or(2.0)
        }
    }

    /// Effective Newton cooling coefficient (kW/°C).
    pub fn effective_k_loss(&self) -> f64 {
        self.k_loss_kw_per_c.unwrap_or(0.1)
    }

    /// Effective constant hot water draw (kW thermal).
    pub fn effective_draw_kw(&self) -> f64 {
        self.draw_kw.unwrap_or(0.0)
    }

    /// Relay switching penalty coefficient [EUR/switch event] for the MILP objective.
    pub fn effective_switching_penalty(&self) -> f64 {
        self.switching_penalty_eur.unwrap_or(0.01)
    }
}

pub(super) fn default_asset_id_heater() -> String {
    crate::ids::ASSET_HEATER.to_string()
}

pub(super) fn default_heater_max() -> f64 {
    5.0
}
pub(super) fn default_heater_temp() -> f64 {
    20.0
}
pub(super) fn default_heater_min() -> f64 {
    18.0
}
pub(super) fn default_heater_max_temp() -> f64 {
    23.0
}

pub(super) fn default_asset_id_pv() -> String {
    crate::ids::ASSET_PV.to_string()
}
pub(super) fn default_pv_rated() -> f64 {
    5.0
}

pub(super) fn default_asset_id_battery() -> String {
    crate::ids::ASSET_BATTERY.to_string()
}

pub(super) fn default_battery_capacity() -> f64 {
    10.0
}
pub(super) fn default_battery_charge() -> f64 {
    5.0
}
pub(super) fn default_battery_discharge() -> f64 {
    5.0
}
pub(super) fn default_battery_soc() -> f64 {
    0.5
}
pub(super) fn default_battery_efficiency() -> f64 {
    0.92
}
pub(super) fn default_battery_min_soc() -> f64 {
    0.10
}

pub(super) fn default_asset_id_base_load() -> String {
    crate::ids::ASSET_BASE_LOAD.to_string()
}
pub(super) fn default_base_load_kw() -> f64 {
    0.5
}

pub(super) fn default_spike_jitter_h() -> f64 {
    0.25
}
pub(super) fn default_spike_duration_h() -> f64 {
    0.5
}
pub(super) fn default_spike_ramp_h() -> f64 {
    0.05
}
pub(super) fn default_spike_probability() -> f64 {
    1.0
}

impl Default for SimulatorConfig {
    fn default() -> Self {
        Self {
            tick_s: default_tick(),
            persist_every_s: default_persist_every(),
            report_interval_s: default_report_interval(),
        }
    }
}

pub(super) fn default_tick() -> u64 {
    1
}
pub(super) fn default_persist_every() -> u64 {
    15
}
pub(super) fn default_report_interval() -> u64 {
    60
}

pub(super) fn default_max_import_kw() -> f64 {
    25.0
}
pub(super) fn default_max_export_kw() -> f64 {
    10.0
}

impl Default for GridConfig {
    fn default() -> Self {
        Self {
            max_import_kw: default_max_import_kw(),
            max_export_kw: default_max_export_kw(),
        }
    }
}

impl PlannerConfig {
    /// Effective slot width: zone[0].step_s when plan_zones is set, else plan_step_s.
    pub fn effective_step_s(&self) -> u64 {
        self.plan_zones
            .as_ref()
            .and_then(|z| z.first())
            .map(|z| z.step_s)
            .unwrap_or(self.plan_step_s)
    }

    /// Effective horizon: sum(step_s × slots) / 3600 when plan_zones is set, else plan_horizon_h.
    pub fn effective_horizon_h(&self) -> u64 {
        self.plan_zones
            .as_ref()
            .filter(|z| !z.is_empty())
            .map(|zones| zones.iter().map(|z| z.step_s * z.slots as u64).sum::<u64>() / 3600)
            .unwrap_or(self.plan_horizon_h)
    }
}

impl Default for PlannerConfig {
    fn default() -> Self {
        Self {
            plan_zones: None,
            plan_step_s: default_plan_step(),
            plan_horizon_h: default_plan_horizon_h(),
            replan_interval_s: default_replan_interval(),
            w_energy: default_w_energy(),
            w_ghg: default_w_ghg(),
            w_grid: 0.0,
            c_bat_wear_eur_kwh: default_bat_wear(),
            c_ev_startup_eur: default_ev_startup(),
            c_bat_startup_eur: default_bat_startup(),
            c_ev_ramp_eur_kw: default_ev_ramp(),
            c_bat_ramp_eur_kw: default_bat_ramp(),
            c_bat_ev_coexist_eur_kwh: default_bat_ev_coexist(),
            w_viol: default_w_viol(),
            pen_imp_eur_kwh: default_pen_imp(),
            pen_exp_eur_kwh: default_pen_exp(),
            v_ev_extra_eur_kwh: default_v_ev_extra(),
            v_ev_core_eur_kwh: default_v_ev_core(),
            w_tier_penalty_eur: default_w_tier_penalty(),
            c_ctrl_imp_malus_eur_kwh: default_c_ctrl_imp_malus(),
            objective: PlannerObjective::MinCost,
            plan_adoption_threshold_eur: default_plan_adoption_threshold(),
            plan_adoption_decay_s: default_plan_adoption_decay(),
            phase2_epsilon_eur: default_phase2_epsilon(),
            solver_timeout_s: default_solver_timeout_s(),
            planning_initial_delay_s: default_planning_initial_delay_s(),
            gate_switch_penalty_eur: 0.0,
            simple_level1_import_cap_pct: default_simple_level1_import_cap_pct(),
            asap_lateness_eur_kwh_h: default_asap_lateness_eur_kwh_h(),
            v_ev_free_charge_eur_kwh: default_v_ev_free_charge(),
            stale_rate_policy: default_stale_rate_policy(),
            stale_rate_safe_pctl: default_stale_rate_safe_pctl(),
        }
    }
}

pub(super) fn default_simple_level1_import_cap_pct() -> f64 {
    0.5
}

pub(super) fn default_asap_lateness_eur_kwh_h() -> f64 {
    10.0
}

pub(super) fn default_v_ev_free_charge() -> f64 {
    0.10
}

pub(super) fn default_stale_rate_policy() -> crate::entities::design_vocabulary::StaleRatePolicy {
    crate::entities::design_vocabulary::StaleRatePolicy::HeuristicForecast
}

pub(super) fn default_stale_rate_safe_pctl() -> f64 {
    0.8
}

pub(super) fn default_phase2_epsilon() -> f64 {
    0.02
}
pub(super) fn default_plan_adoption_threshold() -> f64 {
    0.20
}
pub(super) fn default_plan_adoption_decay() -> f64 {
    1500.0
}
pub(super) fn default_solver_timeout_s() -> u64 {
    60
}
pub(super) fn default_planning_initial_delay_s() -> u64 {
    5
}

pub(super) fn default_plan_step() -> u64 {
    600
}
pub(super) fn default_plan_horizon_h() -> u64 {
    48
}
pub(super) fn default_replan_interval() -> u64 {
    300
}
pub(super) fn default_w_energy() -> f64 {
    1.0
}
pub(super) fn default_w_ghg() -> f64 {
    0.0001
}
pub(super) fn default_bat_wear() -> f64 {
    0.03
}
pub(super) fn default_ev_startup() -> f64 {
    0.01
}
pub(super) fn default_bat_startup() -> f64 {
    0.01
}
pub(super) fn default_ev_ramp() -> f64 {
    0.005
}
pub(super) fn default_bat_ramp() -> f64 {
    0.005
}
pub(super) fn default_bat_ev_coexist() -> f64 {
    0.5
}
pub(super) fn default_w_viol() -> f64 {
    1.0
}
pub(super) fn default_v_ev_extra() -> f64 {
    0.10
}
pub(super) fn default_v_ev_core() -> f64 {
    1.0
}
pub(super) fn default_w_tier_penalty() -> f64 {
    0.001
}
pub(super) fn default_c_ctrl_imp_malus() -> f64 {
    0.22
}
pub(super) fn default_pen_imp() -> f64 {
    10_000.0
}
pub(super) fn default_pen_exp() -> f64 {
    10_000.0
}
