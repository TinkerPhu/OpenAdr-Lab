/// Stage 5 — User Request Manager: creates EnergyPackets from user requests.
///
/// Provides the logic to translate a POST /requests body into an EnergyPacket
/// with proper ValueCurve (multi-tier deadlines, comfort rates), and computes
/// target energy from the asset's current state + profile.
use crate::assets::AssetConfig;
use crate::entities::asset::{ComfortRate, CompletionPolicy};
use crate::entities::energy_packet::{DeadlineTier, EnergyPacket, ValueCurve};
use crate::entities::user_request::{RequestDeadline, UserRequest, UserRequestStatus};
use crate::simulator::AssetEntry;
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
    // ── Leeway fields (§8.2) ────────────────────────────────────────────────
    pub budget_eur: Option<f64>,     // top-level cost ceiling shorthand
    pub interruptible: Option<bool>, // planner may pause/resume
    pub tolerance_min: Option<i64>,  // ±N minutes around deadline acceptable
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
            RequestError::ZeroEnergy => write!(f, "computed target_energy_kwh is zero or negative (asset may already be at or above the target SoC)"),
        }
    }
}

/// Create a (UserRequest, EnergyPacket) pair from the POST /requests body.
pub fn create_from_body(
    body: CreateUserRequestBody,
    assets: &[AssetEntry],
    asset_configs: &[AssetConfig],
    now: DateTime<Utc>,
) -> Result<(UserRequest, EnergyPacket), RequestError> {
    if body.deadlines.is_empty() {
        return Err(RequestError::NoDeadlines);
    }

    // Look up asset entry + config for defaults
    let idx = assets
        .iter()
        .position(|a| a.id == body.asset_id)
        .ok_or_else(|| RequestError::UnknownAsset(body.asset_id.clone()))?;
    let entry = &assets[idx];
    let cfg = &asset_configs[idx];

    // Compute target energy and desired power
    let (target_energy_kwh, desired_power_kw) = resolve_target(&body, entry, cfg)?;

    // Build completion policy (user-specified or asset default)
    let completion_policy = match body.completion_policy.as_deref() {
        Some("CONTINUE") => CompletionPolicy::Continue,
        Some(_) => CompletionPolicy::Stop,
        None => cfg.default_completion_policy(),
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
        cfg.default_comfort_rates()
    };

    // Build deadline tiers from input
    let mut deadline_tiers: Vec<DeadlineTier> = body
        .deadlines
        .iter()
        .map(|d| DeadlineTier {
            deadline: d.latest_end,
            max_total_cost_eur: d.max_total_cost_eur,
            max_marginal_rate_eur_kwh: d.max_marginal_rate_eur_kwh,
            min_completion: d.min_completion.unwrap_or(0.8),
        })
        .collect();

    // Apply top-level budget_eur as first-tier cost ceiling if not already set
    if let Some(budget) = body.budget_eur {
        if let Some(first) = deadline_tiers.first_mut() {
            if first.max_total_cost_eur.is_none() {
                first.max_total_cost_eur = Some(budget);
            }
        }
    }

    let tier_count = deadline_tiers.len();
    let max_total_cost_eur = deadline_tiers.first().and_then(|t| t.max_total_cost_eur);

    let request_id = Uuid::new_v4();

    // Build EnergyPacket
    let value_curve = ValueCurve {
        comfort_rates,
        deadline_tiers,
        active_tier_index: 0,
    };
    let interruptible = body.interruptible.unwrap_or(false);
    let tolerance_min = body.tolerance_min;

    let packet = EnergyPacket {
        target_soc: body.target_soc,
        completion_policy,
        post_deadline_comfort_bid: cfg.default_post_deadline_comfort_bid(),
        interruptible,
        tolerance_min,
        ..EnergyPacket::new(
            body.asset_id.clone(),
            target_energy_kwh,
            desired_power_kw,
            value_curve,
            now,
        )
    };

    // Build UserRequest (thin wrapper linking to the packet).
    // Derive deadlines from the packet's value_curve so budget_eur mutation is reflected.
    let request_deadlines: Vec<crate::entities::user_request::RequestDeadline> = packet
        .value_curve
        .deadline_tiers
        .iter()
        .map(|t| RequestDeadline {
            latest_end: t.deadline,
            max_total_cost_eur: t.max_total_cost_eur,
            max_marginal_rate_eur_kwh: t.max_marginal_rate_eur_kwh,
            min_completion: t.min_completion,
        })
        .collect();

    let user_request = UserRequest {
        id: request_id,
        asset_id: body.asset_id,
        target_soc: body.target_soc,
        target_energy_kwh,
        desired_power_kw,
        deadlines: request_deadlines,
        completion_policy: body.completion_policy.unwrap_or_else(|| {
            match cfg.default_completion_policy() {
                CompletionPolicy::Continue => "CONTINUE".to_string(),
                CompletionPolicy::Stop => "STOP".to_string(),
            }
        }),
        max_total_cost_eur,
        tier_count,
        packet_id: packet.id,
        status: UserRequestStatus::Active,
        estimated_cost_eur: 0.0,
        estimated_co2_g: 0.0,
        interruptible,
        tolerance_min,
        budget_eur: body.budget_eur,
        created_at: now,
        updated_at: now,
    };

    Ok((user_request, packet))
}

/// Compute target energy (kWh) and desired power (kW) from the request body.
fn resolve_target(
    body: &CreateUserRequestBody,
    entry: &AssetEntry,
    cfg: &AssetConfig,
) -> Result<(f64, f64), RequestError> {
    // Explicit target energy wins
    if let Some(kwh) = body.target_energy_kwh {
        if kwh <= 0.0 {
            return Err(RequestError::ZeroEnergy);
        }
        let power = body.desired_power_kw.unwrap_or(1.0);
        return Ok((kwh, power));
    }

    cfg.resolve_request_target(&entry.state, body.target_soc, body.desired_power_kw)
        .ok_or(RequestError::ZeroEnergy)
}
