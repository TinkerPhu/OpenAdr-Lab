use crate::controller::SimSnapshot;
/// Stage 4/5 — Monitor: per-asset energy ledger update.
///
/// `record_tick` updates the cumulative per-asset energy ledger from the
/// current simulation snapshot. Packet attribution has been removed;
/// device sessions (EvSession, HeaterTarget) are managed directly.
///
/// BL-39: the same per-tick import cost this function already computes for
/// the asset ledger is also attributed to any `UserRequest` targeting that
/// asset (`req.asset_id`, `UserRequestStatus::Active` only) via
/// `req.accumulated_cost_eur` — one cost computation, two consumers, rather
/// than a second independent accounting path.
use crate::entities::asset_ledger::AssetLedgerEntry;
use crate::entities::tariff_snapshot::TariffSnapshot;
use crate::entities::user_request::{UserRequest, UserRequestStatus};
use chrono::{DateTime, Utc};

const NEAR_ZERO_KW: f64 = 1e-3;
use std::collections::HashMap;

const DEFAULT_IMPORT_PRICE: f64 = 0.20;
const DEFAULT_CO2_G_KWH: f64 = 300.0;

/// Update the per-asset cumulative energy ledger from the current sim snapshot,
/// and (BL-39) attribute each asset's import cost to any active `UserRequest`
/// targeting it.
#[allow(clippy::too_many_arguments)] // one scalar coefficient added for BL-17's PV embodied-carbon reporting
pub fn record_tick(
    ledger: &mut HashMap<String, AssetLedgerEntry>,
    requests: &mut [UserRequest],
    sim: &SimSnapshot,
    tariffs: &[TariffSnapshot],
    dt_s: f64,
    now: DateTime<Utc>,
    pv_co2_g_kwh: f64,
) {
    let dt_h = dt_s / 3600.0;

    // Find applicable tariff for this tick
    let applicable = tariffs
        .iter()
        .find(|r| r.interval_start <= now && now < r.interval_end);
    let import_tariff = applicable
        .and_then(|r| r.import_tariff_eur_kwh)
        .unwrap_or(DEFAULT_IMPORT_PRICE);
    let co2_rate = applicable
        .and_then(|r| r.co2_g_kwh)
        .unwrap_or(DEFAULT_CO2_G_KWH);

    for (asset_id, asset_snap) in &sim.assets {
        let kw = asset_snap.power_kw;
        if kw.abs() <= NEAR_ZERO_KW {
            continue;
        }
        let entry = ledger
            .entry(asset_id.clone())
            .or_insert_with(|| AssetLedgerEntry::new(asset_id, now));
        entry.energy_kwh += kw.abs() * dt_h;
        let import_cost_eur = if kw > 0.0 {
            kw * dt_h * import_tariff
        } else {
            0.0
        };
        if kw > 0.0 {
            entry.cost_eur += import_cost_eur;
            entry.co2_g += kw * dt_h * co2_rate;
        } else if asset_id == crate::ids::ASSET_PV {
            // BL-17: PV's own embodied/lifecycle carbon, reporting-only — distinct
            // from the grid-import CO2 term above, does not enter the planner.
            entry.co2_g += kw.abs() * dt_h * pv_co2_g_kwh;
        }
        entry.updated_at = Some(now);

        if import_cost_eur > 0.0 {
            for req in requests.iter_mut() {
                if req.status == UserRequestStatus::Active && req.asset_id == *asset_id {
                    req.accumulated_cost_eur += import_cost_eur;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::controller::{AssetSnapshot, GridSnapshot, SimSnapshot};
    use chrono::Utc;
    use std::collections::HashMap;

    fn make_sim(asset_id: &str, power_kw: f64) -> SimSnapshot {
        SimSnapshot {
            ts: Utc::now(),
            grid: GridSnapshot {
                net_power_w: 0.0,
                voltage_v: 230.0,
                import_kwh: 0.0,
                export_kwh: 0.0,
            },
            assets: HashMap::from([(
                asset_id.to_string(),
                AssetSnapshot {
                    power_kw,
                    asset_type: "ev".to_string(),
                    cap_max_import_kw: 0.0,
                    cap_max_export_kw: 0.0,
                    available_discharge_kwh: None,
                    available_charge_kwh: None,
                    default_setpoint_kw: power_kw,
                    setpoint_kw: power_kw,
                    values: HashMap::new(),
                },
            )]),
        }
    }

    #[test]
    fn ledger_skips_power_below_near_zero_kw() {
        let sub_threshold = NEAR_ZERO_KW * 0.5;
        let sim = make_sim("ev", sub_threshold);
        let mut ledger = HashMap::new();
        record_tick(&mut ledger, &mut [], &sim, &[], 1.0, Utc::now(), 0.0);
        assert!(
            ledger.is_empty(),
            "ledger must not accumulate sub-threshold power"
        );
    }

    #[test]
    fn ledger_accumulates_power_above_near_zero_kw() {
        let above_threshold = NEAR_ZERO_KW * 2.0;
        let sim = make_sim("ev", above_threshold);
        let mut ledger = HashMap::new();
        record_tick(&mut ledger, &mut [], &sim, &[], 1.0, Utc::now(), 0.0);
        let entry = ledger
            .get("ev")
            .expect("ledger must have an entry for above-threshold power");
        assert!(
            entry.energy_kwh > 0.0,
            "energy_kwh must be positive, got {}",
            entry.energy_kwh
        );
    }

    // ── T014: cost and CO₂ accumulation with an active tariff snapshot ────────

    #[test]
    fn ledger_accumulates_cost_and_co2_with_tariff() {
        use crate::entities::tariff_snapshot::TariffSnapshot;
        use chrono::Duration;

        let now = Utc::now();
        // Asset importing 5 kW for 1 hour → 5 kWh
        let sim = make_sim("battery", 5.0);
        let tariff = TariffSnapshot {
            interval_start: now - Duration::seconds(60),
            interval_end: now + Duration::seconds(3600),
            import_tariff_eur_kwh: Some(0.30),
            export_tariff_eur_kwh: Some(0.10),
            co2_g_kwh: Some(400.0),
        };
        let mut ledger = HashMap::new();
        record_tick(&mut ledger, &mut [], &sim, &[tariff], 3600.0, now, 0.0);

        let entry = ledger.get("battery").expect("battery ledger entry");
        // energy = 5 kW * 1 h = 5 kWh
        assert!(
            (entry.energy_kwh - 5.0).abs() < 1e-6,
            "energy_kwh: expected 5.0, got {}",
            entry.energy_kwh
        );
        // cost = 5 kW * 1 h * 0.30 €/kWh = 1.50 €
        assert!(
            (entry.cost_eur - 1.5).abs() < 1e-6,
            "cost_eur: expected 1.50, got {}",
            entry.cost_eur
        );
        // co2 = 5 kW * 1 h * 400 g/kWh = 2000 g
        assert!(
            (entry.co2_g - 2000.0).abs() < 1e-6,
            "co2_g: expected 2000.0, got {}",
            entry.co2_g
        );
    }

    // ── BL-39: per-session accumulated cost ─────────────────────────────────

    fn make_request(asset_id: &str, status: UserRequestStatus) -> UserRequest {
        let now = Utc::now();
        UserRequest {
            id: uuid::Uuid::new_v4(),
            asset_id: asset_id.to_string(),
            target_soc: None,
            target_energy_kwh: 0.0,
            desired_power_kw: 0.0,
            deadlines: vec![],
            mode: Default::default(),
            completion_policy: "STOP".to_string(),
            max_total_cost_eur: None,
            tier_count: 0,
            session_id: None,
            session_type: None,
            comfort_rates: vec![],
            status,
            estimated_cost_eur: 0.0,
            estimated_co2_g: 0.0,
            accumulated_cost_eur: 0.0,
            interruptible: false,
            tolerance_min: None,
            budget_eur: None,
            created_at: now,
            updated_at: now,
        }
    }

    #[test]
    fn active_request_accumulates_cost_matching_its_own_asset_over_n_ticks() {
        use crate::entities::tariff_snapshot::TariffSnapshot;
        use chrono::Duration;

        let now = Utc::now();
        let tariff = TariffSnapshot {
            interval_start: now - Duration::seconds(60),
            interval_end: now + Duration::seconds(3600),
            import_tariff_eur_kwh: Some(0.30),
            export_tariff_eur_kwh: Some(0.10),
            co2_g_kwh: Some(400.0),
        };
        let mut ledger = HashMap::new();
        let mut requests = vec![make_request("ev", UserRequestStatus::Active)];

        // 3 ticks of 900s (15 min) each, 3 kW import: Σ(power × Δt × tariff)
        for _ in 0..3 {
            let sim = make_sim("ev", 3.0);
            record_tick(
                &mut ledger,
                &mut requests,
                &sim,
                std::slice::from_ref(&tariff),
                900.0,
                now,
                0.0,
            );
        }

        // 3 × (3 kW × 0.25 h × 0.30 €/kWh) = 3 × 0.225 = 0.675 €
        assert!(
            (requests[0].accumulated_cost_eur - 0.675).abs() < 1e-9,
            "expected 0.675, got {}",
            requests[0].accumulated_cost_eur
        );
    }

    #[test]
    fn only_the_matching_asset_id_request_accumulates() {
        let sim = make_sim("ev", 3.0);
        let mut ledger = HashMap::new();
        let mut requests = vec![
            make_request("ev", UserRequestStatus::Active),
            make_request("heater", UserRequestStatus::Active),
        ];
        record_tick(
            &mut ledger,
            &mut requests,
            &sim,
            &[],
            3600.0,
            Utc::now(),
            0.0,
        );

        assert!(
            requests[0].accumulated_cost_eur > 0.0,
            "ev request must accumulate"
        );
        assert_eq!(
            requests[1].accumulated_cost_eur, 0.0,
            "heater request must not accumulate another asset's cost"
        );
    }

    #[test]
    fn non_active_request_does_not_accumulate() {
        let sim = make_sim("ev", 3.0);
        let mut ledger = HashMap::new();
        let mut requests = vec![make_request("ev", UserRequestStatus::Completed)];
        record_tick(
            &mut ledger,
            &mut requests,
            &sim,
            &[],
            3600.0,
            Utc::now(),
            0.0,
        );

        assert_eq!(
            requests[0].accumulated_cost_eur, 0.0,
            "a Completed session must not keep accumulating"
        );
    }

    #[test]
    fn exporting_asset_does_not_accumulate_cost_for_its_request() {
        // Export (negative power) is revenue, not a cost the budget bar tracks.
        let sim = make_sim("battery", -3.0);
        let mut ledger = HashMap::new();
        let mut requests = vec![make_request("battery", UserRequestStatus::Active)];
        record_tick(
            &mut ledger,
            &mut requests,
            &sim,
            &[],
            3600.0,
            Utc::now(),
            0.0,
        );

        assert_eq!(requests[0].accumulated_cost_eur, 0.0);
    }

    // ── BL-17: PV embodied-carbon reporting ──────────────────────────────────

    #[test]
    fn pv_generation_accumulates_embodied_co2_when_pv_co2_g_kwh_is_set() {
        let sim = make_sim(crate::ids::ASSET_PV, -5.0); // generation is negative power
        let mut ledger = HashMap::new();
        record_tick(&mut ledger, &mut [], &sim, &[], 3600.0, Utc::now(), 40.0);

        let entry = ledger.get(crate::ids::ASSET_PV).expect("pv ledger entry");
        // 5 kWh generated * 40 gCO2/kWh = 200 g
        assert!(
            (entry.co2_g - 200.0).abs() < 1e-6,
            "co2_g: expected 200.0, got {}",
            entry.co2_g
        );
    }

    #[test]
    fn pv_generation_leaves_co2_g_at_zero_when_pv_co2_g_kwh_is_unset() {
        let sim = make_sim(crate::ids::ASSET_PV, -5.0);
        let mut ledger = HashMap::new();
        record_tick(&mut ledger, &mut [], &sim, &[], 3600.0, Utc::now(), 0.0);

        let entry = ledger.get(crate::ids::ASSET_PV).expect("pv ledger entry");
        assert_eq!(
            entry.co2_g, 0.0,
            "no behavior change from before this feature when pv_co2_g_kwh is 0.0"
        );
    }

    #[test]
    fn non_pv_exporting_asset_does_not_accumulate_embodied_co2() {
        // The PV embodied-carbon term is keyed on asset_id == ASSET_PV specifically —
        // any other exporting asset (e.g. a discharging battery) must not pick it up.
        let sim = make_sim("battery", -5.0);
        let mut ledger = HashMap::new();
        record_tick(&mut ledger, &mut [], &sim, &[], 3600.0, Utc::now(), 40.0);

        let entry = ledger.get("battery").expect("battery ledger entry");
        assert_eq!(
            entry.co2_g, 0.0,
            "pv_co2_g_kwh must not apply to a non-PV exporting asset"
        );
    }
}
