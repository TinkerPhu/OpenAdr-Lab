//! The measurement reading and its freshness rule live in `lab_forecast::measurement_wire`, shared with
//! the forecaster container: a live meter reading for *right now*, never a forecast series, so it feeds
//! only the current live tick. This module keeps the VEN's path to them.

pub use lab_forecast::measurement_wire::{
    resolve_measured_kw, MeasurementReading, MEASUREMENT_STALENESS_THRESHOLD,
};
