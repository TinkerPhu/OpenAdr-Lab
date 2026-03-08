use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::entities::asset::PlanTrigger;

/// Whether a plan time slot is firm (near-horizon, dispatched) or flexible (far-horizon).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SlotType {
    Firm,     // within near-horizon — will be dispatched
    Flexible, // far-horizon — may shift
}

/// Allocation of energy to a specific packet within a time slot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PacketAllocation {
    pub packet_id: Uuid,
    pub asset_id: String,
    pub power_kw: f64,
    pub energy_kwh: f64,
    pub cost_eur: f64,
    pub co2_kg: f64,
}

/// A single time slot in the plan.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanTimeSlot {
    pub slot_index: usize,
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
    pub slot_type: SlotType,

    /// Import price for this slot (€/kWh)
    pub import_price_eur_kwh: f64,
    /// Export price for this slot (€/kWh)
    pub export_price_eur_kwh: f64,
    /// CO2 intensity for this slot (g/kWh)
    pub co2_g_kwh: f64,

    /// Net baseline load before any scheduling (kW, positive = import)
    pub baseline_kw: f64,
    /// Import capacity limit for this slot (kW); None = unconstrained
    pub import_cap_kw: Option<f64>,

    /// Planned packet allocations in this slot
    pub allocations: Vec<PacketAllocation>,

    /// PV generation forecast for this slot (kW)
    pub pv_forecast_kw: f64,

    /// Net planned import after all allocations + PV (kW)
    pub net_import_kw: f64,
    /// Net planned export after all allocations + PV (kW)
    pub net_export_kw: f64,
}

/// Flexibility envelope offered to VTN for capacity or price optimization.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlexibilityEnvelope {
    pub packet_id: Uuid,
    pub asset_id: String,
    pub energy_needed_kwh: f64,
    pub power_min_kw: f64,
    pub power_max_kw: f64,
    pub window_start: DateTime<Utc>,
    pub window_end: DateTime<Utc>,
    /// Price range where asset is flexible: (min_bid, max_bid) €/kWh
    pub rate_range: Option<(f64, f64)>,
}

/// A warning generated during planning.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanWarning {
    pub packet_id: Option<Uuid>,
    pub message: String,
    pub severity: String, // "info", "warn", "error"
}

/// A complete plan covering the planning horizon.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Plan {
    pub id: Uuid,
    pub generated_at: DateTime<Utc>,
    pub horizon_start: DateTime<Utc>,
    pub horizon_end: DateTime<Utc>,
    pub trigger: PlanTrigger,

    pub slots: Vec<PlanTimeSlot>,
    pub flexibility_envelopes: Vec<FlexibilityEnvelope>,
    pub warnings: Vec<PlanWarning>,

    /// Total estimated cost over the plan horizon (€)
    pub total_cost_eur: f64,
    /// Total estimated CO2 over the plan horizon (kg)
    pub total_co2_kg: f64,
    /// Total planned import energy (kWh)
    pub total_import_kwh: f64,
    /// Total planned export energy (kWh)
    pub total_export_kwh: f64,
}

impl Plan {
    /// Return the plan slot that covers `now`, if any.
    pub fn current_slot(&self, now: DateTime<Utc>) -> Option<&PlanTimeSlot> {
        self.slots
            .iter()
            .find(|s| s.start <= now && now < s.end)
    }
}

/// Intermediate calculation cache used during planning (per packet × slot).
#[derive(Debug, Clone)]
pub struct CalcCache {
    pub slot_index: usize,
    pub packet_id: Uuid,
    pub effective_cost_eur_kwh: f64,
    pub surplus_for_packet_kw: f64,
    pub comfort_bid_eur_kwh: f64,
    pub time_pressure: f64,
    pub marginal_value: f64,
    pub eligible: bool,
}
