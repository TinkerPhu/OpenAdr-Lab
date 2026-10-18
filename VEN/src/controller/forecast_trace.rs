//! The boundary between the planner and the opportunistic forecast (openspec
//! `opportunistic-objective` D6).
//!
//! Under the opportunistic objective nothing is solved: the planner builds its slot grid and its
//! forecast inputs as for a solve, hands them to a forecaster that steps the site through the
//! horizon with the live tick's own decision (`simulator::opportunistic_forecast`), and reads
//! each asset's part back from the trace (`AssetMilpContext::solution_from_trace`). These are the
//! two values that cross that boundary. They hold data only; the planner may not reach the
//! simulator, so the forecaster is handed in as a function.

use chrono::{DateTime, Utc};
use std::collections::HashMap;

/// The horizon the forecaster steps through: the planner's own slot grid and the forecasts the
/// plan uses.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ForecastGrid {
    /// Start of each slot.
    pub slot_start: Vec<DateTime<Utc>>,
    /// Length of each slot [s].
    pub slot_s: Vec<f64>,
    /// Each forecast participant's (PV array, base load) power per slot [kW], a magnitude, by
    /// asset id: the series the plan itself is built on.
    pub forecast_kw_by_asset: HashMap<String, Vec<f64>>,
}

/// What the forecaster saw happen, slot by slot.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ForecastTrace {
    /// Each asset's net power per slot [kW], positive = import.
    pub net_kw_by_asset: HashMap<String, Vec<f64>>,
    /// Each asset's state at the start of every slot and at the end of the horizon (n + 1
    /// entries), keyed as its live `state_values`.
    pub states_by_asset: HashMap<String, Vec<HashMap<String, f64>>>,
    /// Site import and export per slot [kW], both ≥ 0.
    pub import_kw: Vec<f64>,
    pub export_kw: Vec<f64>,
}
