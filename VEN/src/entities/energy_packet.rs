use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::entities::asset::{ComfortRate, CompletionPolicy};

/// Status of an EnergyPacket through its lifecycle.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum PacketStatus {
    Pending,          // created, not yet scheduled
    Scheduled,        // allocated in plan
    Active,           // currently executing
    Completed,        // successfully finished
    PartialCompleted, // finished with < 100% fill at deadline
    Failed,           // device failed mid-execution
    Abandoned,        // infeasible or past deadline
}

/// A single deadline tier: complete by `latest_end` within budget constraints.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeadlineTier {
    pub latest_end: DateTime<Utc>,
    pub max_total_cost_eur: Option<f64>,
    pub max_marginal_rate_eur_kwh: Option<f64>,
    /// Minimum fill fraction (0.0..1.0) required for this tier to count as success
    pub min_completion: f64,
}

/// A value curve: maps task fill fraction to marginal bid (€/kWh).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValueCurve {
    pub rates: Vec<ComfortRate>,
}

impl ValueCurve {
    /// Interpolate the bid at a given fill fraction (0.0..1.0).
    pub fn bid_at(&self, fill: f64) -> f64 {
        if self.rates.is_empty() {
            return 0.0;
        }
        if fill <= self.rates[0].fill {
            return self.rates[0].bid_eur_kwh;
        }
        for i in 1..self.rates.len() {
            let lo = &self.rates[i - 1];
            let hi = &self.rates[i];
            if fill <= hi.fill {
                let t = (fill - lo.fill) / (hi.fill - lo.fill);
                return lo.bid_eur_kwh + t * (hi.bid_eur_kwh - lo.bid_eur_kwh);
            }
        }
        self.rates.last().map(|r| r.bid_eur_kwh).unwrap_or(0.0)
    }
}

/// An EnergyPacket represents a scheduled energy task (charge EV, run heater, batch process…).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnergyPacket {
    pub id: Uuid,
    pub asset_id: String,
    pub status: PacketStatus,

    /// How much energy is needed total (kWh)
    pub target_energy_kwh: f64,
    /// How much has been delivered so far (kWh)
    pub delivered_energy_kwh: f64,
    /// Desired power level when running (kW)
    pub desired_power_kw: f64,

    /// Earliest the task may start
    pub earliest_start: DateTime<Utc>,
    /// Deadline tiers (first = preferred, later = fallback)
    pub deadlines: Vec<DeadlineTier>,

    /// Value curve for this task (bid vs. fill)
    pub value_curve: ValueCurve,

    /// How to handle the task after the final deadline
    pub completion_policy: CompletionPolicy,

    /// Plan output: planned power profile per slot index (kW)
    #[serde(default)]
    pub planned_power_profile: Vec<f64>,

    /// Estimated total cost at current plan (€)
    pub estimated_cost_eur: f64,
    /// Estimated total CO2 at current plan (kg)
    pub estimated_co2_kg: f64,

    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl EnergyPacket {
    /// Task completion fraction: 0.0 (none) to 1.0 (full).
    pub fn fill(&self) -> f64 {
        if self.target_energy_kwh > 0.0 {
            (self.delivered_energy_kwh / self.target_energy_kwh).clamp(0.0, 1.0)
        } else {
            1.0
        }
    }

    /// The current active deadline tier (first in list).
    pub fn active_deadline(&self) -> Option<&DeadlineTier> {
        self.deadlines.first()
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entities::asset::ComfortRate;

    #[test]
    fn value_curve_interpolates() {
        let curve = ValueCurve {
            rates: vec![
                ComfortRate { fill: 0.0, bid_eur_kwh: 0.35 },
                ComfortRate { fill: 1.0, bid_eur_kwh: 0.05 },
            ],
        };
        assert!((curve.bid_at(0.0) - 0.35).abs() < 1e-6);
        assert!((curve.bid_at(1.0) - 0.05).abs() < 1e-6);
        assert!((curve.bid_at(0.5) - 0.20).abs() < 1e-6);
    }

    #[test]
    fn fill_clamped() {
        let now = Utc::now();
        let packet = EnergyPacket {
            id: Uuid::new_v4(),
            asset_id: "ev".to_string(),
            status: PacketStatus::Active,
            target_energy_kwh: 10.0,
            delivered_energy_kwh: 12.0, // overdelivered
            desired_power_kw: 7.4,
            earliest_start: now,
            deadlines: vec![],
            value_curve: ValueCurve { rates: vec![] },
            completion_policy: CompletionPolicy::Stop,
            planned_power_profile: vec![],
            estimated_cost_eur: 0.0,
            estimated_co2_kg: 0.0,
            created_at: now,
            updated_at: now,
        };
        assert_eq!(packet.fill(), 1.0);
    }
}
