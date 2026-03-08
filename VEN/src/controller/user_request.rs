/// Stage 5 — User Request Manager: creates EnergyPackets from user requests.
///
/// Provides the logic to translate a POST /requests body into an EnergyPacket
/// with proper ValueCurve (multi-tier deadlines, comfort rates), and computes
/// target energy from the asset's current state + profile.
use crate::entities::asset::{ComfortRate, CompletionPolicy, UserRequestMode};
use crate::entities::energy_packet::{DeadlineTier, EnergyPacket, PacketStatus, ValueCurve};
use crate::entities::user_request::{RequestDeadline, UserRequest, UserRequestStatus};
use crate::profile::Profile;
use crate::simulator::SimSnapshot;
use chrono::{DateTime, Utc};
use serde::Deserialize;
use uuid::Uuid;

/// Request body for POST /requests.
#[derive(Debug, Deserialize)]
pub struct CreateUserRequestBody {
    pub asset_id: String,
    pub target_soc: Option<f64>,
    pub target_energy_kwh: Option<f64>,
    pub desired_power_kw: Option<f64>,
    pub deadlines: Vec<RequestDeadlineInput>,
    pub completion_policy: Option<String>,
    pub comfort_rates: Option<Vec<ComfortRateInput>>,
}

#[derive(Debug, Deserialize)]
pub struct RequestDeadlineInput {
    pub latest_end: DateTime<Utc>,
    pub max_total_cost_eur: Option<f64>,
    pub max_marginal_rate_eur_kwh: Option<f64>,
    pub min_completion: Option<f64>,
}

#[derive(Debug, Deserialize)]
pub struct ComfortRateInput {
    pub fill: f64,
    pub bid: f64,
}

/// Error type for user request validation.
#[derive(Debug)]
pub enum RequestError {
    UnknownAsset(String),
    NoDeadlines,
    ZeroEnergy,
}

impl std::fmt::Display for RequestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RequestError::UnknownAsset(id) => write!(f, "unknown asset '{id}'"),
            RequestError::NoDeadlines => write!(f, "at least one deadline is required"),
            RequestError::ZeroEnergy => write!(f, "computed target_energy_kwh is zero or negative"),
        }
    }
}

