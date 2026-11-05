//! The exogenous forecasts of a site and the contract they travel in.
//!
//! Owns: the PV forecast from weather (transposition, cell temperature, snow cover), the base-load
//! forecast learned from history, and the payload every forecast topic carries
//! (`openspec/changes/lab-forecaster`). Reads: nothing itself; callers inject weather, measurements
//! and a `TickSource`. Must never: touch a clock, a socket or a file, or import a VEN type. The VEN
//! asset and the forecaster container both call these functions; neither keeps a copy.

pub mod forecast_payload;
pub mod pv_params;
pub mod pv_profile;
pub mod pv_snow;
pub mod pv_snow_observation;
pub mod solar;
pub mod weather;
pub mod weather_wire;
