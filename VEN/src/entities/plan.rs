use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::entities::asset::PlanTrigger;
use crate::entities::planner_params::PlannerObjective;

/// One zone of a variable-step planning horizon.
/// Defined here (domain layer) so `PlanningHorizon` can carry zone metadata without
/// importing `profile.rs` (infra). `profile::PlannerConfig` imports this type for YAML parsing.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PlanZone {
    /// Slot width for this zone in seconds. Must be a multiple of the first zone's step_s.
    pub step_s: u64,
    /// Number of slots in this zone.
    pub slots: usize,
}

/// Defines the temporal scope of a planning cycle (§6.1).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanningHorizon {
    pub start_time: DateTime<Utc>,
    pub end_time: DateTime<Utc>,
    pub step_size_s: u64, // planning timestep in seconds (e.g. 300 = 5min)
    pub num_steps: usize,
    pub far_horizon: DateTime<Utc>, // = end_time
    /// Zone definitions for variable-step plans.
    /// Contains one entry for uniform-step plans; three entries for 3-tier plans (Part B).
    /// Populated at plan creation; `#[serde(default)]` so old stored plans deserialize cleanly.
    #[serde(default)]
    pub zones: Vec<PlanZone>,
}

/// Assignment of energy to a specific asset within a time slot (§6.3).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetAllocation {
    pub asset_id: String,
    /// Total power allocated to this asset in this slot (kW)
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
    /// Shadow price on this slot's power-balance constraint: how much the planner's
    /// objective would change per extra kWh imported, from a second LP solve with the
    /// winning MILP's binary decisions fixed (see `docs/architecture/VEN_ARCHITECTURE.md`).
    /// Read-only diagnostic — does not influence any other field on this slot.
    /// `#[serde(default)]` so plans persisted before this field existed still deserialize.
    #[serde(default)]
    pub marginal_cost_import_eur_per_kwh: f64,
    /// Same shadow price, export side. Currently identical to the import-side value —
    /// a harmless simplification under cost-minimizing objectives (§5.2) — until a
    /// self-consumption-style objective needs the two to diverge.
    #[serde(default)]
    pub marginal_cost_export_eur_per_kwh: f64,
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
    /// Planned PV export after the planner's curtailment decision (kW, `<= pv_forecast_kw`).
    /// Equals `pv_forecast_kw` when the planner does not curtail (including the infeasibility
    /// fallback path). See `openspec/changes/pv-export-curtailment/`.
    #[serde(default)]
    pub pv_used_kw: f64,
    /// = max(0, -BaselineLoad): PV surplus available above fixed loads
    pub surplus_available_kw: f64,

    // --- Planned Allocations (optimizer output) ---
    pub allocations: Vec<AssetAllocation>,
    /// Net planned import after all allocations + PV (kW)
    pub net_import_kw: f64,
    /// Net planned export after all allocations + PV (kW)
    pub net_export_kw: f64,

    // --- Flexibility (derived after planning) ---
    /// How much more could be imported in this slot (kW)
    pub import_flexibility_kw: f64,
    /// How much more could be exported in this slot (kW)
    pub export_flexibility_kw: f64,

    // --- Battery setpoints (MILP output) ---
    /// Planned battery charge power in this slot (kW, ≥ 0). Set by MILP solver.
    #[serde(default)]
    pub bat_charge_kw: f64,
    /// Planned battery discharge power in this slot (kW, ≥ 0). Set by MILP solver.
    #[serde(default)]
    pub bat_discharge_kw: f64,

    // --- Normalized plan output ---
    /// Power allocated to each asset in this slot (kW), keyed by asset_id.
    /// Positive = consumption/charging, negative = discharge/generation.
    /// Built from `allocations` for easy lookup without iteration.
    #[serde(default)]
    pub planned_kw_by_asset: HashMap<String, f64>,

    /// Per-asset state values at the start of this slot (e.g. SoC, temperature),
    /// computed from the MILP solution at plan assembly time by each asset module.
    /// Key: asset_id → (metric_key → value), e.g. `"battery" → {"soc": 0.82}`.
    /// Empty for non-storage assets and when no plan state is available.
    #[serde(default)]
    pub planned_state_by_asset: HashMap<String, HashMap<String, f64>>,
}

/// Per-device schedulability metadata snapshot (§6.9).
///
/// Emitted for each active device session at plan time. Describes the device's
/// degrees of freedom — energy still needed, time window, asset power bounds,
/// max acceptable rate, budget remaining — not "unscheduled work".
///
/// Note: this is *not* the same as `SiteFlexibilityEnvelope`, which is the
/// live site-level headroom served by `GET /flexibility`. Per-device envelopes
/// only refresh at plan time; site headroom refreshes every dispatcher tick.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlexibilityEnvelope {
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
    pub ts: DateTime<Utc>,
    /// Consumption-reduction headroom available right now (kW). Always ≥ 0.
    pub up_kw: f64,
    /// Consumption-increase headroom available right now (kW). Always ≥ 0.
    pub down_kw: f64,
    /// Estimated duration up_kw can be sustained, in seconds. None = no storage.
    pub up_duration_s: Option<u64>,
    /// Estimated duration down_kw can be sustained, in seconds. None = no storage.
    pub down_duration_s: Option<u64>,
}

