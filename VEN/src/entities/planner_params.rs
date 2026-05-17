use serde::{Deserialize, Deserializer, Serialize, Serializer};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PlannerObjective {
    #[default]
    MinCost,
    MinGhg,
    MinGrid,
    MinImport,
    MaxRevenue,
    Custom,
}

#[derive(Debug, Clone)]
pub struct PlannerParams {
    pub plan_step_s: u64,
    pub plan_horizon_h: u64,
    pub replan_interval_s: u64,
    // Reserved for apply_battery_correction_overlay integration — not yet read in production.
    #[allow(dead_code)]
    pub deviation_threshold_kw: f64,
    #[allow(dead_code)]
    pub deviation_trigger_ticks: u32,
    #[allow(dead_code)]
    pub correction_min_kw: f64,
    pub w_energy: f64,
    pub w_ghg: f64,
    pub w_grid: f64,
    pub c_bat_wear_eur_kwh: f64,
    pub c_ev_startup_eur: f64,
    pub c_bat_startup_eur: f64,
    pub c_ev_ramp_eur_kw: f64,
    pub c_bat_ramp_eur_kw: f64,
    pub c_bat_ev_coexist_eur_kwh: f64,
    pub w_viol: f64,
    pub pen_imp_eur_kwh: f64,
    pub pen_exp_eur_kwh: f64,
    pub v_ev_extra_eur_kwh: f64,
    pub w_tier_penalty_eur: f64,
    pub objective: PlannerObjective,
    pub plan_adoption_threshold_eur: f64,
    pub plan_adoption_decay_s: f64,
    pub phase2_epsilon_eur: f64,
    pub solver_timeout_s: u64,
    pub planning_initial_delay_s: u64,
}

impl Default for PlannerParams {
    fn default() -> Self {
        Self {
            plan_step_s: 300,
            plan_horizon_h: 24,
            replan_interval_s: 300,
            deviation_threshold_kw: 1.0,
            deviation_trigger_ticks: 30,
            correction_min_kw: 0.2,
            w_energy: 1.0,
            w_ghg: 0.0001,
            w_grid: 0.0,
            c_bat_wear_eur_kwh: 0.03,
            c_ev_startup_eur: 0.01,
            c_bat_startup_eur: 0.01,
            c_ev_ramp_eur_kw: 0.005,
            c_bat_ramp_eur_kw: 0.005,
            c_bat_ev_coexist_eur_kwh: 0.5,
            w_viol: 1.0,
            pen_imp_eur_kwh: 10_000.0,
            pen_exp_eur_kwh: 10_000.0,
            v_ev_extra_eur_kwh: 0.10,
            w_tier_penalty_eur: 0.001,
            objective: PlannerObjective::MinCost,
            plan_adoption_threshold_eur: 0.0,
            plan_adoption_decay_s: 0.0,
            phase2_epsilon_eur: 0.02,
            solver_timeout_s: 60,
            planning_initial_delay_s: 5,
        }
    }
}

#[derive(Debug, Clone)]
pub struct AbsorberAssetParams {
    pub id: String,
    pub priority: u8,
    pub min_state_linger_s: u64,
    pub ev_departure_guard_s: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct AbsorberParams {
    pub enabled: bool,
    pub dead_band_kw: f64,
    pub dead_band_clearing_ticks: usize,
    pub deviation_trigger_ticks: u32,
    pub assets: Vec<AbsorberAssetParams>,
}

impl Default for AbsorberParams {
    fn default() -> Self {
        Self {
            enabled: false,
            dead_band_kw: 0.1,
            dead_band_clearing_ticks: 1,
            deviation_trigger_ticks: 30,
            assets: Vec::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct SimulatorParams {
    pub tick_s: u64,
    pub persist_every_s: u64,
    pub report_interval_s: u64,
}

impl Default for SimulatorParams {
    fn default() -> Self {
        Self {
            tick_s: 1,
            persist_every_s: 15,
            report_interval_s: 60,
        }
    }
}

impl PlannerObjective {
    fn as_str(self) -> &'static str {
        match self {
            Self::MinCost => "min_cost",
            Self::MinGhg => "min_ghg",
            Self::MinGrid => "min_grid",
            Self::MinImport => "min_import",
            Self::MaxRevenue => "max_revenue",
            Self::Custom => "custom",
        }
    }
}

impl Serialize for PlannerObjective {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str((*self).as_str())
    }
}

impl<'de> Deserialize<'de> for PlannerObjective {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        match value.as_str() {
            "min_cost" => Ok(Self::MinCost),
            "min_ghg" => Ok(Self::MinGhg),
            "min_grid" => Ok(Self::MinGrid),
            "min_import" => Ok(Self::MinImport),
            "max_revenue" => Ok(Self::MaxRevenue),
            "custom" => Ok(Self::Custom),
            other => Err(serde::de::Error::unknown_variant(
                other,
                &[
                    "min_cost",
                    "min_ghg",
                    "min_grid",
                    "min_import",
                    "max_revenue",
                    "custom",
                ],
            )),
        }
    }
}
