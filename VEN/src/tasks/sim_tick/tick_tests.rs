#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use tokio::sync::{broadcast, watch, Mutex};

    use crate::controller::VtnPort;
    use crate::entities::asset::PlanTrigger;
    use crate::planner_events::PlannerEvent;
    use crate::services::test_support::mock_vtn::MockVtn;
    use crate::simulator::SimState;
    use crate::state::AppState;
    use crate::tasks::sim_tick::tick::{effective_pv_export_ceiling_kw, tick_once};

    #[test]
    fn effective_pv_export_ceiling_kw_takes_the_tighter_of_operator_and_vtn() {
        assert_eq!(
            effective_pv_export_ceiling_kw(Some(5.0), Some(3.0)),
            Some(3.0)
        );
        assert_eq!(
            effective_pv_export_ceiling_kw(Some(3.0), Some(5.0)),
            Some(3.0)
        );
    }

    #[test]
    fn effective_pv_export_ceiling_kw_falls_back_to_whichever_source_is_set() {
        assert_eq!(effective_pv_export_ceiling_kw(Some(3.0), None), Some(3.0));
        assert_eq!(effective_pv_export_ceiling_kw(None, Some(4.0)), Some(4.0));
    }

    #[test]
    fn effective_pv_export_ceiling_kw_is_none_when_neither_source_is_set() {
        assert_eq!(effective_pv_export_ceiling_kw(None, None), None);
    }

    fn minimal_sim() -> Arc<Mutex<SimState>> {
        let s: SimState = serde_json::from_value(serde_json::json!({
            "asset_configs": [],
            "assets": [],
            "grid": {
                "net_power_w": 0.0, "import_w": 0.0, "export_w": 0.0,
                "voltage_v": 0.0, "import_kwh": 0.0, "export_kwh": 0.0
            },
            "last_tick": chrono::Utc::now().to_rfc3339()
        }))
        .expect("minimal SimState must deserialize");
        Arc::new(Mutex::new(s))
    }

    #[tokio::test]
    async fn tick_once_runs_without_profile() {
        let sim = minimal_sim();
        let (trigger_tx, _trigger_rx) = watch::channel(PlanTrigger::Periodic);
        let trigger_tx = Arc::new(trigger_tx);
        let (event_bcast_tx, _) = broadcast::channel::<PlannerEvent>(1);
        let event_tx = Arc::new(event_bcast_tx);
        let vtn: Arc<dyn VtnPort> = Arc::new(MockVtn::new());

        let (_pc, _rc) = tick_once(
            AppState::new(),
            sim,
            "test-ven".to_string(),
            vtn,
            trigger_tx,
            "/tmp".to_string(),
            event_tx,
            0,   // persist_counter
            100, // persist_every_ticks — no persist this tick
            0,   // report_counter
            100, // report_every_ticks — no report this tick
            1,   // tick_s
            Arc::new(crate::controller::NoopWeatherPort),
            None, // weather_pv_params
        )
        .await;
        // passes if no panic
    }

    #[tokio::test]
    async fn tick_once_publishes_site_residual_asset() {
        // The physics engine derives grid.net_power_w as the literal sum of
        // its own modelled assets each tick (see `SimState::tick`'s "Derive
        // grid meter" step) — there is no independent meter reading in this
        // simulator (that would require a dedicated unmodelled-load
        // injection mechanism, out of WP5.1's scope). So with an empty asset
        // list, residual must land at exactly 0 kW; this test proves the
        // site-residual asset is wired into the published SimSnapshot at
        // all, not that a nonzero residual can be produced end-to-end (that
        // case is covered directly against `compute_site_residual_kw` in
        // `controller::residual`'s own unit tests).
        let s: SimState = serde_json::from_value(serde_json::json!({
            "asset_configs": [],
            "assets": [],
            "grid": {
                "net_power_w": 0.0, "import_w": 0.0, "export_w": 0.0,
                "voltage_v": 230.0, "import_kwh": 0.0, "export_kwh": 0.0
            },
            "last_tick": chrono::Utc::now().to_rfc3339()
        }))
        .expect("minimal SimState must deserialize");
        let sim = Arc::new(Mutex::new(s));

        let (trigger_tx, _trigger_rx) = watch::channel(PlanTrigger::Periodic);
        let trigger_tx = Arc::new(trigger_tx);
        let (event_bcast_tx, _) = broadcast::channel::<PlannerEvent>(1);
        let event_tx = Arc::new(event_bcast_tx);
        let vtn: Arc<dyn VtnPort> = Arc::new(MockVtn::new());
        let state = AppState::new();

        let (_pc, _rc) = tick_once(
            state.clone(),
            sim,
            "test-ven".to_string(),
            vtn,
            trigger_tx,
            "/tmp".to_string(),
            event_tx,
            0,   // persist_counter
            100, // persist_every_ticks — no persist this tick
            0,   // report_counter
            100, // report_every_ticks — no report this tick
            1,   // tick_s
            Arc::new(crate::controller::NoopWeatherPort),
            None, // weather_pv_params
        )
        .await;

        let sim_snap = state.sim().await.expect("sim snapshot must be published");
        let residual = sim_snap
            .assets
            .get(crate::controller::residual::SITE_RESIDUAL_ASSET_ID)
            .expect("site-residual asset must be present");
        assert!(
            residual.power_kw.abs() < 1e-9,
            "expected 0kW residual with no assets, got {}",
            residual.power_kw
        );
        assert_eq!(
            residual.asset_type,
            crate::controller::residual::SITE_RESIDUAL_ASSET_TYPE
        );
    }
}
