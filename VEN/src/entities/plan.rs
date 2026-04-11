use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::entities::asset::PlanTrigger;
use crate::entities::energy_packet::EnergyPacket;

/// Defines the temporal scope of a planning cycle (§6.1).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanningHorizon {
    pub start_time: DateTime<Utc>,
    pub end_time: DateTime<Utc>,
    pub step_size_s: u64, // planning timestep in seconds (e.g. 300 = 5min)
    pub num_steps: usize,
    pub far_horizon: DateTime<Utc>, // = end_time
}

/// Assignment of energy to a specific packet within a time slot (§6.3).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PacketAllocation {
    pub packet_id: Uuid,
    pub asset_id: String,
    /// Total power allocated to this packet in this slot (kW)
    pub power_kw: f64,
    /// Portion from PV surplus (opportunity cost = ExportPrice)
    pub surplus_power_kw: f64,
    /// Portion from grid import (cost = ImportPrice); power_kw = surplus_power_kw + grid_power_kw
    pub grid_power_kw: f64,
    /// Effective priority at time of allocation
    pub marginal_value: f64,
    /// Cost in this slot (€): SurplusPower×ExportPrice×dt + GridPower×ImportPrice×dt
    pub cost_eur: f64,
    /// CO2 in this slot (g): GridPower × CO2Rate × dt (surplus has zero CO2)
    pub co2_g: f64,
}

/// A single time slot in the plan (§6.2).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanTimeSlot {
    pub slot_index: usize,
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,

    // --- External Conditions (from RateSnapshot) ---
    /// Import tariff for this slot (€/kWh)
    pub import_tariff_eur_kwh: f64,
    /// Export tariff for this slot (€/kWh)
    pub export_tariff_eur_kwh: f64,
    /// CO2 intensity for this slot (g/kWh)
    pub co2_g_kwh: f64,
    /// = ImportPrice + (CO2Rate × CO2Weight); used for storage arbitrage scoring
    pub grid_effective_cost: f64,
    /// True if rate was filled by StaleRatePolicy (VTN offline); used for PlanWarning generation
    pub rate_estimated: bool,
    /// Effective import capacity limit (subscription + event limit) (kW)
    pub import_cap_kw: f64,
    /// Effective export capacity limit (kW)
    pub export_cap_kw: f64,

    // --- Baseline and Surplus ---
    /// Net baseline load before any scheduling (kW, positive = import)
    pub baseline_kw: f64,
    /// PV generation forecast for this slot (kW)
    pub pv_forecast_kw: f64,
    /// = max(0, -BaselineLoad): PV surplus available above fixed loads
    pub surplus_available_kw: f64,

    // --- Planned Allocations (optimizer output) ---
    pub allocations: Vec<PacketAllocation>,
    /// Net planned import after all allocations + PV (kW)
    pub net_import_kw: f64,
    /// Net planned export after all allocations + PV (kW)
    pub net_export_kw: f64,

    // --- Flexibility (derived after planning) ---
    /// How much more could be imported in this slot (kW)
    pub import_flexibility_kw: f64,
    /// How much more could be exported in this slot (kW)
    pub export_flexibility_kw: f64,
}

/// Flexibility envelope offered to VTN for capacity or price optimization (§6.9).
/// One per packet with unallocated energy in the planning horizon.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlexibilityEnvelope {
    pub packet_id: Uuid,
    pub asset_id: String,
    /// Energy still needed in the horizon (kWh)
    pub energy_needed_kwh: f64,
    /// Asset's min power (kW)
    pub power_min_kw: f64,
    /// Asset's max power (kW)
    pub power_max_kw: f64,
    /// Earliest slot for this packet
    pub window_start: DateTime<Utc>,
    /// Latest slot (LatestEnd for STOP, open for CONTINUE)
    pub window_end: DateTime<Utc>,
    /// Number of slots in window
    pub slots_available: usize,
    /// Max rate this packet will accept (€/kWh)
    pub max_acceptable_rate: f64,
    /// Min rate at projected fill (€/kWh)
    pub min_acceptable_rate: f64,
    /// MaxTotalCost - AccumulatedCost (€)
    pub budget_remaining_eur: f64,
    /// Estimated cost (EnergyNeeded × avg eligible slot GridEffectiveCost) (€)
    pub estimated_cost_eur: f64,
    /// Estimated CO2 (EnergyNeeded × avg eligible slot CO2Rate) (g)
    pub estimated_co2_g: f64,
}

/// Live site-level flexibility available to the grid right now (§9).
///
/// Computed directly from current asset state — independent of the active plan.
/// Always queryable without triggering a planning cycle.
///
/// up_kw:   how much the VEN can reduce grid consumption right now (kW, ≥ 0).
/// down_kw: how much the VEN can increase grid consumption right now (kW, ≥ 0).
///
/// Duration fields estimate how long the VEN can sustain the headroom based
/// on available storage energy. None if no storage assets are present.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SiteFlexibilityEnvelope {
    pub ts:              DateTime<Utc>,
    /// Consumption-reduction headroom available right now (kW). Always ≥ 0.
    pub up_kw:           f64,
    /// Consumption-increase headroom available right now (kW). Always ≥ 0.
    pub down_kw:         f64,
    /// Estimated duration up_kw can be sustained, in seconds. None = no storage.
    pub up_duration_s:   Option<u64>,
    /// Estimated duration down_kw can be sustained, in seconds. None = no storage.
    pub down_duration_s: Option<u64>,
}

/// Severity of a plan warning.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum WarningSeverity {
    Info,
    Warning,
    Critical,
}

/// A warning generated during planning (§6.5).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanWarning {
    pub severity: WarningSeverity,
    pub packet_id: Option<Uuid>, // null if system-level warning
    pub message: String,
    pub suggested_action: Option<String>,
}

/// Summary of the full plan horizon.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PlanSummary {
    pub total_cost_eur: f64,
    pub total_co2_g: f64,
    pub total_import_kwh: f64,
    pub total_export_kwh: f64,
}

/// A complete plan covering the planning horizon (§6.10).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Plan {
    pub id: Uuid,
    pub created_at: DateTime<Utc>,
    pub trigger: PlanTrigger,
    pub horizon: PlanningHorizon,

    // --- All time slots (uniform, full horizon) ---
    pub slots: Vec<PlanTimeSlot>,
    pub summary: PlanSummary,

    // --- Flexibility offered to VTN ---
    pub envelopes: Vec<FlexibilityEnvelope>,

    // --- Combined ---
    /// Snapshot of all packets considered at plan time
    pub packets: Vec<EnergyPacket>,
    pub warnings: Vec<PlanWarning>,
    /// Full per-(ts × asset) audit trail.
    pub steps: Vec<PlanStep>,
}

impl Plan {
    /// All slots in chronological order.
    pub fn all_slots(&self) -> impl Iterator<Item = &PlanTimeSlot> {
        self.slots.iter()
    }

    /// Return the plan slot that covers `now`, if any.
    pub fn current_slot(&self, now: DateTime<Utc>) -> Option<&PlanTimeSlot> {
        self.slots.iter().find(|s| s.start <= now && now < s.end)
    }
}

/// One planning decision for one asset at one time step.
/// Populated by the planner; steps field of Plan holds the full audit trail.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanStep {
    pub ts: DateTime<Utc>,
    pub asset_id: String,
    pub setpoint_kw: f64,
    pub actual_power_kw: f64,
}
