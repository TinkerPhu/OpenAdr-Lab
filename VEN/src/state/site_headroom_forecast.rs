//! Forward-looking per-slot headroom trajectory, re-derived fresh every
//! dispatcher tick (see `SiteFlexibilityForecastSlot`'s doc comment for why).
//! A plain replace-on-tick cache, not a ring like `flexibility_history.rs` —
//! each tick already produces the whole remaining-horizon trajectory, not
//! one more sample to retain.

use crate::entities::plan::SiteFlexibilityForecastSlot;

use super::AppState;

impl AppState {
    pub async fn site_headroom_forecast(&self) -> Vec<SiteFlexibilityForecastSlot> {
        self.hems.read().await.site_headroom_forecast.clone()
    }

    pub async fn set_site_headroom_forecast(&self, forecast: Vec<SiteFlexibilityForecastSlot>) {
        self.hems.write().await.site_headroom_forecast = forecast;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn slot(up_kw: f64) -> SiteFlexibilityForecastSlot {
        SiteFlexibilityForecastSlot {
            ts: Utc::now(),
            up_kw,
            down_kw: 0.0,
        }
    }

    #[tokio::test]
    async fn empty_before_the_first_tick() {
        let state = AppState::new();
        assert!(state.site_headroom_forecast().await.is_empty());
    }

    #[tokio::test]
    async fn set_replaces_the_whole_trajectory_not_a_ring() {
        let state = AppState::new();
        state
            .set_site_headroom_forecast(vec![slot(1.0), slot(2.0)])
            .await;
        state.set_site_headroom_forecast(vec![slot(3.0)]).await;
        let forecast = state.site_headroom_forecast().await;
        assert_eq!(forecast.len(), 1);
        assert_eq!(forecast[0].up_kw, 3.0);
    }
}
