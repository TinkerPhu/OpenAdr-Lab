/// Dispatcher: translates FIRM plan slot allocations into per-asset setpoints.
///
/// Single responsibility: given the current plan, simulator assets, and capacity
/// constraints, produce a HashMap<asset_id, kW> that drives the simulator tick.
/// The plan is the sole authority.
use crate::controller::SimSnapshot;
use crate::entities::asset_params::PvCurtailmentSource;
use crate::entities::capacity::OadrCapacityState;
use crate::entities::plan::Plan;
use chrono::{DateTime, Utc};
use std::collections::HashMap;

/// Build a setpoints map for all known assets based on the active plan.
///
/// Single responsibility (narrowed — see `controller::arbiter` for the
/// reactive adjustment layer that used to run as this function's final step):
/// 1. Start with each asset's `default_setpoint_kw` from the snapshot.
/// 2. Find the slot covering `now` in the plan.
/// 3. Overwrite entries for assets that have an allocation in that slot.
/// 4. If `heater_setpoint_c` override is set and the plan has no heater allocation,
///    compute ON/OFF setpoint based on current temperature vs. target.
///
/// PV generation limiting is not handled here — see `resolve_pv_generation_limit_kw`,
/// applied directly to `PvInverter.generation_limit_kw` every tick, since
/// `PvInverter::step_inner` never reads the setpoints map (non-curtailable via setpoint).
///
/// Opportunistic EV charging and reactive battery correction have moved to
/// `controller::arbiter::reconcile`, called by the tick loop after this
/// function returns — see `openspec/changes/deviation-arbiter/`.
pub fn build_setpoints(
    plan: &Plan,
    sim: &SimSnapshot,
    heater_setpoint_c: Option<f64>,
    now: DateTime<Utc>,
) -> HashMap<String, f64> {
    // Start with defaults from snapshot
    let mut setpoints: HashMap<String, f64> = sim
        .assets
        .iter()
        .map(|(id, snap)| (id.clone(), snap.default_setpoint_kw))
        .collect();

    // Find the slot covering now
    let slot_allocs: Option<&Vec<crate::entities::plan::AssetAllocation>> = plan
        .slots
        .iter()
        .find(|s| s.start <= now && now < s.end)
        .map(|s| &s.allocations);

    let mut plan_allocated_heater = false;
    if let Some(allocs) = slot_allocs {
        for alloc in allocs {
            // Battery allocations have no associated packet
            if alloc.asset_id == crate::ids::ASSET_BATTERY {
                setpoints.insert(crate::ids::ASSET_BATTERY.to_string(), alloc.power_kw);
                continue;
            }
            if alloc.asset_id == crate::ids::ASSET_HEATER {
                plan_allocated_heater = true;
            }
            setpoints.insert(alloc.asset_id.clone(), alloc.power_kw);
        }
    }

    // Heater setpoint override: compute ON/OFF based on current temp vs. target.
    // Only applies when the plan has no heater allocation for the current slot.
    if let Some(target_c) = heater_setpoint_c {
        if !plan_allocated_heater {
            if let Some(snap) = sim.assets.get(crate::ids::ASSET_HEATER) {
                let temp_c = snap.val("temp_c").unwrap_or(20.0);
                let max_kw = snap.values.get("max_kw").copied().unwrap_or(0.0);
                let power_kw = if temp_c < target_c { max_kw } else { 0.0 };
                setpoints.insert(crate::ids::ASSET_HEATER.to_string(), power_kw);
            }
        }
    }

    setpoints
}

/// Whether the plan has an EV allocation in the slot covering `now` — the
/// arbiter's EV lever is only offered when this is `false` (opportunistic
/// regime; a plan-committed EV rate is never second-guessed).
pub fn plan_has_ev_allocation(plan: &Plan, now: DateTime<Utc>) -> bool {
    plan.slots
        .iter()
        .find(|s| s.start <= now && now < s.end)
        .is_some_and(|s| {
            s.allocations
                .iter()
                .any(|a| a.asset_id == crate::ids::ASSET_EV)
        })
}

/// Result of resolving the effective PV generation limit for the current tick.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ResolvedPvGenerationLimit {
    /// kW, negative = generation ceiling; `None` = uncurtailed.
    pub limit_kw: Option<f64>,
    /// Which source produced `limit_kw`.
    pub source: PvCurtailmentSource,
}

