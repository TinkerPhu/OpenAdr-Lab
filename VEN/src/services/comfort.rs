//! WP4.2 (BL-19) — user comfort-curve overrides.
//!
//! A user may replace an asset's built-in `default_comfort_rates()` with
//! their own bid curve. Overrides live in a hot in-memory map on `AppState`
//! (read on every user-request build) and persist through `SettingsPort`
//! (`user_settings` table) so they survive restarts; the map is re-seeded
//! from the store at startup.

use std::collections::HashMap;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use tracing::warn;

use crate::controller::settings_port::SETTING_COMFORT_CURVE;
use crate::controller::SettingsPort;
use crate::entities::asset::ComfortRate;
use crate::state::AppState;

/// Bid-price sanity bound [€/kWh] — an order of magnitude above any real tariff.
const MAX_BID_EUR_KWH: f64 = 10.0;

/// Validate a user-provided curve: non-empty, all values finite, fills in
/// [0, 1] and strictly increasing, bids bounded.
pub fn validate_curve(rates: &[ComfortRate]) -> Result<(), String> {
    if rates.is_empty() {
        return Err("comfort curve must contain at least one point".into());
    }
    let mut prev_fill = -1.0_f64;
    for (i, r) in rates.iter().enumerate() {
        if !(r.fill.is_finite()
            && r.max_marginal_price.is_finite()
            && r.max_marginal_co2.is_finite())
        {
            return Err(format!("point {i}: values must be finite"));
        }
        if !(0.0..=1.0).contains(&r.fill) {
            return Err(format!("point {i}: fill {} outside [0, 1]", r.fill));
        }
        if r.fill <= prev_fill {
            return Err(format!(
                "point {i}: fill {} not strictly increasing (previous {prev_fill})",
                r.fill
            ));
        }
        if !(0.0..=MAX_BID_EUR_KWH).contains(&r.max_marginal_price) {
            return Err(format!(
                "point {i}: max_marginal_price {} outside [0, {MAX_BID_EUR_KWH}] €/kWh",
                r.max_marginal_price
            ));
        }
        if r.max_marginal_co2 < 0.0 {
            return Err(format!("point {i}: max_marginal_co2 must be ≥ 0"));
        }
        prev_fill = r.fill;
    }
    Ok(())
}

/// The curve planning should use: the user override when present, the
/// asset's built-in default otherwise (BL-19 verify clause).
pub fn effective_comfort_rates(
    overrides: &HashMap<String, Vec<ComfortRate>>,
    asset_id: &str,
    default_rates: Vec<ComfortRate>,
) -> Vec<ComfortRate> {
    overrides.get(asset_id).cloned().unwrap_or(default_rates)
}

/// Validate, store in the hot map, persist. Persistence failures are logged,
/// not propagated — the override is live for this run either way.
pub async fn set_override(
    state: &AppState,
    settings: Option<Arc<dyn SettingsPort>>,
    now: DateTime<Utc>,
    asset_id: &str,
    rates: Vec<ComfortRate>,
) -> Result<(), String> {
    validate_curve(&rates)?;
    state
        .set_comfort_override(asset_id.to_string(), rates.clone())
        .await;
    if let Some(s) = settings {
        let json = serde_json::to_string(&rates).map_err(|e| e.to_string())?;
        let aid = asset_id.to_string();
        let res = tokio::task::spawn_blocking(move || {
            s.put_setting(SETTING_COMFORT_CURVE, &aid, &json, now)
        })
        .await;
        if let Ok(Err(e)) = res {
            warn!(error = %e, asset_id, "comfort-curve persist failed");
        }
    }
    Ok(())
}

/// Remove the override (restoring the built-in default). Returns whether one existed.
pub async fn clear_override(
    state: &AppState,
    settings: Option<Arc<dyn SettingsPort>>,
    asset_id: &str,
) -> bool {
    let existed = state.remove_comfort_override(asset_id).await;
    if let Some(s) = settings {
        let aid = asset_id.to_string();
        let res =
            tokio::task::spawn_blocking(move || s.delete_setting(SETTING_COMFORT_CURVE, &aid))
                .await;
        if let Ok(Err(e)) = res {
            warn!(error = %e, asset_id, "comfort-curve delete failed");
        }
    }
    existed
}