/// Create a (UserRequest, EnergyPacket) pair from the POST /requests body.
pub fn create_from_body(
    body: CreateUserRequestBody,
    profile: &Profile,
    sim: Option<&SimSnapshot>,
    now: DateTime<Utc>,
) -> Result<(UserRequest, EnergyPacket), RequestError> {
    if body.deadlines.is_empty() {
        return Err(RequestError::NoDeadlines);
    }

    // Compute target energy and desired power
    let (target_energy_kwh, desired_power_kw) =
        resolve_target(&body, profile, sim)?;

    // Build completion policy
    let completion_policy = match body
        .completion_policy
        .as_deref()
        .unwrap_or("STOP")
    {
        "CONTINUE" => CompletionPolicy::Continue,
        _ => CompletionPolicy::Stop,
    };

    // Build comfort rates (user-specified or asset default)
    let comfort_rates: Vec<ComfortRate> = if let Some(ref rates) = body.comfort_rates {
        rates
            .iter()
            .map(|r| ComfortRate {
                fill: r.fill,
                max_marginal_price: r.bid,
                max_marginal_co2: 0.0,
            })
            .collect()
    } else {
        vec![
            ComfortRate { fill: 0.0, max_marginal_price: 0.35, max_marginal_co2: 0.0 },
            ComfortRate { fill: 1.0, max_marginal_price: 0.05, max_marginal_co2: 0.0 },
        ]
    };

    // Build deadline tiers from input
    let deadline_tiers: Vec<DeadlineTier> = body
        .deadlines
        .iter()
        .map(|d| DeadlineTier {
            deadline: d.latest_end,
            max_total_cost_eur: d.max_total_cost_eur,
            max_marginal_rate_eur_kwh: d.max_marginal_rate_eur_kwh,
            min_completion: d.min_completion.unwrap_or(0.8),
        })
        .collect();

    let tier_count = deadline_tiers.len();
    let max_total_cost_eur = body.deadlines.first().and_then(|d| d.max_total_cost_eur);

    let packet_id = Uuid::new_v4();
    let request_id = Uuid::new_v4();

    // Build EnergyPacket
    let packet = EnergyPacket {
        id: packet_id,
        asset_id: body.asset_id.clone(),
        status: PacketStatus::Pending,
        earliest_start: now,
        latest_start: None,
        target_energy_kwh,
        target_soc: body.target_soc,
        desired_power_kw,
        value_curve: ValueCurve {
            comfort_rates,
            deadline_tiers,
            active_tier_index: 0,
        },
        request_mode: UserRequestMode::ByDeadline,
        completion_policy,
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
    };

    // Build UserRequest (thin wrapper linking to the packet)
    let request_deadlines: Vec<crate::entities::user_request::RequestDeadline> = body
        .deadlines
        .iter()
        .map(|d| RequestDeadline {
            latest_end: d.latest_end,
            max_total_cost_eur: d.max_total_cost_eur,
            max_marginal_rate_eur_kwh: d.max_marginal_rate_eur_kwh,
            min_completion: d.min_completion.unwrap_or(0.8),
        })
        .collect();

    let user_request = UserRequest {
        id: request_id,
        asset_id: body.asset_id,
        target_soc: body.target_soc,
        target_energy_kwh,
        desired_power_kw,
        deadlines: request_deadlines,
        completion_policy: body.completion_policy.unwrap_or_else(|| "STOP".to_string()),
        max_total_cost_eur,
        tier_count,
        packet_id,
        status: UserRequestStatus::Active,
        estimated_cost_eur: 0.0,
        estimated_co2_g: 0.0,
        created_at: now,
        updated_at: now,
    };

    Ok((user_request, packet))
}

/// Compute target energy (kWh) and desired power (kW) from the request body.
fn resolve_target(
    body: &CreateUserRequestBody,
    profile: &Profile,
    sim: Option<&SimSnapshot>,
) -> Result<(f64, f64), RequestError> {
    // Explicit target energy wins
    if let Some(kwh) = body.target_energy_kwh {
        if kwh <= 0.0 {
            return Err(RequestError::ZeroEnergy);
        }
        let power = body.desired_power_kw.unwrap_or(1.0);
        return Ok((kwh, power));
    }

    match body.asset_id.as_str() {
        "ev" => {
            let ev_cfg = profile.devices.ev.as_ref().ok_or_else(|| {
                RequestError::UnknownAsset("ev".to_string())
            })?;
            let current_soc = sim
                .and_then(|s| s.ev.as_ref())
                .map(|e| e.soc)
                .unwrap_or(ev_cfg.initial_soc);
            let target_soc = body.target_soc.unwrap_or(ev_cfg.soc_target);
            let delta = (target_soc - current_soc).max(0.0);
            let kwh = delta * ev_cfg.battery_kwh;
            if kwh < 1e-6 {
                return Err(RequestError::ZeroEnergy);
            }
            let power = body.desired_power_kw.unwrap_or(ev_cfg.max_charge_kw);
            Ok((kwh, power))
        }
        "battery" => {
            let bat = profile.devices.battery.as_ref().ok_or_else(|| {
                RequestError::UnknownAsset("battery".to_string())
            })?;
            let current_soc = sim
                .and_then(|s| s.battery.as_ref())
                .map(|b| b.soc)
                .unwrap_or(bat.initial_soc);
            let target_soc = body.target_soc.unwrap_or(1.0);
            let delta = (target_soc - current_soc).max(0.0);
            let kwh = delta * bat.capacity_kwh;
            if kwh < 1e-6 {
                return Err(RequestError::ZeroEnergy);
            }
            let power = body.desired_power_kw.unwrap_or(bat.max_charge_kw);
            Ok((kwh, power))
        }
        other => Err(RequestError::UnknownAsset(other.to_string())),
    }
}