/// One historical sample of a `SiteFlexibilityEnvelope`, retained in a bounded
/// in-memory ring (`AppState::flexibility_history`, BL-43) so the UI can plot a
/// live headroom band over time — `SiteFlexibilityEnvelope` itself is
/// instant-only and has no forward schedule (see its own doc comment).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SiteFlexibilitySample {
    pub ts: DateTime<Utc>,
    pub up_kw: f64,
    pub down_kw: f64,
}

impl From<&SiteFlexibilityEnvelope> for SiteFlexibilitySample {
    fn from(env: &SiteFlexibilityEnvelope) -> Self {
        Self {
            ts: env.ts,
            up_kw: env.up_kw,
            down_kw: env.down_kw,
        }
    }
}

/// One future slot of the forward-looking site headroom trajectory —
/// distinct from `SiteFlexibilityEnvelope`, which is instant-only.
///
/// Re-derived fresh every dispatcher tick (not read statically off
/// `PlanTimeSlot.planned_state_by_asset`, a stale solve-time-only snapshot):
/// each asset's REAL current state is re-simulated forward via
/// `Asset::simulate_forward`, driven by the active plan's own
/// `planned_kw_by_asset` schedule, so this trajectory self-corrects for any
/// drift between the plan's assumptions and reality on every tick — see
/// `controller::envelope_forecast::compute_headroom_forecast`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SiteFlexibilityForecastSlot {
    pub ts: DateTime<Utc>,
    /// Consumption-reduction headroom at this future slot (kW). Always ≥ 0.
    pub up_kw: f64,
    /// Consumption-increase headroom at this future slot (kW). Always ≥ 0.
    pub down_kw: f64,
}

/// Outcome of the MILP solve that produced a `Plan`.
///
/// `Infeasible` covers every solve failure the two-phase solver can hit
/// (genuinely infeasible, unbounded, or any other solver error) — the
/// codebase has no distinct heuristic-solve path today (verified: all
/// "heuristic" references are `AssetHeuristics`/BL-14 learned load profiles,
/// unrelated to solver fallback), so `fallback_plan` is synonymous with
/// infeasibility, not a substitute heuristic solve.
///
/// `TimeLimit`/`GapLimit` (GB-31) cover the two ways a *feasible* solve can
/// still be non-optimal: HiGHS stopped at `solver_timeout_s` before
/// certifying optimality, or it stopped once the solution was within
/// `MIP_GAP_TARGET` of the best known bound (the more common case in
/// practice) — both read straight from `good_lp`'s `SolutionStatus`, not
/// hardcoded. See `MIP_GAP_TARGET`'s doc comment for what's still not
/// exposed (the achieved gap as a number, as opposed to this coarser
/// within-target/not classification).
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SolveStatus {
    Optimal,
    TimeLimit,
    GapLimit,
    Infeasible,
}

/// Severity of a plan warning.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum WarningSeverity {
    Info,
    Warning,
    Critical,
}

/// Stable, machine-readable classification of a `PlanWarning` (GB-25).
///
/// Covers exactly the 5 real construction sites in
/// `controller::milp_planner::results` today, plus `Other` as a catch-all for
/// any future warning that hasn't earned its own variant yet. `services::notify`
/// dedups new-vs-carried-over warnings on `kind` (not `message`, which can
/// carry per-cycle interpolated numbers).
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum WarningKind {
    /// `results::fallback_plan` — the MILP solver failed and this is the infeasibility fallback.
    SolverInfeasible,
    /// WP4.4 (BL-07) — the tariff/CO2 rate used for one or more slots was filled by the
    /// StaleRatePolicy rather than a fresh VTN-reported value.
    StaleRateEstimate,
    /// WP4.1-c (BL-28) — the MAX_COST budget could not be met at the target SoC.
    BudgetShortfall,
    /// Grid import/export capacity was exceeded in one or more slots; the solver used slack.
    CapacityViolation,
    /// WP6.3 (BL-09) — a penalty-rule threshold was still exceeded after solving (penalty accepted).
    PeakPenaltyExceeded,
    /// Reserved for future warning sites not yet classified above.
    Other,
}

/// A warning generated during planning (§6.5).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanWarning {
    pub severity: WarningSeverity,
    #[serde(default = "WarningKind::default_other")]
    pub kind: WarningKind,
    pub message: String,
    pub suggested_action: Option<String>,
}