/// Startup: re-seed the hot map from the store. Rows that fail to parse are
/// logged and skipped (never block startup on one bad row).
pub async fn load_overrides(state: &AppState, settings: Arc<dyn SettingsPort>) {
    let rows =
        tokio::task::spawn_blocking(move || settings.settings_for_key(SETTING_COMFORT_CURVE)).await;
    let rows = match rows {
        Ok(Ok(rows)) => rows,
        Ok(Err(e)) => {
            warn!(error = %e, "loading comfort-curve overrides failed");
            return;
        }
        Err(e) => {
            warn!(error = %e, "loading comfort-curve overrides panicked");
            return;
        }
    };
    for (asset_id, json) in rows {
        match serde_json::from_str::<Vec<ComfortRate>>(&json) {
            Ok(rates) => state.set_comfort_override(asset_id, rates).await,
            Err(e) => warn!(error = %e, asset_id, "stored comfort curve unparseable — skipped"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::history_store::SqliteHistoryStore;

    fn pt(fill: f64, bid: f64) -> ComfortRate {
        ComfortRate {
            fill,
            max_marginal_price: bid,
            max_marginal_co2: 0.0,
        }
    }

    #[test]
    fn test_validate_curve_accepts_monotonic_bounded() {
        assert!(validate_curve(&[pt(0.5, 0.40), pt(0.8, 0.25), pt(1.0, 0.10)]).is_ok());
    }

    #[test]
    fn test_validate_curve_rejects_bad_input() {
        assert!(validate_curve(&[]).is_err(), "empty");
        assert!(
            validate_curve(&[pt(0.8, 0.2), pt(0.5, 0.3)]).is_err(),
            "non-monotonic fill"
        );
        assert!(
            validate_curve(&[pt(0.5, 0.2), pt(0.5, 0.3)]).is_err(),
            "duplicate fill"
        );
        assert!(validate_curve(&[pt(1.2, 0.2)]).is_err(), "fill > 1");
        assert!(validate_curve(&[pt(0.5, -0.1)]).is_err(), "negative bid");
        assert!(validate_curve(&[pt(0.5, 99.0)]).is_err(), "bid unbounded");
        assert!(validate_curve(&[pt(f64::NAN, 0.2)]).is_err(), "NaN");
    }

    #[test]
    fn test_effective_comfort_rates_prefers_override() {
        let mut overrides = HashMap::new();
        overrides.insert("ev".to_string(), vec![pt(0.9, 0.50)]);
        let default_rates = vec![pt(0.8, 0.30)];

        let eff = effective_comfort_rates(&overrides, "ev", default_rates.clone());
        assert!(
            (eff[0].max_marginal_price - 0.50).abs() < 1e-9,
            "override wins"
        );

        let eff = effective_comfort_rates(&overrides, "heater", default_rates);
        assert!(
            (eff[0].max_marginal_price - 0.30).abs() < 1e-9,
            "no override → built-in default"
        );
    }

    #[tokio::test]
    async fn test_set_override_persists_and_clear_restores() {
        let state = AppState::new();
        let store: Arc<SqliteHistoryStore> = Arc::new(SqliteHistoryStore::in_memory().unwrap());
        let settings: Arc<dyn SettingsPort> = store;
        let now = Utc::now();

        set_override(
            &state,
            Some(settings.clone()),
            now,
            "ev",
            vec![pt(0.9, 0.5)],
        )
        .await
        .unwrap();
        assert!(state.comfort_overrides_map().await.contains_key("ev"));
        assert!(settings
            .get_setting(SETTING_COMFORT_CURVE, "ev")
            .unwrap()
            .is_some());

        // A fresh state seeded from the same store sees the override (restart).
        let state2 = AppState::new();
        load_overrides(&state2, settings.clone()).await;
        assert!(state2.comfort_overrides_map().await.contains_key("ev"));

        assert!(clear_override(&state, Some(settings.clone()), "ev").await);
        assert!(!state.comfort_overrides_map().await.contains_key("ev"));
        assert!(settings
            .get_setting(SETTING_COMFORT_CURVE, "ev")
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn test_set_override_rejects_invalid_curve() {
        let state = AppState::new();
        let err = set_override(&state, None, Utc::now(), "ev", vec![]).await;
        assert!(err.is_err());
        assert!(state.comfort_overrides_map().await.is_empty());
    }
}
