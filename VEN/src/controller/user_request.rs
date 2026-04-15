/// Stage 5 — User Request Manager: creates UserRequests from API bodies.
///
/// Validates the request body, resolves target energy from asset state,
/// and produces a UserRequest that links to an EvSession or HeaterTarget.
use crate::assets::AssetConfig;
use crate::entities::asset::ComfortRate;
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

/// Create a UserRequest from the POST /user-requests body.
///
/// Returns the UserRequest. The caller (hems.rs handler) is responsible for
/// creating and storing the appropriate device session (EvSession or HeaterTarget).
pub fn create_from_body(
    body: CreateUserRequestBody,
    assets: &[AssetEntry],
    asset_configs: &[AssetConfig],
    now: DateTime<Utc>,
) -> Result<UserRequest, RequestError> {
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

    // Build completion policy string for storage
    let completion_policy_str = body.completion_policy.unwrap_or_else(|| {
        use crate::entities::asset::CompletionPolicy;
        match cfg.default_completion_policy() {
            CompletionPolicy::Continue => "CONTINUE".to_string(),
            CompletionPolicy::Stop => "STOP".to_string(),
        }
    });

    // Build comfort rates (user-specified or asset default)
    let _comfort_rates: Vec<ComfortRate> = if let Some(ref rates) = body.comfort_rates {
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

    // Build deadline list from input
    let mut request_deadlines: Vec<RequestDeadline> = body
        .deadlines
        .iter()
        .map(|d| RequestDeadline {
            latest_end: d.latest_end,
            max_total_cost_eur: d.max_total_cost_eur,
            max_marginal_rate_eur_kwh: d.max_marginal_rate_eur_kwh,
            min_completion: d.min_completion.unwrap_or(0.8),
        })
        .collect();

    // Apply top-level budget_eur as first-tier cost ceiling if not already set
    if let Some(budget) = body.budget_eur {
        if let Some(first) = request_deadlines.first_mut() {
            if first.max_total_cost_eur.is_none() {
                first.max_total_cost_eur = Some(budget);
            }
        }
    }

    let tier_count = request_deadlines.len();
    let max_total_cost_eur = request_deadlines.first().and_then(|t| t.max_total_cost_eur);
    let interruptible = body.interruptible.unwrap_or(false);
    let tolerance_min = body.tolerance_min;

    let user_request = UserRequest {
        id: Uuid::new_v4(),
        asset_id: body.asset_id,
        target_soc: body.target_soc,
        target_energy_kwh,
        desired_power_kw,
        deadlines: request_deadlines,
        completion_policy: completion_policy_str,
        max_total_cost_eur,
        tier_count,
        session_id: None, // set by caller after device session is created
        status: UserRequestStatus::Active,
        estimated_cost_eur: 0.0,
        estimated_co2_g: 0.0,
        interruptible,
        tolerance_min,
        budget_eur: body.budget_eur,
        created_at: now,
        updated_at: now,
    };

    Ok(user_request)
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
