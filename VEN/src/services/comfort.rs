//! WP4.2 (BL-19) — user comfort-curve overrides.
//!
//! A user may replace an asset's built-in `default_comfort_rates()` with
//! their own bid curve. Overrides live in a hot in-memory map on `AppState`
//! (read on every user-request build) and persist through `SettingsPort`
//! (`user_settings` table) so they survive restarts; the map is re-seeded
//! from the store at startup.

use std::sync::Arc;

use chrono::{DateTime, Utc};
use tracing::warn;

use crate::controller::settings_port::SETTING_COMFORT_CURVE;
use crate::controller::SettingsPort;
use crate::entities::asset::ComfortRate;
use crate::entities::comfort::validate_curve;
use crate::state::AppState;

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
        crate::entities::comfort::curve_point(fill, bid)
    }

    // ── ev-comfort-piecewise-core: bids must not rise with fill ──────────
    //
    // The EV prices energy by walking this curve, one continuous variable per
    // segment. That is only exactly solvable without binaries while the bids
    // are non-increasing — a rising bid would make the solver prefer the LAST
    // kWh over the first, i.e. top up beyond the target before filling toward
    // it. Rejecting here is what keeps the solver binary-free.

    /// The rule must break no data this project actually ships. Note the
    /// profile YAMLs' own `packets:` blocks carry `comfort_rates` too, but
    /// nothing in `VEN/src` parses `packets` — the curves that reach the
    /// planner are these built-in defaults and user overrides.
    #[test]
    fn every_built_in_default_curve_satisfies_the_new_rule() {
        use crate::assets::base_load::BaseLoad;
        use crate::assets::ev::EvCharger;
        use crate::assets::heater::Heater;
        use crate::assets::Asset;
        use crate::entities::asset_params::{BaseLoadParams, EvParams, HeaterParams};

        let curves: Vec<(&str, Vec<ComfortRate>)> = vec![
            (
                "ev",
                EvCharger::from_params(&EvParams::default()).default_comfort_rates(),
            ),
            (
                "heater",
                Heater::from_params(&HeaterParams::default()).default_comfort_rates(),
            ),
            (
                "base_load",
                BaseLoad::from_params(&BaseLoadParams::default()).default_comfort_rates(),
            ),
        ];
        for (name, rates) in curves {
            assert!(
                validate_curve(&rates).is_ok(),
                "{name}'s built-in default curve must still validate: {:?}",
                validate_curve(&rates)
            );
        }
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
