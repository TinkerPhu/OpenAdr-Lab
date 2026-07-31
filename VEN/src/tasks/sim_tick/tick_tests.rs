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
    use crate::tasks::sim_tick::tick::tick_once;

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

    // ── deviation arbiter: §5.1/§5.3 regression safeguard, tick_once-level ──
    //
    // Full multi-tick oscillation-shape and lever-switching-chatter proofs
    // live at the unit level (`controller::arbiter::arbiter_tests`:
    // `battery_lever_converges_under_stationary_disturbance_across_multiple_ticks`,
    // `near_equal_cost_levers_do_not_switch_every_tick`), which exercise the
    // exact mechanisms responsible for both properties. This test is the
    // narrower `tick_once`-level smoke proof: the arbiter runs end-to-end
    // through the real tick loop (plan → dispatcher → arbiter → physics)
    // without panicking and visibly moves the battery to absorb a live PV
    // surplus the plan didn't expect, confirming the wiring itself — not just
    // the arbiter's internal functions in isolation — is correct.
    fn battery_pv_sim() -> Arc<Mutex<SimState>> {
        use crate::entities::asset_params::{AssetParams, BatteryParams, PvParams};
        let s = SimState::from_params(
            &[
                AssetParams::Battery(BatteryParams {
                    id: crate::ids::ASSET_BATTERY.to_string(),
                    capacity_kwh: 10.0,
                    max_charge_kw: 5.0,
                    max_discharge_kw: 5.0,
                    initial_soc: 0.5,
                    round_trip_efficiency: 0.95,
                    min_soc: 0.1,
                    c_terminal_eur_kwh: None,
                }),
                AssetParams::Pv(PvParams {
                    id: crate::ids::ASSET_PV.to_string(),
                    rated_kw: 5.0,
                    inverter_max_kw: 5.0,
                }),
            ],
            chrono::Utc::now(),
        );
        Arc::new(Mutex::new(s))
    }

    fn minimal_plan_no_pv_expected(
        now: chrono::DateTime<chrono::Utc>,
    ) -> crate::entities::plan::Plan {
        use crate::entities::plan::{
            CostBreakdown, Plan, PlanSummary, PlanTimeSlot, PlanZone, PlanningHorizon,
        };
        use chrono::Duration;
        use uuid::Uuid;
        let slot = PlanTimeSlot {
            slot_index: 0,
            start: now - Duration::seconds(60),
            end: now + Duration::seconds(600),
            import_tariff_eur_kwh: 0.25,
            export_tariff_eur_kwh: 0.08,
            co2_g_kwh: 300.0,
            grid_effective_cost: 0.25,
            marginal_cost_import_eur_per_kwh: 0.25,
            marginal_cost_export_eur_per_kwh: 0.08,
            rate_estimated: false,
            import_cap_kw: 25.0,
            export_cap_kw: 10.0,
            baseline_kw: 0.0,
            pv_forecast_kw: 0.0,
            pv_used_kw: 0.0,
            surplus_available_kw: 0.0,
            allocations: vec![],
            net_import_kw: 0.0,
            net_export_kw: 0.0,
            import_flexibility_kw: 0.0,
            export_flexibility_kw: 0.0,
            bat_charge_kw: 0.0,
            bat_discharge_kw: 0.0,
            planned_kw_by_asset: std::collections::HashMap::new(),
            planned_state_by_asset: std::collections::HashMap::new(),
        };
        Plan {
            id: Uuid::new_v4(),
            created_at: now,
            trigger: PlanTrigger::Periodic,
            horizon: PlanningHorizon {
                start_time: now,
                end_time: now + Duration::seconds(600),
                step_size_s: 600,
                num_steps: 1,
                far_horizon: now + Duration::seconds(600),
                zones: vec![PlanZone {
                    step_s: 600,
                    slots: 1,
                }],
            },
            slots: vec![slot],
            summary: PlanSummary::default(),
            envelopes: vec![],
            warnings: vec![],
            soc_trajectory_kwh: vec![],
            objective: crate::entities::PlannerObjective::MinCost,
            objective_eur: 0.0,
            friction_eur: 0.0,
            cost_breakdown: CostBreakdown::default(),
            solve_status: crate::entities::plan::SolveStatus::Optimal,
        }
    }

    #[tokio::test]
    async fn deviation_arbiter_absorbs_unplanned_pv_surplus_end_to_end() {
        let sim = battery_pv_sim();
        let (trigger_tx, _trigger_rx) = watch::channel(PlanTrigger::Periodic);
        let trigger_tx = Arc::new(trigger_tx);
        let (event_bcast_tx, _) = broadcast::channel::<PlannerEvent>(1);
        let event_tx = Arc::new(event_bcast_tx);
        let vtn: Arc<dyn VtnPort> = Arc::new(MockVtn::new());
        let state = AppState::new();

        let now = chrono::Utc::now();
        state
            .set_active_plan(Some(minimal_plan_no_pv_expected(now)))
            .await;
        state.set_deviation_arbiter_enabled(true).await;
        // Force full PV output — a surplus the plan above never expected
        // (pv_forecast_kw/pv_used_kw = 0.0).
        let mut inject = state.inject_state().await;
        inject.pv_irradiance = Some(1.0);
        state.set_inject_state(inject).await;

        let (_pc, _rc) = tick_once(
            state.clone(),
            sim,
            "test-ven".to_string(),
            vtn,
            trigger_tx,
            "/tmp".to_string(),
            event_tx,
            0,
            100,
            0,
            100,
            1,
            Arc::new(crate::controller::NoopWeatherPort),
            None,
        )
        .await;

        let sim_snap = state.sim().await.expect("sim snapshot must be published");
        let battery = sim_snap
            .assets
            .get(crate::ids::ASSET_BATTERY)
            .expect("battery asset must be present");
        assert!(
            battery.setpoint_kw > 0.0,
            "arbiter must charge the battery to absorb the unplanned PV surplus \
             (plan expected 0 kW net exchange), got setpoint {}",
            battery.setpoint_kw
        );
    }
}