/// Resolve the effective PV generation limit applied to `PvInverter.generation_limit_kw` every
/// tick, as the more restrictive of:
/// - the live capacity state's `export_limit_kw` (VTN `EXPORT_CAPACITY_LIMIT` or sim-inject,
///   stored as a positive magnitude — this is the site-level grid export cap, used here as an
///   input, not the same quantity as the PV-level limit being resolved),
/// - the current plan slot's curtailment target (`pv_used_kw < pv_forecast_kw`),
/// - `arbiter_tighten_kw` (§5.4, `controller::arbiter`'s PV-curtailment backstop lever —
///   a positive kW magnitude, or `None` when the arbiter isn't offering it this tick), and
/// - `manual_limit_kw` (an operator/tester override via `SimInjectState.pv_generation_limit_kw`,
///   a positive kW magnitude, or `None` when not set).
///
/// Any source can only tighten the limit, never loosen it, so taking the smaller-magnitude
/// (numerically larger, since all are `<= 0` once converted) value is correct without needing
/// to know which source is active — but which one it *is* is also returned, tagged at this
/// exact moment, so curtailment can be recorded as planned/unplanned/arbiter-driven/manual
/// without ever reconstructing past plans. On an exact tie, the later-listed source in
/// `candidates` below wins (preserves the pre-existing "plan wins ties over capacity" rule —
/// see `openspec/changes/pv-curtailment-history/` — and extends it so arbiter wins ties over
/// both, and manual — the most deliberate/explicit source — wins ties over all three).
pub fn resolve_pv_generation_limit_kw(
    plan: Option<&Plan>,
    capacity: &OadrCapacityState,
    now: DateTime<Utc>,
    arbiter_tighten_kw: Option<f64>,
    manual_limit_kw: Option<f64>,
) -> ResolvedPvGenerationLimit {
    let capacity_limit = capacity.export_limit_kw.map(|v| -v.abs());
    let plan_limit = plan.and_then(|p| {
        p.slots
            .iter()
            .find(|s| s.start <= now && now < s.end)
            .and_then(|slot| {
                if slot.pv_used_kw + 1e-6 < slot.pv_forecast_kw {
                    Some(-slot.pv_used_kw)
                } else {
                    None
                }
            })
    });
    let arbiter_limit = arbiter_tighten_kw.map(|v| -v.abs());
    let manual_limit = manual_limit_kw.map(|v| -v.abs());

    let candidates = [
        (capacity_limit, PvCurtailmentSource::Capacity),
        (plan_limit, PvCurtailmentSource::Plan),
        (arbiter_limit, PvCurtailmentSource::Arbiter),
        (manual_limit, PvCurtailmentSource::Manual),
    ];
    // Tighter (numerically larger, since all values are <= 0) wins; `max_by`
    // returns the last of several equally-maximal elements, so on an exact
    // tie the later-listed source wins (see doc comment above).
    let winner = candidates
        .into_iter()
        .filter_map(|(limit, source)| limit.map(|v| (v, source)))
        .max_by(|(a, _), (b, _)| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

    match winner {
        Some((limit_kw, source)) => ResolvedPvGenerationLimit {
            limit_kw: Some(limit_kw),
            source,
        },
        None => ResolvedPvGenerationLimit {
            limit_kw: None,
            source: PvCurtailmentSource::None,
        },
    }
}

/// Predicts the heater's actual physical power this tick when its own thermostat
/// hysteresis forces a value regardless of the commanded setpoint — mirrors the same
/// "commanded ≠ actual" gap `live_pv_kw` already closes for PV. The emergency-heat
/// hysteresis (fires at `temp_min_c`, holds until `temp_min_c + 3°C`, see
/// `Heater::step_inner`) and the overheat safety cutoff both override whatever
/// setpoint the dispatcher/overlay computed, so any net-power accounting that trusts
/// the commanded heater setpoint under-counts import while the hysteresis is active
/// (found via the WP3.4 DISPATCH_SETPOINT E2E scenario: reported `grid.net_power_w`
/// was 3 kW — exactly `max_kw` — above the commanded target because the heater was
/// mid-hysteresis and drawing full power while its setpoint read 0).
/// Returns `None` when the heater is following its commanded setpoint normally, so
/// callers should use the setpoints-map value as before.
pub(crate) fn predict_heater_forced_kw(
    snap: &crate::controller::simulator_port::AssetSnapshot,
) -> Option<f64> {
    let max_kw = snap.val("max_kw")?;
    let temp_c = snap.val("temp_c")?;
    let temp_min_c = snap.val("temp_min_c")?;
    let temp_max_c = snap.val("temp_max_c")?;
    let temp_safety_max_c = snap.val("temp_safety_max_c").unwrap_or(temp_max_c);
    let curtail = snap.val("emergency_curtail").unwrap_or(0.0) > 0.5;
    let absorb = snap.val("emergency_absorb").unwrap_or(0.0) > 0.5;
    const EMERGENCY_HYSTERESIS_C: f64 = 3.0;
    let emergency_active = !curtail
        && (temp_c <= temp_min_c
            || (snap.power_kw >= max_kw && temp_c < temp_min_c + EMERGENCY_HYSTERESIS_C));
    let safety_ceiling_c = if absorb {
        temp_safety_max_c
    } else {
        temp_max_c
    };
    if temp_c >= safety_ceiling_c {
        Some(0.0)
    } else if emergency_active {
        Some(max_kw)
    } else {
        None
    }
}

/// Opportunistic surplus EV charging overlay.
///
/// Kept for the `deviation_arbiter_enabled == false` rollout-gate path only
/// (`tasks::sim_tick::helpers::build_tick_setpoints`) — when the arbiter is
/// enabled, `controller::arbiter::reconcile` owns this decision instead (its
/// `apply_ev_lever_opportunistic` is a from-scratch reimplementation, not a
/// call into this function, so this exact pre-arbiter behavior is preserved
/// byte-for-byte regardless of any future change to the arbiter's version).
/// See `openspec/changes/deviation-arbiter/` — "when false, the tick loop
/// SHALL behave exactly as before this change" is the reason this still
/// exists rather than being deleted outright.
///
/// When generation exceeds all other active loads, offer the surplus to the EV
/// (up to its max charge rate). All non-EV, non-battery assets are included in
/// the surplus calculation so the EV charge targets zero net grid power.
///
/// Does nothing when:
/// - `overlay_enabled` is false (user disabled or auto-paused by active EvSession)
/// - `plan_has_ev_allocation` is true (plan-level commitment takes priority)
/// - EV is unplugged
/// - EV SoC has reached its target
/// - Surplus is below the 100 W noise floor
///
/// `live_pv_kw`: this tick's PV output (`SimState::peek_pv_kw`), preferred
/// over `sim`'s snapshot for the PV term in `net_other_kw`. Without it, PV's
/// contribution falls back to `AssetSnapshot.power_kw`, which is last tick's
/// actual output.
pub fn apply_surplus_ev_overlay(
    setpoints: &mut HashMap<String, f64>,
    sim: &SimSnapshot,
    plan_has_ev_allocation: bool,
    overlay_enabled: bool,
    live_pv_kw: Option<f64>,
) {
    if plan_has_ev_allocation || !overlay_enabled {
        return;
    }
    let net_other_kw: f64 = sim
        .assets
        .iter()
        .filter(|(id, _)| {
            id.as_str() != crate::ids::ASSET_EV && id.as_str() != crate::ids::ASSET_BATTERY
        })
        .map(|(id, snap)| {
            if id.as_str() == crate::ids::ASSET_PV {
                if let Some(pv_kw) = live_pv_kw {
                    return pv_kw;
                }
            }
            if id.as_str() == crate::ids::ASSET_HEATER {
                if let Some(forced_kw) = predict_heater_forced_kw(snap) {
                    return forced_kw;
                }
            }
            let sp = setpoints.get(id).copied().unwrap_or(snap.power_kw);
            if sp.abs() > 1e20 {
                snap.power_kw
            } else {
                sp
            }
        })
        .sum();
    let battery_charge_kw = setpoints
        .get(crate::ids::ASSET_BATTERY)
        .copied()
        .unwrap_or(0.0)
        .max(0.0);
    let surplus_kw = (-net_other_kw - battery_charge_kw).max(0.0);
    if surplus_kw < 0.1 {
        return;
    }
    if let Some(snap) = sim.assets.get(crate::ids::ASSET_EV) {
        let plugged = snap.val("plugged").unwrap_or(0.0) > 0.5;
        let soc = snap.val("soc").unwrap_or(0.0);
        let soc_target = snap.val("soc_target").unwrap_or(1.0);
        if plugged && soc < soc_target {
            let max_charge_kw = snap.values.get("max_charge_kw").copied().unwrap_or(0.0);
            let min_charge_kw = snap.values.get("min_charge_kw").copied().unwrap_or(0.0);
            let charge_kw = surplus_kw.min(max_charge_kw);
            if charge_kw >= min_charge_kw {
                setpoints.insert(crate::ids::ASSET_EV.to_string(), charge_kw);
            }
        }
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::controller::{AssetSnapshot, GridSnapshot, SimSnapshot};
    use std::collections::HashMap as StdHashMap;

    fn battery_entry(soc: f64) -> (String, AssetSnapshot) {
        let cap_max_export_kw = if soc <= 0.1 { 0.0 } else { -5.0 };
        let cap_max_import_kw = if soc >= 1.0 { 0.0 } else { 5.0 };
        let available_discharge_kwh = Some((soc - 0.1).max(0.0) * 10.0);
        let available_charge_kwh = Some((1.0 - soc).max(0.0) * 10.0);
        let mut values = StdHashMap::new();
        values.insert("soc".into(), soc);
        values.insert("capacity_kwh".into(), 10.0);
        values.insert("max_charge_kw".into(), 5.0);
        values.insert("max_discharge_kw".into(), 5.0);
        values.insert("min_soc".into(), 0.1);
        (
            "battery".to_string(),
            AssetSnapshot {
                power_kw: 0.0,
                asset_type: "battery".to_string(),
                cap_max_import_kw,
                cap_max_export_kw,
                available_discharge_kwh,
                available_charge_kwh,
                default_setpoint_kw: 0.0,
                setpoint_kw: 0.0,
                values,
            },
        )
    }

    fn ev_entry(soc: f64, plugged: bool, soc_target: f64) -> (String, AssetSnapshot) {
        let max_ch = 7.4_f64;
        let bat_kwh = 60.0_f64;
        let (cap_max_import_kw, cap_max_export_kw, avail_dis, avail_ch) = if plugged {
            let import = if soc >= soc_target { 0.0 } else { max_ch };
            (
                import,
                0.0_f64,
                Some(soc * bat_kwh),
                Some((1.0 - soc) * bat_kwh),
            )
        } else {
            (0.0, 0.0, None, None)
        };
        let mut values = StdHashMap::new();
        values.insert("soc".into(), soc);
        values.insert("plugged".into(), if plugged { 1.0 } else { 0.0 });
        values.insert("max_charge_kw".into(), max_ch);
        values.insert("min_charge_kw".into(), 1.4);
        values.insert("soc_target".into(), soc_target);
        values.insert("battery_kwh".into(), bat_kwh);
        (
            "ev".to_string(),
            AssetSnapshot {
                power_kw: 0.0,
                asset_type: "ev".to_string(),
                cap_max_import_kw,
                cap_max_export_kw,
                available_discharge_kwh: avail_dis,
                available_charge_kwh: avail_ch,
                default_setpoint_kw: 0.0,
                setpoint_kw: 0.0,
                values,
            },
        )
    }

    fn pv_entry(last_power_kw: f64) -> (String, AssetSnapshot) {
        let mut values = StdHashMap::new();
        values.insert("irradiance".into(), 0.0);
        values.insert("rated_kw".into(), 10.0);
        values.insert("irradiance_offset".into(), 0.0);
        values.insert("pv_alpha".into(), 0.1);
        (
            "pv".to_string(),
            AssetSnapshot {
                power_kw: last_power_kw,
                asset_type: "pv".to_string(),
                cap_max_import_kw: last_power_kw,
                cap_max_export_kw: last_power_kw,
                available_discharge_kwh: None,
                available_charge_kwh: None,
                default_setpoint_kw: 0.0,
                setpoint_kw: 0.0,
                values,
            },
        )
    }

    fn base_entry(last_power_kw: f64) -> (String, AssetSnapshot) {
        let mut values = StdHashMap::new();
        values.insert("baseline_kw".into(), last_power_kw.max(0.0));
        (
            "base_load".to_string(),
            AssetSnapshot {
                power_kw: last_power_kw,
                asset_type: "base_load".to_string(),
                cap_max_import_kw: last_power_kw,
                cap_max_export_kw: last_power_kw,
                available_discharge_kwh: None,
                available_charge_kwh: None,
                default_setpoint_kw: last_power_kw.max(0.0),
                setpoint_kw: 0.0,
                values,
            },
        )
    }

    fn make_sim_snap(pairs: Vec<(String, AssetSnapshot)>) -> SimSnapshot {
        let assets = pairs.into_iter().collect();
        SimSnapshot {
            ts: chrono::Utc::now(),
            grid: GridSnapshot {
                net_power_w: 0.0,
                voltage_v: 230.0,
                import_kwh: 0.0,
                export_kwh: 0.0,
                import_limit_kw: f64::MAX,
                export_limit_kw: -f64::MAX,
            },
            assets,
        }
    }

    fn build_sim_snap(
        pv_kw: f64,
        base_kw: f64,
        ev_soc: f64,
        ev_plugged: bool,
        ev_target: f64,
    ) -> SimSnapshot {
        make_sim_snap(vec![
            pv_entry(pv_kw),
            base_entry(base_kw),
            ev_entry(ev_soc, ev_plugged, ev_target),
        ])
    }

    // ── surplus_ev_overlay tests ──────────────────────────────────────────────

    #[test]
    fn predict_heater_forced_kw_returns_max_kw_during_emergency_hysteresis() {
        // Regression (WP3.4 DISPATCH_SETPOINT E2E failure): temp is back inside the
        // comfort band (20 > temp_min_c 18) but the heater was drawing max_kw last
        // tick and hasn't yet climbed past temp_min_c + 3°C — hysteresis says it's
        // still forced on regardless of what setpoint the dispatcher commanded.
        let mut values = StdHashMap::new();
        values.insert("temp_c".into(), 20.0);
        values.insert("max_kw".into(), 3.0);
        values.insert("temp_min_c".into(), 18.0);
        values.insert("temp_max_c".into(), 23.0);
        values.insert("temp_safety_max_c".into(), 23.0);
        let snap = AssetSnapshot {
            power_kw: 3.0, // last tick's actual output == max_kw
            asset_type: "heater".to_string(),
            cap_max_import_kw: 3.0,
            cap_max_export_kw: 0.0,
            available_discharge_kwh: None,
            available_charge_kwh: None,
            default_setpoint_kw: 0.0,
            setpoint_kw: 0.0,
            values,
        };
        assert_eq!(
            predict_heater_forced_kw(&snap),
            Some(3.0),
            "hysteresis must force max_kw even though temp is inside the comfort band"
        );
    }

    #[test]
    fn predict_heater_forced_kw_returns_none_in_normal_band() {
        // Temp mid-band, last tick's output was below max_kw — no hysteresis or
        // safety override active, so the caller should trust the commanded setpoint.
        let mut values = StdHashMap::new();
        values.insert("temp_c".into(), 20.0);
        values.insert("max_kw".into(), 3.0);
        values.insert("temp_min_c".into(), 18.0);
        values.insert("temp_max_c".into(), 23.0);
        values.insert("temp_safety_max_c".into(), 23.0);
        let snap = AssetSnapshot {
            power_kw: 0.0,
            asset_type: "heater".to_string(),
            cap_max_import_kw: 3.0,
            cap_max_export_kw: 0.0,
            available_discharge_kwh: None,
            available_charge_kwh: None,
            default_setpoint_kw: 0.0,
            setpoint_kw: 0.0,
            values,
        };
        assert_eq!(predict_heater_forced_kw(&snap), None);
    }

    // ── surplus_ev_overlay: pinned for the deviation_arbiter_enabled=false
    //    rollout-gate path (see this function's doc comment) ─────────────────

    #[test]
    fn surplus_charges_ev_when_pv_exceeds_base() {
        let sim = build_sim_snap(-3.0, 1.0, 0.4, true, 0.8);
        let mut sp: HashMap<String, f64> = HashMap::new();
        apply_surplus_ev_overlay(&mut sp, &sim, false, true, None);
        let ev_sp = sp.get("ev").copied().unwrap_or(0.0);
        assert!((ev_sp - 2.0).abs() < 1e-6, "expected 2.0 kW, got {ev_sp}");
    }

    #[test]
    fn surplus_overlay_prefers_live_pv_kw_over_stale_snapshot() {
        let sim = build_sim_snap(-0.5, 1.0, 0.4, true, 0.8);
        let mut sp: HashMap<String, f64> = HashMap::new();
        apply_surplus_ev_overlay(&mut sp, &sim, false, true, Some(-5.0));
        let ev_sp = sp.get("ev").copied().unwrap_or(0.0);
        assert!((ev_sp - 4.0).abs() < 1e-6, "expected 4.0 kW, got {ev_sp}");
    }

    #[test]
    fn surplus_capped_at_ev_max_charge_kw() {
        let sim = build_sim_snap(-10.0, 0.0, 0.1, true, 0.8);
        let mut sp: HashMap<String, f64> = HashMap::new();
        apply_surplus_ev_overlay(&mut sp, &sim, false, true, None);
        let ev_sp = sp.get("ev").copied().unwrap_or(0.0);
        assert!(
            (ev_sp - 7.4).abs() < 1e-6,
            "expected cap at 7.4, got {ev_sp}"
        );
    }

    #[test]
    fn surplus_not_applied_when_plan_has_ev_allocation() {
        let sim = build_sim_snap(-3.0, 1.0, 0.4, true, 0.8);
        let mut sp: HashMap<String, f64> = HashMap::new();
        sp.insert("ev".to_string(), 5.0);
        apply_surplus_ev_overlay(&mut sp, &sim, true, true, None);
        let ev_sp = sp.get("ev").copied().unwrap_or(0.0);
        assert!(
            (ev_sp - 5.0).abs() < 1e-6,
            "plan allocation must not be overridden, got {ev_sp}"
        );
    }

    #[test]
    fn overlay_disabled_suppresses_ev_even_with_surplus() {
        let sim = build_sim_snap(-3.0, 1.0, 0.4, true, 0.8);
        let mut sp: HashMap<String, f64> = HashMap::new();
        apply_surplus_ev_overlay(&mut sp, &sim, false, false, None);
        assert!(
            !sp.contains_key("ev"),
            "overlay must not fire when disabled"
        );
    }

    // ── T012: build_setpoints & overlay edge-case tests ──────────────────────

    /// Build a minimal Plan with one slot covering `now` allocating `battery_kw` to battery.
    fn make_test_plan(battery_kw: f64, now: chrono::DateTime<Utc>) -> crate::entities::plan::Plan {
        use crate::entities::asset::PlanTrigger;
        use crate::entities::plan::{
            AssetAllocation, CostBreakdown, Plan, PlanSummary, PlanTimeSlot, PlanningHorizon,
        };
        use chrono::Duration;
        use uuid::Uuid;

        let slot = PlanTimeSlot {
            slot_index: 0,
            start: now - Duration::seconds(1),
            end: now + Duration::seconds(300),
            import_tariff_eur_kwh: 0.20,
            export_tariff_eur_kwh: 0.05,
            co2_g_kwh: 300.0,
            grid_effective_cost: 0.26,
            marginal_cost_import_eur_per_kwh: 0.20,
            marginal_cost_export_eur_per_kwh: 0.20,
            rate_estimated: false,
            import_cap_kw: 10.0,
            export_cap_kw: 5.0,
            baseline_kw: 0.5,
            pv_forecast_kw: 0.0,
            pv_used_kw: 0.0,
            surplus_available_kw: 0.0,
            allocations: vec![AssetAllocation {
                asset_id: "battery".to_string(),
                power_kw: battery_kw,
                surplus_power_kw: 0.0,
                grid_power_kw: battery_kw,
                marginal_value: 1.0,
                cost_eur: 0.0,
                co2_g: 0.0,
            }],
            net_import_kw: battery_kw,
            net_export_kw: 0.0,
            import_flexibility_kw: 0.0,
            export_flexibility_kw: 0.0,
            bat_charge_kw: 0.0,
            bat_discharge_kw: battery_kw.abs(),
            planned_kw_by_asset: HashMap::from([("battery".to_string(), battery_kw)]),
            planned_state_by_asset: HashMap::new(),
        };

        Plan {
            id: Uuid::new_v4(),
            created_at: now,
            trigger: PlanTrigger::Periodic,
            horizon: PlanningHorizon {
                start_time: now,
                end_time: now + Duration::seconds(300),
                step_size_s: 300,
                num_steps: 1,
                far_horizon: now + Duration::seconds(300),
                zones: vec![crate::entities::plan::PlanZone {
                    step_s: 300,
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
            penalty_rules_active: vec![],
            solver_ms: None,
            mip_gap_target: None,
        }
    }

    #[test]
    fn build_setpoints_follows_plan_battery_allocation() {
        let now = Utc::now();
        let sim = make_sim_snap(vec![battery_entry(0.5)]);
        let plan = make_test_plan(-3.0, now);
        let sp = build_setpoints(&plan, &sim, None, now);
        let bat = sp.get("battery").copied().unwrap_or(999.0);
        assert!(
            (bat - (-3.0)).abs() < 0.01,
            "battery setpoint should follow plan allocation -3.0 kW, got {bat}"
        );
    }

    #[test]
    fn build_setpoints_empty_assets_returns_empty_map() {
        let now = Utc::now();
        let sim = make_sim_snap(vec![]);
        // Plan with no slots → no allocations → setpoints come only from snapshot defaults
        let plan = {
            use crate::entities::asset::PlanTrigger;
            use crate::entities::plan::{CostBreakdown, Plan, PlanSummary, PlanningHorizon};
            use chrono::Duration;
            use uuid::Uuid;
            Plan {
                id: Uuid::new_v4(),
                created_at: now,
                trigger: PlanTrigger::Periodic,
                horizon: PlanningHorizon {
                    start_time: now,
                    end_time: now + Duration::seconds(300),
                    step_size_s: 300,
                    num_steps: 1,
                    far_horizon: now + Duration::seconds(300),
                    zones: vec![crate::entities::plan::PlanZone {
                        step_s: 300,
                        slots: 1,
                    }],
                },
                slots: vec![], // no slots → no allocations
                summary: PlanSummary::default(),
                envelopes: vec![],
                warnings: vec![],
                soc_trajectory_kwh: vec![],
                objective: crate::entities::PlannerObjective::MinCost,
                objective_eur: 0.0,
                friction_eur: 0.0,
                cost_breakdown: CostBreakdown::default(),
                solve_status: crate::entities::plan::SolveStatus::Optimal,
                penalty_rules_active: vec![],
                solver_ms: None,
                mip_gap_target: None,
            }
        };
        let sp = build_setpoints(&plan, &sim, None, now);
        assert!(
            sp.is_empty(),
            "empty snapshot + no plan slots → empty setpoints map"
        );
    }

    // ── resolve_pv_generation_limit_kw (pv-export-curtailment) ────────────

    /// Build a minimal Plan with one slot covering `now`, with the given
    /// PV forecast/used values and no other allocations.
    fn make_pv_plan(
        pv_forecast_kw: f64,
        pv_used_kw: f64,
        now: chrono::DateTime<Utc>,
    ) -> crate::entities::plan::Plan {
        use crate::entities::asset::PlanTrigger;
        use crate::entities::plan::{
            CostBreakdown, Plan, PlanSummary, PlanTimeSlot, PlanningHorizon,
        };
        use chrono::Duration;
        use uuid::Uuid;

        let slot = PlanTimeSlot {
            slot_index: 0,
            start: now - Duration::seconds(1),
            end: now + Duration::seconds(300),
            import_tariff_eur_kwh: 0.20,
            export_tariff_eur_kwh: 0.05,
            co2_g_kwh: 300.0,
            grid_effective_cost: 0.26,
            marginal_cost_import_eur_per_kwh: 0.20,
            marginal_cost_export_eur_per_kwh: 0.20,
            rate_estimated: false,
            import_cap_kw: 10.0,
            export_cap_kw: 5.0,
            baseline_kw: 0.5,
            pv_forecast_kw,
            pv_used_kw,
            surplus_available_kw: 0.0,
            allocations: vec![],
            net_import_kw: 0.0,
            net_export_kw: 0.0,
            import_flexibility_kw: 0.0,
            export_flexibility_kw: 0.0,
            bat_charge_kw: 0.0,
            bat_discharge_kw: 0.0,
            planned_kw_by_asset: HashMap::new(),
            planned_state_by_asset: HashMap::new(),
        };

        Plan {
            id: Uuid::new_v4(),
            created_at: now,
            trigger: PlanTrigger::Periodic,
            horizon: PlanningHorizon {
                start_time: now,
                end_time: now + Duration::seconds(300),
                step_size_s: 300,
                num_steps: 1,
                far_horizon: now + Duration::seconds(300),
                zones: vec![crate::entities::plan::PlanZone {
                    step_s: 300,
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
            penalty_rules_active: vec![],
            solver_ms: None,
            mip_gap_target: None,
        }
    }

    fn capacity_with_export_limit_kw(
        limit_kw: f64,
    ) -> crate::entities::capacity::OadrCapacityState {
        crate::entities::capacity::OadrCapacityState {
            export_limit_kw: Some(limit_kw),
            ..Default::default()
        }
    }

    #[test]
    fn resolve_pv_generation_limit_no_active_limit_is_none() {
        let now = Utc::now();
        let plan = make_pv_plan(5.0, 5.0, now); // no plan-side curtailment
        let capacity = crate::entities::capacity::OadrCapacityState::default();
        let resolved = resolve_pv_generation_limit_kw(Some(&plan), &capacity, now, None, None);
        assert_eq!(resolved.limit_kw, None);
        assert_eq!(resolved.source, PvCurtailmentSource::None);
    }

    #[test]
    fn resolve_pv_generation_limit_plan_curtailment_alone() {
        let now = Utc::now();
        let plan = make_pv_plan(5.0, 3.0, now); // plan curtails 2 kW
        let capacity = crate::entities::capacity::OadrCapacityState::default();
        let resolved = resolve_pv_generation_limit_kw(Some(&plan), &capacity, now, None, None);
        assert!(
            (resolved.limit_kw.unwrap() - (-3.0)).abs() < 1e-9,
            "plan-driven limit should be -pv_used_kw, got {resolved:?}"
        );
        assert_eq!(resolved.source, PvCurtailmentSource::Plan);
    }

    #[test]
    fn resolve_pv_generation_limit_capacity_alone() {
        let now = Utc::now();
        let plan = make_pv_plan(5.0, 5.0, now); // no plan-side curtailment
        let capacity = capacity_with_export_limit_kw(2.0);
        let resolved = resolve_pv_generation_limit_kw(Some(&plan), &capacity, now, None, None);
        assert!(
            (resolved.limit_kw.unwrap() - (-2.0)).abs() < 1e-9,
            "capacity-driven limit should be -export_limit_kw, got {resolved:?}"
        );
        assert_eq!(resolved.source, PvCurtailmentSource::Capacity);
    }

    #[test]
    fn resolve_pv_generation_limit_tighter_of_two_wins() {
        let now = Utc::now();
        // Plan wants to curtail to 3 kW; capacity separately caps at 2 kW (tighter).
        let plan = make_pv_plan(5.0, 3.0, now);
        let capacity = capacity_with_export_limit_kw(2.0);
        let resolved = resolve_pv_generation_limit_kw(Some(&plan), &capacity, now, None, None);
        assert!(
            (resolved.limit_kw.unwrap() - (-2.0)).abs() < 1e-9,
            "tighter (capacity) limit must win, got {resolved:?}"
        );
        assert_eq!(resolved.source, PvCurtailmentSource::Capacity);

        // Now capacity is looser (4 kW) than the plan's 3 kW target — plan wins.
        let capacity_loose = capacity_with_export_limit_kw(4.0);
        let resolved2 =
            resolve_pv_generation_limit_kw(Some(&plan), &capacity_loose, now, None, None);
        assert!(
            (resolved2.limit_kw.unwrap() - (-3.0)).abs() < 1e-9,
            "tighter (plan) limit must win, got {resolved2:?}"
        );
        assert_eq!(resolved2.source, PvCurtailmentSource::Plan);
    }

    #[test]
    fn resolve_pv_generation_limit_equal_tightness_is_tagged_plan() {
        let now = Utc::now();
        // Plan curtails to 3 kW; capacity independently also caps at 3 kW — a tie.
        let plan = make_pv_plan(5.0, 3.0, now);
        let capacity = capacity_with_export_limit_kw(3.0);
        let resolved = resolve_pv_generation_limit_kw(Some(&plan), &capacity, now, None, None);
        assert!(
            (resolved.limit_kw.unwrap() - (-3.0)).abs() < 1e-9,
            "got {resolved:?}"
        );
        assert_eq!(
            resolved.source,
            PvCurtailmentSource::Plan,
            "a tie must be tagged as planned, not unplanned"
        );
    }

    #[test]
    fn resolve_pv_generation_limit_no_plan_falls_back_to_capacity_only() {
        let now = Utc::now();
        let capacity = capacity_with_export_limit_kw(1.5);
        let resolved = resolve_pv_generation_limit_kw(None, &capacity, now, None, None);
        assert!(
            (resolved.limit_kw.unwrap() - (-1.5)).abs() < 1e-9,
            "with no plan, capacity limit alone applies, got {resolved:?}"
        );
        assert_eq!(resolved.source, PvCurtailmentSource::Capacity);
    }

    #[test]
    fn resolve_pv_generation_limit_manual_only() {
        let now = Utc::now();
        let plan = make_pv_plan(5.0, 5.0, now); // no plan-side curtailment
        let capacity = crate::entities::capacity::OadrCapacityState::default();
        let resolved = resolve_pv_generation_limit_kw(Some(&plan), &capacity, now, None, Some(2.0));
        assert!(
            (resolved.limit_kw.unwrap() - (-2.0)).abs() < 1e-9,
            "manual-only limit should be -pv_generation_limit_kw, got {resolved:?}"
        );
        assert_eq!(resolved.source, PvCurtailmentSource::Manual);
    }

    #[test]
    fn resolve_pv_generation_limit_manual_tighter_than_capacity_wins() {
        let now = Utc::now();
        let plan = make_pv_plan(5.0, 5.0, now);
        let capacity = capacity_with_export_limit_kw(5.0); // looser than manual
        let resolved = resolve_pv_generation_limit_kw(Some(&plan), &capacity, now, None, Some(2.0));
        assert!(
            (resolved.limit_kw.unwrap() - (-2.0)).abs() < 1e-9,
            "tighter (manual) limit must win, got {resolved:?}"
        );
        assert_eq!(resolved.source, PvCurtailmentSource::Manual);
    }

    #[test]
    fn resolve_pv_generation_limit_manual_looser_than_capacity_is_not_selected() {
        let now = Utc::now();
        let plan = make_pv_plan(5.0, 5.0, now);
        let capacity = capacity_with_export_limit_kw(1.0); // tighter than manual
        let resolved = resolve_pv_generation_limit_kw(Some(&plan), &capacity, now, None, Some(3.0));
        assert!(
            (resolved.limit_kw.unwrap() - (-1.0)).abs() < 1e-9,
            "tighter (capacity) limit must win over a looser manual override, got {resolved:?}"
        );
        assert_eq!(resolved.source, PvCurtailmentSource::Capacity);
    }

    #[test]
    fn resolve_pv_generation_limit_manual_wins_exact_tie() {
        let now = Utc::now();
        let plan = make_pv_plan(5.0, 5.0, now);
        // Capacity and manual independently resolve to the exact same limit.
        let capacity = capacity_with_export_limit_kw(2.0);
        let resolved = resolve_pv_generation_limit_kw(Some(&plan), &capacity, now, None, Some(2.0));
        assert!(
            (resolved.limit_kw.unwrap() - (-2.0)).abs() < 1e-9,
            "got {resolved:?}"
        );
        assert_eq!(
            resolved.source,
            PvCurtailmentSource::Manual,
            "manual is the most deliberate/explicit source and must win an exact tie"
        );
    }
}
