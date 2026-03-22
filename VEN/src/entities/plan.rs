use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::assets::{AssetCapability, AssetState};
use crate::entities::asset::{PlanTrigger, UserRequestMode};
use crate::entities::energy_packet::EnergyPacket;

/// Whether a plan time slot is firm (near-horizon, dispatched) or flexible (far-horizon) (§6.2.1).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SlotType {
    Firm,     // within near-horizon — will be dispatched
    Flexible, // far-horizon — capacity preserved for flexibility
}

/// Defines the temporal scope of a planning cycle (§6.1).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanningHorizon {
    pub start_time: DateTime<Utc>,
    pub end_time: DateTime<Utc>,
    pub step_size_s: u64, // planning timestep in seconds (e.g. 300 = 5min)
    pub num_steps: usize,
    pub near_horizon: DateTime<Utc>, // = now + NearHorizonDuration
    pub far_horizon: DateTime<Utc>,  // = end_time
}

/// Assignment of energy to a specific packet within a time slot (§6.3).
/// Only exists in FIRM slots.
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
    /// Effective priority at time of allocation (from CalcCache)
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
    pub slot_type: SlotType,

    // --- External Conditions (from RateSnapshot) ---
    /// Import tariff for this slot (€/kWh)
    pub import_tariff_eur_kwh: f64,
    /// Export tariff for this slot (€/kWh)
    pub export_tariff_eur_kwh: f64,
    /// CO2 intensity for this slot (g/kWh)
    pub co2_g_kwh: f64,
    /// = ImportPrice + (CO2Rate × CO2Weight); used for FLEXIBLE slot scoring and storage arbitrage
    pub grid_effective_cost: f64,
    /// True if rate was filled by StaleRatePolicy (VTN offline); used for PlanWarning generation
    pub rate_estimated: bool,
    /// Effective import capacity limit (subscription + reservation + event limit) (kW)
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

    // --- Planned Allocations (optimizer output, FIRM slots only) ---
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
/// One per packet with unallocated energy in FLEXIBLE slots.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlexibilityEnvelope {
    pub packet_id: Uuid,
    pub asset_id: String,
    /// Energy still needed in flexible horizon (kWh)
    pub energy_needed_kwh: f64,
    /// Asset's min power (if STEPPED, smallest nonzero step) (kW)
    pub power_min_kw: f64,
    /// Asset's max power (kW)
    pub power_max_kw: f64,
    /// Earliest FLEXIBLE slot for this packet
    pub window_start: DateTime<Utc>,
    /// Latest FLEXIBLE slot (LatestEnd for STOP, open for CONTINUE)
    pub window_end: DateTime<Utc>,
    /// Number of FLEXIBLE slots in window
    pub slots_available: usize,
    /// Max rate this packet will accept (€/kWh)
    pub max_acceptable_rate: f64,
    /// Min rate at projected fill (€/kWh)
    pub min_acceptable_rate: f64,
    /// MaxTotalCost - AccumulatedCost - FIRM slot costs (€)
    pub budget_remaining_eur: f64,
    /// Estimated cost (EnergyNeeded × avg eligible slot GridEffectiveCost) (€)
    pub estimated_cost_eur: f64,
    /// Estimated CO2 (EnergyNeeded × avg eligible slot CO2Rate) (g)
    pub estimated_co2_g: f64,
}

/// Live site-level flexibility available to the grid right now (§9).
///
/// Computed directly from current asset state and active reservations —
/// independent of the active plan. Always queryable without triggering
/// a planning cycle.
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

/// Firm section summary.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FirmSummary {
    pub total_cost_eur: f64,
    pub total_co2_g: f64,
    pub total_import_kwh: f64,
    pub total_export_kwh: f64,
}

/// Flexible section summary.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FlexibleSummary {
    pub total_energy_kwh: f64,
    pub estimated_cost_eur: f64,
    pub estimated_co2_g: f64,
}

