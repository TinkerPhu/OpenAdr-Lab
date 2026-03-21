use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::entities::asset::{ComfortRate, CompletionPolicy, UserRequestMode};

/// Status of an EnergyPacket through its lifecycle (§1.4).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum PacketStatus {
    Pending,          // not yet started, waiting for optimal slot
    Scheduled,        // planned start time assigned
    Active,           // currently executing (energy flowing)
    Paused,           // temporarily suspended (by conflict, VTN, or user)
    Completed,        // target energy/SoC reached (FillPercentage = 1.0)
    PartialCompleted, // deadline reached with FillPercentage < 1.0 and CompletionPolicy = STOP
    Abandoned,        // all tiers exhausted or user cancelled
    Failed,           // device failure prevented completion
}

/// A single deadline tier: complete by `deadline` within budget constraints (§2.8).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeadlineTier {
    pub deadline: DateTime<Utc>,
    pub max_total_cost_eur: Option<f64>,
    pub max_marginal_rate_eur_kwh: Option<f64>,
    /// Minimum fill fraction (0.0..1.0) required for this tier to count as success
    pub min_completion: f64,
}

/// Actual or planned energy measurement at a timestep (§2.6).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnergySnapshot {
    pub ts: DateTime<Utc>,
    pub power_kw: f64,                // instantaneous power at this timestep
    pub cumulative_energy_kwh: f64,   // cumulative energy delivered since packet start
}

/// Complete user preference model for an EnergyPacket (§2.9).
/// Contains both the fill-based comfort rates AND the temporal deadline tiers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValueCurve {
    /// Fill-based marginal value: sorted ascending by fill fraction.
    pub comfort_rates: Vec<ComfortRate>,
    /// Time-based tiers: sorted by preference, Tier 0 = most preferred.
    pub deadline_tiers: Vec<DeadlineTier>,
    /// Currently targeted tier index (set by Planner, 0-based).
    pub active_tier_index: usize,
}

impl ValueCurve {
    /// Interpolate the bid at a given fill fraction (0.0..1.0).
    pub fn bid_at(&self, fill: f64) -> f64 {
        let rates = &self.comfort_rates;
        if rates.is_empty() {
            return 0.0;
        }
        if fill <= rates[0].fill {
            return rates[0].max_marginal_price;
        }
        for i in 1..rates.len() {
            let lo = &rates[i - 1];
            let hi = &rates[i];
            if fill <= hi.fill {
                let t = (fill - lo.fill) / (hi.fill - lo.fill);
                return lo.max_marginal_price + t * (hi.max_marginal_price - lo.max_marginal_price);
            }
        }
        rates.last().map(|r| r.max_marginal_price).unwrap_or(0.0)
    }

    /// The currently active deadline tier (or None if empty).
    pub fn active_deadline(&self) -> Option<&DeadlineTier> {
        self.deadline_tiers.get(self.active_tier_index)
    }

    /// The absolute latest end (from the last tier's deadline).
    pub fn latest_end(&self) -> Option<DateTime<Utc>> {
        self.deadline_tiers.last().map(|t| t.deadline)
    }
}

