/// Stage 5 — User Request Manager: creates UserRequests from API bodies.
///
/// Validates the request body, resolves target energy from asset state,
/// and produces a UserRequest that links to an EvSession or HeaterTarget.
use crate::entities::asset::ComfortRate;
use crate::entities::asset_params::AssetRequestSlice;
use crate::entities::design_vocabulary::UserRequestMode;
use crate::entities::user_request::{RequestDeadline, UserRequest, UserRequestStatus};
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
    // ── Shiftable-load fields (Plan C) ──────────────────────────────────────
    pub power_kw: Option<f64>,
    pub duration_min: Option<u32>,
    pub earliest_start: Option<DateTime<Utc>>,
    pub latest_end: Option<DateTime<Utc>>,
    // ── Per-device overrides (Plan D) ────────────────────────────────────────
    pub soft_deadline: Option<bool>,
    pub target_temp_c: Option<f64>,
    // ── Request mode (BL-28) — omitted = BY_DEADLINE (legacy behaviour) ─────
    pub mode: Option<UserRequestMode>,
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
    /// Max gCO2/kWh the user accepts at this fill level (BL-17 comfort bidding).
    /// `None` = no CO2 preference expressed, treated as 0.0.
    pub co2: Option<f64>,
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
    asset_data: &[AssetRequestSlice],
    now: DateTime<Utc>,
) -> Result<UserRequest, RequestError> {
    if body.deadlines.is_empty() {
        return Err(RequestError::NoDeadlines);
    }

    let slice = asset_data
        .iter()
        .find(|s| s.id == body.asset_id)
        .ok_or_else(|| RequestError::UnknownAsset(body.asset_id.clone()))?;

    // Compute target energy and desired power
    let (target_energy_kwh, desired_power_kw) = if let Some(kwh) = body.target_energy_kwh {
        if kwh <= 0.0 {
            return Err(RequestError::ZeroEnergy);
        }
        (kwh, body.desired_power_kw.unwrap_or(1.0))
    } else {
        slice
            .resolve_request_target(body.target_soc, body.desired_power_kw)
            .ok_or(RequestError::ZeroEnergy)?
    };

    // Build completion policy string for storage
    let completion_policy_str = body.completion_policy.unwrap_or_else(|| {
        use crate::entities::asset::CompletionPolicy;
        match slice.completion_policy {
            CompletionPolicy::Continue => "CONTINUE".to_string(),
            CompletionPolicy::Stop => "STOP".to_string(),
        }
    });

    // Build comfort rates (user-specified or asset default)
    let comfort_rates: Vec<ComfortRate> = if let Some(ref rates) = body.comfort_rates {
        rates
            .iter()
            .map(|r| ComfortRate {
                fill: r.fill,
                max_marginal_price: r.bid,
                max_marginal_co2: r.co2.unwrap_or(0.0),
            })
            .collect()
    } else {
        slice.comfort_rates.clone()
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
        mode: body.mode.unwrap_or_default(),
        completion_policy: completion_policy_str,
        max_total_cost_eur,
        tier_count,
        session_id: None,   // set by caller after device session is created
        session_type: None, // set by caller (Ev, Heater, etc.)
        comfort_rates,
        status: UserRequestStatus::Active,
        estimated_cost_eur: 0.0,
        estimated_co2_g: 0.0,
        accumulated_cost_eur: 0.0,
        interruptible,
        tolerance_min,
        budget_eur: body.budget_eur,
        created_at: now,
        updated_at: now,
    };

    Ok(user_request)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entities::asset::CompletionPolicy;
    use crate::entities::asset_params::AssetRequestSlice;
    use chrono::TimeZone;

    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 8, 15, 12, 0, 0).unwrap()
    }

    fn far_future() -> DateTime<Utc> {
        now() + chrono::Duration::hours(6)
    }

    fn slice(id: &str) -> AssetRequestSlice {
        AssetRequestSlice {
            id: id.to_string(),
            current_soc: Some(0.3),
            default_soc_target: Some(0.8),
            capacity_kwh: Some(10.0),
            max_charge_kw: Some(3.7),
            completion_policy: CompletionPolicy::Continue,
            comfort_rates: vec![],
        }
    }

    fn deadline_input(max_total_cost_eur: Option<f64>) -> RequestDeadlineInput {
        RequestDeadlineInput {
            latest_end: far_future(),
            max_total_cost_eur,
            max_marginal_rate_eur_kwh: None,
            min_completion: None,
        }
    }

    fn base_body() -> CreateUserRequestBody {
        CreateUserRequestBody {
            asset_id: "ev".to_string(),
            target_soc: None,
            target_energy_kwh: None,
            desired_power_kw: None,
            deadlines: vec![deadline_input(None)],
            completion_policy: None,
            comfort_rates: None,
            budget_eur: None,
            interruptible: None,
            tolerance_min: None,
            power_kw: None,
            duration_min: None,
            earliest_start: None,
            latest_end: None,
            soft_deadline: None,
            target_temp_c: None,
            mode: None,
        }
    }

    #[test]
    fn no_deadlines_is_rejected() {
        let mut body = base_body();
        body.deadlines = vec![];
        let err = create_from_body(body, &[slice("ev")], now()).unwrap_err();
        assert!(matches!(err, RequestError::NoDeadlines));
    }

    #[test]
    fn unknown_asset_id_is_rejected() {
        let body = base_body();
        // asset_data only knows "battery", not "ev" (the body's asset_id)
        let err = create_from_body(body, &[slice("battery")], now()).unwrap_err();
        match err {
            RequestError::UnknownAsset(id) => assert_eq!(id, "ev"),
            other => panic!("expected UnknownAsset, got {other:?}"),
        }
    }

    #[test]
    fn explicit_target_energy_kwh_zero_is_rejected() {
        let mut body = base_body();
        body.target_energy_kwh = Some(0.0);
        let err = create_from_body(body, &[slice("ev")], now()).unwrap_err();
        assert!(matches!(err, RequestError::ZeroEnergy));
    }

    #[test]
    fn explicit_target_energy_kwh_negative_is_rejected() {
        let mut body = base_body();
        body.target_energy_kwh = Some(-2.5);
        let err = create_from_body(body, &[slice("ev")], now()).unwrap_err();
        assert!(matches!(err, RequestError::ZeroEnergy));
    }

    #[test]
    fn asset_already_at_or_above_target_soc_is_rejected() {
        // current_soc (0.9) already meets default_soc_target (0.8) -- resolve_request_target
        // returns None, and create_from_body must surface that as ZeroEnergy, not panic
        // or silently create a zero-energy request.
        let mut s = slice("ev");
        s.current_soc = Some(0.9);
        s.default_soc_target = Some(0.8);
        let body = base_body();
        let err = create_from_body(body, &[s], now()).unwrap_err();
        assert!(matches!(err, RequestError::ZeroEnergy));
    }

    #[test]
    fn resolves_target_energy_from_soc_gap_when_not_explicit() {
        // current_soc=0.3, target_soc=0.8, capacity=10 kWh -> 5.0 kWh needed
        let s = slice("ev");
        let body = base_body();
        let req = create_from_body(body, &[s], now()).unwrap();
        assert!((req.target_energy_kwh - 5.0).abs() < 1e-9);
        // desired_power_kw falls back to the asset's own max_charge_kw
        assert!((req.desired_power_kw - 3.7).abs() < 1e-9);
    }

    #[test]
    fn resolves_target_energy_using_explicit_target_soc_over_asset_default() {
        let s = slice("ev"); // current_soc=0.3, default_soc_target=0.8
        let mut body = base_body();
        body.target_soc = Some(0.6); // overrides the asset's default_soc_target of 0.8
        let req = create_from_body(body, &[s], now()).unwrap();
        // (0.6 - 0.3) * 10.0 = 3.0 kWh, not (0.8-0.3)*10=5.0
        assert!((req.target_energy_kwh - 3.0).abs() < 1e-9);
    }

    #[test]
    fn explicit_target_energy_kwh_defaults_desired_power_to_one_kw_when_unspecified() {
        let mut body = base_body();
        body.target_energy_kwh = Some(8.0);
        body.desired_power_kw = None;
        let req = create_from_body(body, &[slice("ev")], now()).unwrap();
        assert!((req.target_energy_kwh - 8.0).abs() < 1e-9);
        assert!((req.desired_power_kw - 1.0).abs() < 1e-9);
    }

    #[test]
    fn explicit_desired_power_kw_overrides_asset_max_charge_kw() {
        let mut body = base_body();
        body.target_energy_kwh = Some(8.0);
        body.desired_power_kw = Some(2.2);
        let req = create_from_body(body, &[slice("ev")], now()).unwrap();
        assert!((req.desired_power_kw - 2.2).abs() < 1e-9);
    }

    #[test]
    fn completion_policy_defaults_from_asset_continue() {
        let mut s = slice("ev");
        s.completion_policy = CompletionPolicy::Continue;
        let body = base_body();
        let req = create_from_body(body, &[s], now()).unwrap();
        assert_eq!(req.completion_policy, "CONTINUE");
    }

    #[test]
    fn completion_policy_defaults_from_asset_stop() {
        let mut s = slice("ev");
        s.completion_policy = CompletionPolicy::Stop;
        let body = base_body();
        let req = create_from_body(body, &[s], now()).unwrap();
        assert_eq!(req.completion_policy, "STOP");
    }

    #[test]
    fn completion_policy_explicit_override_wins_over_asset_default() {
        let mut s = slice("ev");
        s.completion_policy = CompletionPolicy::Continue;
        let mut body = base_body();
        body.completion_policy = Some("STOP".to_string());
        let req = create_from_body(body, &[s], now()).unwrap();
        assert_eq!(req.completion_policy, "STOP");
    }

    #[test]
    fn comfort_rates_default_from_asset_when_unspecified() {
        let mut s = slice("ev");
        s.comfort_rates = vec![ComfortRate {
            fill: 0.5,
            max_marginal_price: 0.3,
            max_marginal_co2: 200.0,
        }];
        let body = base_body();
        let req = create_from_body(body, &[s], now()).unwrap();
        assert_eq!(req.comfort_rates.len(), 1);
        assert!((req.comfort_rates[0].max_marginal_co2 - 200.0).abs() < 1e-9);
    }

    #[test]
    fn comfort_rates_explicit_override_replaces_asset_default_co2_defaults_to_zero_when_omitted() {
        // Explicit comfort_rates in the request body override the asset's own default curve
        // entirely (a different curve, different fill/price). When the override's `co2` field
        // is omitted, it defaults to 0.0 -- distinct from the asset default's own CO2 value,
        // proving the override genuinely replaces rather than merges with the asset default.
        let mut s = slice("ev");
        s.comfort_rates = vec![ComfortRate {
            fill: 0.5,
            max_marginal_price: 0.3,
            max_marginal_co2: 200.0,
        }];
        let mut body = base_body();
        body.comfort_rates = Some(vec![ComfortRateInput {
            fill: 1.0,
            bid: 0.5,
            co2: None,
        }]);
        let req = create_from_body(body, &[s], now()).unwrap();
        assert_eq!(req.comfort_rates.len(), 1);
        assert!((req.comfort_rates[0].fill - 1.0).abs() < 1e-9);
        assert!((req.comfort_rates[0].max_marginal_price - 0.5).abs() < 1e-9);
        assert!((req.comfort_rates[0].max_marginal_co2 - 0.0).abs() < 1e-9);
    }

    #[test]
    fn comfort_rates_explicit_override_co2_bid_is_passed_through_when_given() {
        // BL-17 comfort bidding: an explicit CO2 bid in the request body must actually reach
        // the resolved ComfortRate, not be silently dropped -- this is the exact gap the whole
        // CO2-comfort-bidding feature exists to close.
        let mut body = base_body();
        body.comfort_rates = Some(vec![ComfortRateInput {
            fill: 1.0,
            bid: 0.5,
            co2: Some(150.0),
        }]);
        let req = create_from_body(body, &[slice("ev")], now()).unwrap();
        assert_eq!(req.comfort_rates.len(), 1);
        assert!((req.comfort_rates[0].max_marginal_co2 - 150.0).abs() < 1e-9);
    }

    #[test]
    fn budget_eur_backfills_first_deadline_cost_ceiling_when_unset() {
        let mut body = base_body();
        body.deadlines = vec![deadline_input(None)];
        body.budget_eur = Some(50.0);
        let req = create_from_body(body, &[slice("ev")], now()).unwrap();
        assert_eq!(req.deadlines[0].max_total_cost_eur, Some(50.0));
        assert_eq!(req.max_total_cost_eur, Some(50.0));
    }

    #[test]
    fn budget_eur_does_not_override_an_already_set_deadline_cost_ceiling() {
        let mut body = base_body();
        body.deadlines = vec![deadline_input(Some(20.0))];
        body.budget_eur = Some(50.0); // must not clobber the explicit per-deadline ceiling
        let req = create_from_body(body, &[slice("ev")], now()).unwrap();
        assert_eq!(req.deadlines[0].max_total_cost_eur, Some(20.0));
    }

    #[test]
    fn tier_count_reflects_number_of_deadlines() {
        let mut body = base_body();
        body.deadlines = vec![
            deadline_input(Some(20.0)),
            deadline_input(None),
            deadline_input(None),
        ];
        let req = create_from_body(body, &[slice("ev")], now()).unwrap();
        assert_eq!(req.tier_count, 3);
        // max_total_cost_eur is API convenience for the *first* tier only
        assert_eq!(req.max_total_cost_eur, Some(20.0));
    }

    #[test]
    fn min_completion_defaults_to_0_8_when_unspecified() {
        let body = base_body(); // deadline_input(None) has min_completion: None
        let req = create_from_body(body, &[slice("ev")], now()).unwrap();
        assert!((req.deadlines[0].min_completion - 0.8).abs() < 1e-9);
    }

    #[test]
    fn mode_defaults_to_by_deadline_when_unspecified() {
        let body = base_body();
        let req = create_from_body(body, &[slice("ev")], now()).unwrap();
        assert_eq!(req.mode, UserRequestMode::ByDeadline);
    }

    #[test]
    fn mode_explicit_value_is_passed_through() {
        let mut body = base_body();
        body.mode = Some(UserRequestMode::Asap);
        let req = create_from_body(body, &[slice("ev")], now()).unwrap();
        assert_eq!(req.mode, UserRequestMode::Asap);
    }

    #[test]
    fn interruptible_defaults_to_false_when_unspecified() {
        let body = base_body();
        let req = create_from_body(body, &[slice("ev")], now()).unwrap();
        assert!(!req.interruptible);
    }

    #[test]
    fn interruptible_explicit_true_is_passed_through() {
        let mut body = base_body();
        body.interruptible = Some(true);
        let req = create_from_body(body, &[slice("ev")], now()).unwrap();
        assert!(req.interruptible);
    }
}
