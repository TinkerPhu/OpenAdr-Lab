use crate::assets::{
    base_load::BaseLoadParams, battery::BatteryParams, ev::EvParams, heater::HeaterParams,
    pv::PvParams,
};
use crate::entities::asset::{ComfortRate, CompletionPolicy};

#[derive(Debug, Clone)]
pub enum AssetParams {
    Battery(BatteryParams),
    Ev(EvParams),
    Heater(HeaterParams),
    Pv(PvParams),
    BaseLoad(BaseLoadParams),
}

/// Minimal asset snapshot for user-request creation.
/// Built by the adapter layer (routes/hems.rs) from a locked SimState.
/// Pure domain type — no assets/ or simulator/ imports.
#[derive(Debug, Clone)]
pub struct AssetRequestSlice {
    pub id: String,
    /// Current SoC [0.0, 1.0] for storage assets; None for non-storage.
    pub current_soc: Option<f64>,
    /// Default SoC target when body.target_soc is None.
    pub default_soc_target: Option<f64>,
    /// Usable capacity in kWh; None for non-storage assets.
    pub capacity_kwh: Option<f64>,
    /// Max charge rate (kW); used as default desired_power when not specified.
    pub max_charge_kw: Option<f64>,
    pub completion_policy: CompletionPolicy,
    pub comfort_rates: Vec<ComfortRate>,
}

impl AssetRequestSlice {
    pub fn resolve_request_target(
        &self,
        target_soc: Option<f64>,
        desired_power_kw: Option<f64>,
    ) -> Option<(f64, f64)> {
        let current_soc = self.current_soc?;
        let capacity_kwh = self.capacity_kwh?;
        let target = target_soc.or(self.default_soc_target).unwrap_or(1.0);
        let kwh = (target - current_soc).max(0.0) * capacity_kwh;
        if kwh < 1e-6 {
            return None;
        }
        Some((kwh, desired_power_kw.or(self.max_charge_kw).unwrap_or(1.0)))
    }
}