/// A complete plan covering the planning horizon (§6.10).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Plan {
    pub id: Uuid,
    pub created_at: DateTime<Utc>,
    pub trigger: PlanTrigger,
    pub horizon: PlanningHorizon,
    /// Divides FIRM from FLEXIBLE sections
    pub firm_boundary: DateTime<Utc>,

    // --- FIRM section (near horizon) ---
    pub firm_slots: Vec<PlanTimeSlot>,
    pub firm_summary: FirmSummary,

    // --- FLEXIBLE section (far horizon) ---
    pub flexible_slots: Vec<PlanTimeSlot>,
    pub envelopes: Vec<FlexibilityEnvelope>,
    pub flexible_summary: FlexibleSummary,

    // --- Combined ---
    /// Snapshot of all packets considered at plan time
    pub packets: Vec<EnergyPacket>,
    pub warnings: Vec<PlanWarning>,
    /// Full per-(ts × asset) audit trail. Populated by Phase D CP2.
    pub steps: Vec<PlanStep>,
}

impl Plan {
    /// All slots (firm + flexible), in order.
    pub fn all_slots(&self) -> impl Iterator<Item = &PlanTimeSlot> {
        self.firm_slots.iter().chain(self.flexible_slots.iter())
    }

    /// Return the plan slot that covers `now`, if any.
    pub fn current_slot(&self, now: DateTime<Utc>) -> Option<&PlanTimeSlot> {
        self.all_slots().find(|s| s.start <= now && now < s.end)
    }
}

// ─── Phase D types ────────────────────────────────────────────────────────────

/// Source that created a reservation (moved from controller/reservation.rs to
/// avoid entities → controller circular dependency).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ReservationSource {
    /// VTN SIMPLE-type FIRM event: "reduce consumption by kw kW during window."
    VtnFirmEvent { event_id: String },
    /// FlexibilityPolicy scheduled window (Phase C).
    PolicySchedule { policy_id: String },
    /// FlexibilityPolicy default reserve (Phase C).
    PolicyDefault,
    /// User request (Phase F).
    UserRequest { request_id: Uuid },
}

/// Which comfort bound was violated to produce a setpoint.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ComfortBoundType {
    MinTemperature,
    MaxTemperature,
    MinSoc,
    MaxSoc,
}

/// The rule that fired to produce a PlanStep's setpoint.
/// Emitted at decision time — never reconstructed after the fact.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum PlanReason {
    FirmObligation { source: ReservationSource, required_kw: f64 },
    CheapTariff { tariff_eur_per_kwh: f64, threshold_eur_per_kwh: f64 },
    ExpensiveTariff { tariff_eur_per_kwh: f64, threshold_eur_per_kwh: f64 },
    GridImportLimit { limit_kw: f64 },
    GridExportLimit { limit_kw: f64 },
    SocCeiling { soc_pct: f64 },
    SocFloor { soc_pct: f64 },
    ComfortBound { bound_type: ComfortBoundType },
    UserOverride { request_id: Uuid, mode: UserRequestMode },
    PolicyReserve { policy_id: String },
    OpportunityMissed { reason: String },
    Idle,
}

/// One planning decision for one asset at one time step.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanStep {
    pub ts: DateTime<Utc>,
    pub asset_id: String,
    pub state_before: AssetState,
    pub capability: AssetCapability,
    pub reserved_up_kw: f64,
    pub reserved_down_kw: f64,
    pub avail_max_export_kw: f64,
    pub avail_max_import_kw: f64,
    pub setpoint_kw: f64,
    pub actual_power_kw: f64,
    pub reason: PlanReason,
}

/// Pre-computed per asset before the planning loop.
/// Internal to the planner — not serialized.
#[derive(Debug, Clone)]
pub struct LookaheadContext {
    pub capability_trajectory: Vec<(DateTime<Utc>, AssetCapability)>,
    pub tariff_min_ahead_eur_per_kwh: f64,
    pub tariff_max_ahead_eur_per_kwh: f64,
    pub ceiling_eta: Option<DateTime<Utc>>,
    pub floor_eta: Option<DateTime<Utc>>,
}

/// Intermediate calculation cache used during planning per (packet × slot) (§2.10).
/// Internal to the Planner; not persisted.
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
