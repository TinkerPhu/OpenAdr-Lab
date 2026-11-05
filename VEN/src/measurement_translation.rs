//! The measurement-payload translation lives in `lab_forecast::measurement_wire`, shared with the
//! forecaster container; this module keeps the VEN's path to it.

pub use lab_forecast::measurement_wire::{parse_base_load_measurement, parse_pv_measurement};