impl WarningKind {
    fn default_other() -> Self {
        WarningKind::Other
    }
}

/// Decomposed MILP objective cost components for diagnostics.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CostBreakdown {
    pub c_energy_eur: f64,
    pub c_ghg_eur: f64,
    pub c_grid_eur: f64,
    pub c_wear_eur: f64,
    pub c_violations_eur: f64,
    pub v_services_eur: f64,
    /// WP6.3 (BL-09) — accepted peak-demand penalty cost, summed across all
    /// rule/window slacks that still exceeded threshold after solving.
    #[serde(default)]
    pub c_peak_penalty_eur: f64,
}

/// A penalty rule active on the current plan, for UI consumption (WP6.3, BL-09).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ActivePenaltyRule {
    pub rule_id: String,
    pub threshold_kw: f64,
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

    // --- Diagnostics ---
    pub warnings: Vec<PlanWarning>,
    // --- MILP solver output ---
    /// Battery SoC trajectory [kWh] at the end of each planning step (length = num_steps + 1).
    /// First entry is the initial SoC; populated by the MILP solver.
    #[serde(default)]
    pub soc_trajectory_kwh: Vec<f64>,
    /// Optimization objective used for this planning cycle.
    #[serde(default)]
    pub objective: PlannerObjective,
    /// Total MILP objective value (€). Phase 1 (economic cost) only — does not include
    /// Phase 2 friction terms.
    #[serde(default)]
    pub objective_eur: f64,
    /// Phase 2 friction objective value [EUR]. Sum of switching/startup/ramp/tier penalties.
    /// 0.0 when Phase 2 is disabled (phase2_epsilon_eur == 0.0) or on fallback.
    #[serde(default)]
    pub friction_eur: f64,
    /// Decomposed cost components for diagnostics.
    #[serde(default)]
    pub cost_breakdown: CostBreakdown,
    /// Whether this plan came from a successful MILP solve or the infeasibility
    /// fallback (WP-T2, `docs/history/project_journal.md, search "WP-T"`).
    #[serde(default = "SolveStatus::default_optimal")]
    pub solve_status: SolveStatus,
    /// WP6.3 (BL-09) — penalty rules active for this plan, so a UI client can
    /// render per-slot peak-demand status without deriving it independently.
    /// Empty when the feature is not configured.
    #[serde(default)]
    pub penalty_rules_active: Vec<ActivePenaltyRule>,
    /// GB-25 — wall-clock milliseconds the MILP solve took for this plan cycle.
    /// `None` for plans built outside `services::planning::adopt_if_warranted`
    /// (e.g. hand-built test fixtures) — never a synthesized `0`.
    #[serde(default)]
    pub solver_ms: Option<u64>,
    /// GB-25 — the solver's configured MIP gap tolerance (`MIP_GAP_TARGET`) at
    /// the time this plan was solved. A proxy only: the *configured* target,
    /// not the *achieved* gap on this particular solve (good_lp/highs expose no
    /// achieved-gap query today — see `docs/reference/TECHNICAL_DEBTS.md`).
    #[serde(default)]
    pub mip_gap_target: Option<f64>,
}

impl SolveStatus {
    fn default_optimal() -> Self {
        SolveStatus::Optimal
    }
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

#[cfg(test)]
mod solve_status_tests {
    use super::SolveStatus;

    #[test]
    fn solve_status_serializes_as_screaming_snake_case() {
        assert_eq!(
            serde_json::to_string(&SolveStatus::Optimal).unwrap(),
            "\"OPTIMAL\""
        );
        assert_eq!(
            serde_json::to_string(&SolveStatus::TimeLimit).unwrap(),
            "\"TIME_LIMIT\""
        );
        assert_eq!(
            serde_json::to_string(&SolveStatus::GapLimit).unwrap(),
            "\"GAP_LIMIT\""
        );
        assert_eq!(
            serde_json::to_string(&SolveStatus::Infeasible).unwrap(),
            "\"INFEASIBLE\""
        );
        assert_eq!(
            serde_json::from_str::<SolveStatus>("\"OPTIMAL\"").unwrap(),
            SolveStatus::Optimal
        );
        assert_eq!(
            serde_json::from_str::<SolveStatus>("\"TIME_LIMIT\"").unwrap(),
            SolveStatus::TimeLimit
        );
        assert_eq!(
            serde_json::from_str::<SolveStatus>("\"GAP_LIMIT\"").unwrap(),
            SolveStatus::GapLimit
        );
        assert_eq!(
            serde_json::from_str::<SolveStatus>("\"INFEASIBLE\"").unwrap(),
            SolveStatus::Infeasible
        );
    }
}
