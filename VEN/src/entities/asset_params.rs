use crate::assets::{
    base_load::BaseLoadParams, battery::BatteryParams, ev::EvParams, heater::HeaterParams,
    pv::PvParams,
};

#[derive(Debug, Clone)]
pub enum AssetParams {
    Battery(BatteryParams),
    Ev(EvParams),
    Heater(HeaterParams),
    Pv(PvParams),
    BaseLoad(BaseLoadParams),
}
