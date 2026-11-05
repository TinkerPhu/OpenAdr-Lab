//! The lab-forecaster: turns the weather forecast and the live PV measurement into the retained forecast
//! messages of `lab_forecast::forecast_payload`.
//!
//! `engine` is the decision (pure: inputs and a `now` in, publications out); everything that touches the
//! world (broker, files, clock) lives outside it, so the engine is tested without any of them.

pub mod config;
pub mod dispatch;
pub mod engine;
pub mod runtime;