/// An EnergyPacket represents a discrete energy task (§4.1).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnergyPacket {
    pub id: Uuid,
    pub asset_id: String,
    pub status: PacketStatus,

    // ── Temporal Bounds ─────────────────────────────────────────────────────
    pub earliest_start: DateTime<Utc>,
    pub latest_start: Option<DateTime<Utc>>,  // must begin by this or abandon

    // ── Energy Target ────────────────────────────────────────────────────────
    pub target_energy_kwh: f64,        // total energy required for 100% completion
    pub target_soc: Option<f64>,       // target SoC if storage asset (alternative to energy)
    pub desired_power_kw: f64,         // preferred power level when running

    // ── Value ────────────────────────────────────────────────────────────────
    pub value_curve: ValueCurve,       // comfort rates + deadline tiers
    pub request_mode: UserRequestMode,
    pub completion_policy: CompletionPolicy,
    /// €/kWh bid for priority after last deadline (only when CompletionPolicy = CONTINUE)
    pub post_deadline_comfort_bid: Option<f64>,

    // ── Power Profile ────────────────────────────────────────────────────────
    /// Optimizer output: planned power at each planning timestep (kW)
    #[serde(default)]
    pub planned_power_profile: Vec<EnergySnapshot>,
    /// Actual measurements recorded during execution
    #[serde(default)]
    pub past_power_profile: Vec<EnergySnapshot>,

    // ── Budget Tracking ──────────────────────────────────────────────────────
    pub accumulated_cost_eur: f64,     // Σ(PastPower × ImportPrice × dt) so far
    pub accumulated_co2_g: f64,        // Σ(PastPower × CO2Rate × dt) so far

    // ── Planner Estimates (updated each plan cycle) ──────────────────────────
    pub estimated_cost_eur: f64,
    pub estimated_co2_g: f64,
    pub estimated_completion: f64,     // 0.0..1.0, expected fill at active tier deadline
    pub last_estimate_at: Option<DateTime<Utc>>,

    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl EnergyPacket {
    /// Construct a new packet with all defaults set (Pending, empty profiles, zero accumulators).
    /// Callers override specific fields via struct update syntax as needed.
    pub fn new(
        asset_id: String,
        target_energy_kwh: f64,
        desired_power_kw: f64,
        value_curve: ValueCurve,
        now: DateTime<Utc>,
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            asset_id,
            status: PacketStatus::Pending,
            earliest_start: now,
            latest_start: None,
            target_energy_kwh,
            target_soc: None,
            desired_power_kw,
            value_curve,
            request_mode: UserRequestMode::ByDeadline,
            completion_policy: CompletionPolicy::Stop,
            post_deadline_comfort_bid: None,
            planned_power_profile: vec![],
            past_power_profile: vec![],
            accumulated_cost_eur: 0.0,
            accumulated_co2_g: 0.0,
            estimated_cost_eur: 0.0,
            estimated_co2_g: 0.0,
            estimated_completion: 0.0,
            last_estimate_at: None,
            created_at: now,
            updated_at: now,
        }
    }

    /// Task completion fraction: 0.0 (none) to 1.0 (full).
    pub fn fill(&self) -> f64 {
        if self.target_energy_kwh > 0.0 {
            let past_energy: f64 = self.past_power_profile.iter()
                .map(|s| s.cumulative_energy_kwh)
                .next_back()
                .unwrap_or(0.0);
            (past_energy / self.target_energy_kwh).clamp(0.0, 1.0)
        } else {
            1.0
        }
    }

    /// Total energy already delivered (kWh).
    pub fn past_energy_kwh(&self) -> f64 {
        self.past_power_profile.iter()
            .map(|s| s.cumulative_energy_kwh)
            .next_back()
            .unwrap_or(0.0)
    }

    /// Energy still needed (kWh).
    pub fn undelivered_energy_kwh(&self) -> f64 {
        (self.target_energy_kwh - self.past_energy_kwh()).max(0.0)
    }

    /// True if the packet has reached a terminal status.
    pub fn is_terminal(&self) -> bool {
        matches!(
            self.status,
            PacketStatus::Completed
                | PacketStatus::PartialCompleted
                | PacketStatus::Failed
                | PacketStatus::Abandoned
        )
    }

    /// True if the packet is currently executing.
    pub fn is_executing(&self) -> bool {
        self.status == PacketStatus::Active
    }

    /// True if execution has started (any past data recorded).
    pub fn started(&self) -> bool {
        !self.past_power_profile.is_empty()
    }

    /// Absolute latest end time (from last deadline tier).
    pub fn latest_end(&self) -> Option<DateTime<Utc>> {
        self.value_curve.latest_end()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entities::asset::ComfortRate;

    fn make_curve(rates: Vec<(f64, f64)>) -> ValueCurve {
        ValueCurve {
            comfort_rates: rates
                .into_iter()
                .map(|(fill, price)| ComfortRate {
                    fill,
                    max_marginal_price: price,
                    max_marginal_co2: 0.0,
                })
                .collect(),
            deadline_tiers: vec![],
            active_tier_index: 0,
        }
    }

    #[test]
    fn new_sets_defaults() {
        let now = Utc::now();
        let curve = ValueCurve { comfort_rates: vec![], deadline_tiers: vec![], active_tier_index: 0 };
        let p = EnergyPacket::new("ev".to_string(), 10.0, 3.0, curve, now);
        assert_eq!(p.status, PacketStatus::Pending);
        assert!(p.past_power_profile.is_empty());
        assert!(p.planned_power_profile.is_empty());
        assert_eq!(p.accumulated_cost_eur, 0.0);
        assert_eq!(p.accumulated_co2_g, 0.0);
        assert_eq!(p.estimated_cost_eur, 0.0);
        assert_eq!(p.estimated_completion, 0.0);
        assert_eq!(p.created_at, now);
        assert_eq!(p.target_energy_kwh, 10.0);
        assert_eq!(p.desired_power_kw, 3.0);
        assert!(p.target_soc.is_none());
        assert!(p.last_estimate_at.is_none());
    }

    #[test]
    fn value_curve_interpolates() {
        let curve = make_curve(vec![(0.0, 0.35), (1.0, 0.05)]);
        assert!((curve.bid_at(0.0) - 0.35).abs() < 1e-6);
        assert!((curve.bid_at(1.0) - 0.05).abs() < 1e-6);
        assert!((curve.bid_at(0.5) - 0.20).abs() < 1e-6);
    }

    #[test]
    fn value_curve_empty_returns_zero() {
        let curve = make_curve(vec![]);
        assert_eq!(curve.bid_at(0.5), 0.0);
    }
}
