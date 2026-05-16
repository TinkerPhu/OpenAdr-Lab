/// Multi-asset deviation absorber (Tier 1 real-time control).
///
/// Corrects grid power deviations in real-time by adjusting battery, EV, and heater setpoints
/// sequentially within per-asset flexibility bounds. Tracks uncovered (residual) deviation
/// for Tier 2 escalation to MILP replanning.
///
/// ## Architecture
///
/// **Design Philosophy**: Absorber is a lightweight, stateless correction mechanism that operates
/// outside the MILP planner. It applies transient corrections (deltas from MILP setpoints) without
/// triggering expensive plan replans. Only when absorption fails (residual persists) does Tier 2
/// escalate to MILP replanning.
///
/// **Per-Asset Settling State Machine**:
///
/// Each asset independently tracks settling via the `settling_ticks` counter and `active_overlay_kw` map:
///
/// ```
/// ┌──────────────────────────────────┐
/// │ Idle                             │  ← active_overlay_kw[asset] = 0.0
/// │ settling_ticks[asset] = 0        │  ← no correction applied
/// └────────────┬─────────────────────┘
///              │ [|deviation| > dead_band_kw]
///              │ absorption applies correction
///              ▼
/// ┌──────────────────────────────────┐
/// │ Correcting                       │  ← active_overlay_kw[asset] ≠ 0.0
/// │ overlay represents delta from    │  ← setpoint = plan_sp + overlay
/// │ MILP setpoint                    │
/// └────────────┬─────────────────────┘
///              │ [|deviation| ≤ dead_band_kw for dead_band_clearing_ticks ticks]
///              │ absorption begins settling
///              ▼
/// ┌──────────────────────────────────┐
/// │ Settling                         │  ← overlay ramps to 0.0 over 1 tick
/// │ settling_ticks[asset] increments │  ← clamps delta.clamp(-headroom, +headroom)
/// └────────────┬─────────────────────┘
///              │ [settling_ticks ≥ 1]
///              │ overlay reaches 0.0
///              ▼
/// ┌──────────────────────────────────┐
/// │ Idle                             │  ← return to clean MILP setpoint
/// └──────────────────────────────────┘
/// ```
///
/// **Why 1-Tick Ramp**: Settling in exactly 1 tick ensures the absorber quickly returns
/// to clean MILP setpoints (no stale overlays). This avoids coupling the absorber's internal
/// ramp timing to the MILP plan frequency, keeping the two systems decoupled.
///
/// **Residual Tracking**: The absorber returns `residual_kw` after attempting to absorb all
/// assets. Tier 2 accumulates this residual (not raw grid deviation) to detect sustained
/// absorption failures, triggering replans only when the absorber is truly exhausted.
///
/// The absorber does NOT persist asset changes; it returns the uncovered residual for
/// Tier 2 (DeviceDeviation escalation) decision-making.
use chrono::{DateTime, Utc};
use std::collections::HashMap;

use crate::controller::SimSnapshot;
use crate::entities::device_session::EvSession;
use crate::entities::plan::Plan;
use crate::entities::planner_params::{AbsorberAssetParams, AbsorberParams, PlannerObjective};
use crate::planner_events::{PlannerEvent, PlannerEventTx};
use tracing::{error, warn};

/// Runtime state for the multi-asset absorber.
///
/// Persists across ticks within a single VEN process. Reset on process restart.
#[derive(Debug, Clone)]
pub struct AbsorberState {
    /// Counter of consecutive ticks where residual > dead_band_kw (used for Tier 2 escalation).
    pub residual_ticks: u32,

    /// Per-asset: timestamp of last state change (used for linger enforcement).
    pub last_state_change_ts: HashMap<String, DateTime<Utc>>,

    /// Per-asset: consecutive ticks this asset's deviation has been inside the dead-band.
    /// Used as the wait-gate counter for `dead_band_clearing_ticks` chatter suppression.
    /// Resets to 0 when deviation returns above dead-band.
    /// When counter reaches the threshold, the overlay is zeroed on that same tick.
    pub settling_ticks: HashMap<String, u32>,

    /// Per-asset: current correction overlay (delta from MILP setpoint); 0.0 = no correction.
    pub active_overlay_kw: HashMap<String, f64>,

    /// True if any asset has active overlay (used for SSE state bookkeeping).
    pub correction_is_active: bool,

    /// Magnitude of last emitted SSE event (used to deduplicate events, FR-012).
    pub last_emitted_correction_kw: f64,
}

/// Apply multi-asset deviation absorption (Tier 1).
///
/// Corrects grid deviation by adjusting battery, EV, and heater setpoints sequentially
/// within flexibility bounds. Returns uncovered (residual) deviation for Tier 2 escalation.
///
/// # Arguments
///
/// * `state` - Mutable absorber state (updated in-place with corrections and linger tracking).
/// * `deviation_kw` - Signed grid deviation: `actual_net_kw - planned_net_kw`.
/// * `setpoints` - Mutable setpoint map: updated with correction overlays.
/// * `sim` - Current simulation state (for asset SoC, power bounds).
/// * `plan_snap` - Snapshot of current MILP plan (for flexibility envelope, may be None).
/// * `profile` - Profile config (for absorber config: enabled, dead_band_kw, assets list).
/// * `now` - Current UTC timestamp (for linger enforcement).
/// * `event_tx` - SSE broadcast channel (for CorrectionActive/CorrectionCleared events).
/// * `ev_session` - Active EV session (for departure guard; None = no guard).
///
/// # Returns
///
/// Signed residual deviation (what could NOT be absorbed). Equals `deviation_kw` when absorber
/// is disabled or deviation is within dead-band (passthrough for Tier 2 decision).
pub fn apply_deviation_absorption(
    state: &mut AbsorberState,
    deviation_kw: f64,
    setpoints: &mut HashMap<String, f64>,
    sim: &SimSnapshot,
    plan_snap: Option<&Plan>,
    params: &AbsorberParams,
    now: DateTime<Utc>,
    event_tx: &PlannerEventTx,
    ev_session: Option<&EvSession>,
) -> f64 {
    let was_active = state.correction_is_active;

    // Early exit: absorber disabled — passthrough full deviation to Tier 2.
    if !params.enabled {
        let asset_ids: Vec<_> = state.active_overlay_kw.keys().cloned().collect();
        for id in asset_ids {
            state.active_overlay_kw.insert(id, 0.0);
        }
        if was_active {
            let _ = event_tx.send(PlannerEvent::CorrectionCleared {
                ts: now,
                reason: "absorber_disabled".to_string(),
            });
            state.last_emitted_correction_kw = 0.0;
        }
        state.correction_is_active = false;
        return deviation_kw;
    }

    // Within dead-band: apply wait-gate before clearing overlays (FR-007).
    //
    // Each asset's `settling_ticks` counter increments per tick deviation stays
    // inside the dead-band.  Only when the counter reaches `dead_band_clearing_ticks`
    // is the overlay zeroed.  If deviation returns above the dead-band first, the
    // counter resets (see correction-application block below).
    //
    // With the default `dead_band_clearing_ticks = 1` an asset's counter reaches
    // threshold on the very first in-band tick — identical to the previous behaviour.
    if deviation_kw.abs() <= params.dead_band_kw {
        let threshold = params.dead_band_clearing_ticks as u32;

        // Only assets with a live overlay participate in the wait gate.
        let active_ids: Vec<String> = state
            .active_overlay_kw
            .iter()
            .filter(|(_, &v)| v.abs() > 0.01)
            .map(|(id, _)| id.clone())
            .collect();

        for id in &active_ids {
            let ticks = state.settling_ticks.entry(id.clone()).or_insert(0);
            *ticks += 1;
            if *ticks >= threshold {
                // Gate passed: zero overlay and reset counter.
                state.active_overlay_kw.insert(id.clone(), 0.0);
                state.settling_ticks.insert(id.clone(), 0);
            }
            // else: gate not yet passed — keep overlay, stay in Correcting.
        }

        // Fire CorrectionCleared and clear correction_is_active only when ALL overlays gone.
        let any_overlay_remaining = state.active_overlay_kw.values().any(|&v| v.abs() > 0.01);
        if !any_overlay_remaining {
            if was_active {
                let _ = event_tx.send(PlannerEvent::CorrectionCleared {
                    ts: now,
                    reason: "deviation_cleared".to_string(),
                });
                state.last_emitted_correction_kw = 0.0;
            }
            state.correction_is_active = false;
        }
        return deviation_kw;
    }

    let mut remaining_kw = deviation_kw;

    // Sequential asset iteration by priority (FR-002)
    let mut assets_by_priority = params.assets.clone();
    assets_by_priority.sort_by_key(|a| a.priority);

    for asset_cfg in &assets_by_priority {
        // Stop if residual is within dead-band
        if remaining_kw.abs() <= params.dead_band_kw {
            break;
        }

        // Check linger: skip asset if relay wear protection blocks it (FR-003)
        if !linger_ok(state, &asset_cfg.id, asset_cfg.min_state_linger_s, now) {
            continue;
        }

        // EV departure guard: skip reducing EV charge when departure is imminent (FR-008, US3)
        if let Some(guard_s) = asset_cfg.ev_departure_guard_s {
            if remaining_kw > 0.0 {
                // Positive deviation would reduce EV charge — check guard
                if let Some(snap) = sim.assets.get(&asset_cfg.id) {
                    let soc = snap.val("soc").unwrap_or(0.0);
                    let soc_target = snap.val("soc_target").unwrap_or(1.0);
                    if soc < soc_target {
                        // SoC below target: guard only applies when we'd curtail needed charging
                        match ev_session {
                            Some(session) => {
                                let secs_remaining =
                                    (session.departure_time - now).num_seconds();
                                // Guard triggers when departure is within guard window
                                if secs_remaining >= 0 && (secs_remaining as u64) < guard_s {
                                    continue; // Skip EV: departure imminent
                                }
                            }
                            // No active session → unknown departure → no guard (T048)
                            None => {}
                        }
                    }
                }
            }
        }

        // Compute headroom for this asset using current planned setpoint
        let current_sp = *setpoints.get(&asset_cfg.id).unwrap_or(&0.0);
        let headroom_kw =
            compute_asset_headroom(&asset_cfg.id, remaining_kw, sim, plan_snap, current_sp);
        if headroom_kw == 0.0 {
            continue;
        }

        // Apply correction (capped to headroom)
        let delta_kw = remaining_kw.clamp(-headroom_kw, headroom_kw);

        // Update setpoint if delta is significant
        if delta_kw.abs() >= 0.01 {
            // delta is reduction in import, so subtract from setpoint
            let new_sp = current_sp - delta_kw;
            setpoints.insert(asset_cfg.id.clone(), new_sp);

            // Track correction overlay and update linger
            state
                .active_overlay_kw
                .insert(asset_cfg.id.clone(), delta_kw);
            state.last_state_change_ts.insert(asset_cfg.id.clone(), now);
            // Reset in-band counter: deviation is above dead-band, gate restarts from zero.
            state.settling_ticks.insert(asset_cfg.id.clone(), 0);

            remaining_kw -= delta_kw;
        }
    }

    // Update correction active state
    let total_correction_kw: f64 = state.active_overlay_kw.values().sum();
    let is_active = total_correction_kw.abs() > 0.01;
    state.correction_is_active = is_active;

    // SSE event emission with 0.2 kW deduplication threshold (FR-012)
    if is_active && (total_correction_kw - state.last_emitted_correction_kw).abs() > 0.2 {
        let primary_asset = state
            .active_overlay_kw
            .iter()
            .max_by(|a, b| {
                a.1.abs()
                    .partial_cmp(&b.1.abs())
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .map(|(id, _)| id.clone())
            .unwrap_or_default();
        let planned_net_kw = plan_snap
            .and_then(|p| p.current_slot(now))
            .map(|s| s.net_import_kw - s.net_export_kw)
            .unwrap_or(0.0);
        let actual_net_kw = sim.grid.net_power_w / 1000.0;
        let objective = plan_snap
            .map(|p| p.objective)
            .unwrap_or(PlannerObjective::MinCost);
        let _ = event_tx.send(PlannerEvent::CorrectionActive {
            ts: now,
            asset_id: primary_asset,
            reason: "deviation_absorption".to_string(),
            planned_net_kw,
            actual_net_kw,
            deviation_kw,
            correction_kw: total_correction_kw,
            objective,
        });
        state.last_emitted_correction_kw = total_correction_kw;
    } else if !is_active && was_active {
        let _ = event_tx.send(PlannerEvent::CorrectionCleared {
            ts: now,
            reason: "deviation_absorbed".to_string(),
        });
        state.last_emitted_correction_kw = 0.0;
    }

    remaining_kw
}

/// Compute available correction headroom for a single asset (FR-011).
///
/// Bounds the delta that can be applied to a setpoint by asset physical limits
/// and plan flexibility envelope.
///
/// # Arguments
///
/// * `current_setpoint_kw` - The current planned setpoint (from dispatcher), used for EV curtailment.
///
/// # Returns
///
/// Maximum magnitude that can be applied as correction.
/// Positive = reduce import (discharge battery, curtail EV charge, reduce heater power).
/// Negative = increase import (charge battery, increase EV charge, increase heater power).
fn compute_asset_headroom(
    asset_id: &str,
    deviation_kw: f64,
    sim: &SimSnapshot,
    _plan_snap: Option<&Plan>,
    current_setpoint_kw: f64,
) -> f64 {
    let snap = match sim.assets.get(asset_id) {
        Some(s) => s,
        None => {
            warn!("absorber: asset {} not found in sim state", asset_id);
            return 0.0;
        }
    };

    match snap.asset_type.as_str() {
        "battery" => {
            if deviation_kw > 0.0 {
                let soc = snap.val("soc").unwrap_or(0.0);
                let min_soc = snap.val("min_soc").unwrap_or(0.0);
                let capacity_kwh = snap.val("capacity_kwh").unwrap_or(0.0);
                let max_discharge_kw = snap.val("max_discharge_kw").unwrap_or(0.0);
                let headroom = (soc - min_soc) * capacity_kwh;
                headroom.max(0.0).min(max_discharge_kw)
            } else {
                let soc = snap.val("soc").unwrap_or(0.0);
                let capacity_kwh = snap.val("capacity_kwh").unwrap_or(0.0);
                let max_charge_kw = snap.val("max_charge_kw").unwrap_or(0.0);
                let headroom = (1.0 - soc) * capacity_kwh;
                headroom.max(0.0).min(max_charge_kw)
            }
        }
        "ev" => {
            if deviation_kw > 0.0 {
                // Curtail EV charging (load reduction, not V2G discharge).
                // Headroom = current charge setpoint (how much rate we can reduce, down to 0).
                current_setpoint_kw.max(0.0)
            } else {
                // Increase EV charging to absorb surplus.
                let soc = snap.val("soc").unwrap_or(0.0);
                let soc_target = snap.val("soc_target").unwrap_or(1.0);
                let battery_kwh = snap.val("battery_kwh").unwrap_or(0.0);
                let max_charge_kw = snap.val("max_charge_kw").unwrap_or(0.0);
                let headroom = (soc_target - soc) * battery_kwh;
                headroom.max(0.0).min(max_charge_kw)
            }
        }
        "heater" => {
            // Heater has discrete power levels (0, mid, full)
            let current_power = snap.setpoint_kw;
            let mid_kw = snap.val("mid_kw").unwrap_or(0.0);
            let max_kw = snap.val("max_kw").unwrap_or(0.0);

            if deviation_kw > 0.0 {
                // Need to reduce power: go from current level toward 0
                if current_power > mid_kw {
                    (current_power - mid_kw).max(0.0)
                } else if current_power > 0.0 {
                    current_power
                } else {
                    0.0
                }
            } else {
                // Need to increase power: go from current level toward full
                if current_power < mid_kw {
                    mid_kw - current_power
                } else if current_power < max_kw {
                    max_kw - current_power
                } else {
                    0.0
                }
            }
        }
        _ => 0.0, // PV, BaseLoad are not controllable
    }
}

/// Check if linger enforcement allows state change for this asset (FR-003).
///
/// Returns true if enough time has passed since last state change, or if linger is disabled (0).
pub(crate) fn linger_ok(
    state: &AbsorberState,
    asset_id: &str,
    min_linger_s: u64,
    now: DateTime<Utc>,
) -> bool {
    if min_linger_s == 0 {
        return true;
    }
    match state.last_state_change_ts.get(asset_id) {
        None => true, // First change: always allowed
        Some(ts) => (now - ts).num_seconds() as u64 >= min_linger_s,
    }
}

/// Validate absorber configuration at startup (FR-013).
///
/// Checks:
/// 1. Asset ID Matching: all `AbsorberAssetConfig.id` must exist in `SimState.assets`
/// 2. Priority Uniqueness: logs WARN if duplicates detected
/// 3. Linger Bounds: logs WARN if min_state_linger_s > 300s
pub fn validate_startup(params: &AbsorberParams, sim: &SimSnapshot) -> anyhow::Result<()> {
    if !params.enabled {
        return Ok(());
    }

    let sim_asset_ids: std::collections::HashSet<&str> =
        sim.assets.keys().map(|s| s.as_str()).collect();

    for asset_cfg in &params.assets {
        if !sim_asset_ids.contains(asset_cfg.id.as_str()) {
            error!(
                absorber_asset_id = &asset_cfg.id,
                "absorber asset ID not found in SimState.assets — check profile"
            );
            anyhow::bail!(
                "absorber asset ID '{}' does not match any asset in SimState.assets",
                asset_cfg.id
            );
        }
    }

    let mut priorities = std::collections::HashSet::new();
    for asset_cfg in &params.assets {
        if !priorities.insert(asset_cfg.priority) {
            warn!(
                priority = asset_cfg.priority,
                "absorber has duplicate priority — may indicate config error"
            );
        }
    }

    for asset_cfg in &params.assets {
        if asset_cfg.min_state_linger_s > 300 {
            warn!(
                asset_id = &asset_cfg.id,
                linger_s = asset_cfg.min_state_linger_s,
                "absorber asset linger time exceeds 300s — likely configuration error"
            );
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Test helpers ──────────────────────────────────────────────────────────

    fn make_test_profile() -> AbsorberParams {
        AbsorberParams {
            enabled: true,
            dead_band_kw: 0.1,
            dead_band_clearing_ticks: 1,
            deviation_trigger_ticks: 30,
            assets: vec![
                AbsorberAssetParams {
                    id: "battery".to_string(),
                    priority: 0,
                    min_state_linger_s: 0,
                    ev_departure_guard_s: None,
                },
                AbsorberAssetParams {
                    id: "ev".to_string(),
                    priority: 1,
                    min_state_linger_s: 0,
                    ev_departure_guard_s: Some(1800),
                },
            ],
        }
    }

    fn make_test_profile_battery_linger() -> AbsorberParams {
        AbsorberParams {
            enabled: true,
            dead_band_kw: 0.1,
            dead_band_clearing_ticks: 1,
            deviation_trigger_ticks: 30,
            assets: vec![AbsorberAssetParams {
                id: "battery".to_string(),
                priority: 0,
                min_state_linger_s: 30,
                ev_departure_guard_s: None,
            }],
        }
    }

    fn make_test_snap() -> SimSnapshot {
        use crate::controller::{AssetSnapshot, GridSnapshot};
        use std::collections::HashMap;

        // Battery: soc=0.50, cap=10, max_ch/dis=5, min_soc=0.10
        // cap_max_export_kw = -5.0, cap_max_import_kw = 5.0
        // available_discharge = (0.50-0.10)*10 = 4.0, available_charge = (1.0-0.50)*10 = 5.0
        let mut bat_values = HashMap::new();
        bat_values.insert("soc".into(), 0.50_f64);
        bat_values.insert("capacity_kwh".into(), 10.0);
        bat_values.insert("max_charge_kw".into(), 5.0);
        bat_values.insert("max_discharge_kw".into(), 5.0);
        bat_values.insert("min_soc".into(), 0.10);

        // EV: soc=0.30, plugged=true, max_ch=7.4, battery=60, soc_target=0.80
        // cap_max_import_kw = 7.4, cap_max_export_kw = 0.0 (no V2G)
        // available_discharge = 0.30*60 = 18.0, available_charge = (1.0-0.30)*60 = 42.0
        let mut ev_values = HashMap::new();
        ev_values.insert("soc".into(), 0.30_f64);
        ev_values.insert("plugged".into(), 1.0);
        ev_values.insert("max_charge_kw".into(), 7.4);
        ev_values.insert("soc_target".into(), 0.80);
        ev_values.insert("battery_kwh".into(), 60.0);

        let mut assets = HashMap::new();
        assets.insert(
            "battery".to_string(),
            AssetSnapshot {
                power_kw: 0.0,
                asset_type: "battery".to_string(),
                cap_max_import_kw: 5.0,
                cap_max_export_kw: -5.0,
                available_discharge_kwh: Some(4.0),
                available_charge_kwh: Some(5.0),
                default_setpoint_kw: 0.0,
                setpoint_kw: 0.0,
                values: bat_values,
            },
        );
        assets.insert(
            "ev".to_string(),
            AssetSnapshot {
                power_kw: 0.0,
                asset_type: "ev".to_string(),
                cap_max_import_kw: 7.4,
                cap_max_export_kw: 0.0,
                available_discharge_kwh: Some(18.0),
                available_charge_kwh: Some(42.0),
                default_setpoint_kw: 0.0,
                setpoint_kw: 0.0,
                values: ev_values,
            },
        );

        SimSnapshot {
            ts: chrono::Utc::now(),
            grid: GridSnapshot {
                net_power_w: 0.0,
                voltage_v: 230.0,
                import_kwh: 0.0,
                export_kwh: 0.0,
            },
            assets,
        }
    }

    /// Battery at min_soc (no discharge headroom), EV charging at 7.4 kW.
    fn make_test_sim_battery_floor_ev_charging() -> (SimSnapshot, HashMap<String, f64>) {
        let mut snap = make_test_snap();
        // Battery at min_soc: cap_max_export_kw=0.0, available_discharge=0.0
        {
            let bat = snap.assets.get_mut("battery").unwrap();
            bat.values.insert("soc".into(), 0.10);
            bat.cap_max_export_kw = 0.0;
            bat.available_discharge_kwh = Some(0.0);
            bat.available_charge_kwh = Some(9.0); // (1.0-0.10)*10
        }
        // EV charging at max rate
        {
            let ev = snap.assets.get_mut("ev").unwrap();
            ev.setpoint_kw = 7.4;
        }
        let setpoints = HashMap::from([("battery".to_string(), 0.0), ("ev".to_string(), 7.4)]);
        (snap, setpoints)
    }

    /// Battery at min_soc, EV idle (setpoint 0) — both have 0 positive-deviation headroom.
    fn make_test_sim_all_exhausted_positive() -> (SimSnapshot, HashMap<String, f64>) {
        let mut snap = make_test_snap();
        // Battery at min_soc
        {
            let bat = snap.assets.get_mut("battery").unwrap();
            bat.values.insert("soc".into(), 0.10);
            bat.cap_max_export_kw = 0.0;
            bat.available_discharge_kwh = Some(0.0);
            bat.available_charge_kwh = Some(9.0);
        }
        // EV idle → 0 curtailment headroom
        {
            let ev = snap.assets.get_mut("ev").unwrap();
            ev.setpoint_kw = 0.0;
        }
        let setpoints = HashMap::from([("battery".to_string(), 0.0), ("ev".to_string(), 0.0)]);
        (snap, setpoints)
    }

    /// Battery fully charged (no charge headroom), EV idle — for negative deviation tests.
    fn make_test_sim_battery_full_ev_idle() -> (SimSnapshot, HashMap<String, f64>) {
        let mut snap = make_test_snap();
        // Battery full: cap_max_import_kw=0.0, available_charge=0.0
        {
            let bat = snap.assets.get_mut("battery").unwrap();
            bat.values.insert("soc".into(), 1.0);
            bat.cap_max_import_kw = 0.0;
            bat.available_discharge_kwh = Some(9.0); // (1.0-0.10)*10
            bat.available_charge_kwh = Some(0.0);
        }
        // EV idle
        {
            let ev = snap.assets.get_mut("ev").unwrap();
            ev.setpoint_kw = 0.0;
        }
        let setpoints = HashMap::from([("battery".to_string(), 0.0), ("ev".to_string(), 0.0)]);
        (snap, setpoints)
    }

    fn make_test_event_tx() -> PlannerEventTx {
        let (tx, _rx) = tokio::sync::broadcast::channel(16);
        std::sync::Arc::new(tx)
    }

    fn make_fresh_state() -> AbsorberState {
        AbsorberState {
            residual_ticks: 0,
            last_state_change_ts: HashMap::new(),
            settling_ticks: HashMap::new(),
            active_overlay_kw: HashMap::new(),
            correction_is_active: false,
            last_emitted_correction_kw: 0.0,
        }
    }

    fn make_ev_session(departure_in_s: i64) -> crate::entities::device_session::EvSession {
        crate::entities::device_session::EvSession {
            id: uuid::Uuid::new_v4(),
            target_soc: 0.80,
            departure_time: Utc::now() + chrono::Duration::seconds(departure_in_s),
            soft_deadline: false,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    // ── T013: zero deviation and empty-assets edge cases ─────────────────────

    #[test]
    fn absorber_zero_deviation_has_zero_residual() {
        let mut state = make_fresh_state();
        let profile = make_test_profile();
        let sim = make_test_snap();
        let event_tx = make_test_event_tx();
        let mut setpoints =
            HashMap::from([("battery".to_string(), 0.0), ("ev".to_string(), 0.0)]);
        let now = Utc::now();

        // 0.0 kW is within dead-band (0.1 kW) → no correction applied
        let residual = apply_deviation_absorption(
            &mut state,
            0.0,
            &mut setpoints,
            &sim,
            None,
            &profile,
            now,
            &event_tx,
            None,
        );

        assert_eq!(residual, 0.0, "zero deviation → zero residual; got {residual}");
        assert!(!state.correction_is_active, "no correction active for zero deviation");
        assert_eq!(setpoints["battery"], 0.0, "battery setpoint unchanged");
        assert_eq!(setpoints["ev"], 0.0, "ev setpoint unchanged");
    }

    #[test]
    fn absorber_empty_snapshot_does_not_panic() {
        use crate::controller::{GridSnapshot, SimSnapshot};
        let mut state = make_fresh_state();
        let profile = make_test_profile(); // has battery + ev in absorber config
        let sim = SimSnapshot {
            ts: Utc::now(),
            grid: GridSnapshot {
                net_power_w: 0.0,
                voltage_v: 230.0,
                import_kwh: 0.0,
                export_kwh: 0.0,
            },
            assets: HashMap::new(), // no assets
        };
        let event_tx = make_test_event_tx();
        let mut setpoints = HashMap::new();
        let now = Utc::now();

        // All asset headroom = 0 (not found in empty sim) → residual == full deviation
        let residual = apply_deviation_absorption(
            &mut state,
            3.0,
            &mut setpoints,
            &sim,
            None,
            &profile,
            now,
            &event_tx,
            None,
        );

        assert!(
            (residual - 3.0).abs() < 1e-9,
            "full deviation returned as residual when assets empty; got {residual}"
        );
        assert!(!state.correction_is_active, "no correction active with empty assets");
    }

    // ── User Story 1: Transient Deviation Absorption ──────────────────────────

    #[test]
    fn absorber_battery_absorbs_positive_deviation_within_capacity() {
        let mut state = make_fresh_state();
        let profile = make_test_profile();
        let sim = make_test_snap();
        let event_tx = make_test_event_tx();
        let mut setpoints = HashMap::from([("battery".to_string(), 0.0), ("ev".to_string(), 0.0)]);
        let now = Utc::now();

        let residual = apply_deviation_absorption(
            &mut state,
            2.0,
            &mut setpoints,
            &sim,
            None,
            &profile,
            now,
            &event_tx,
            None,
        );

        assert!(
            residual.abs() < 0.01,
            "residual should be ~0, got {}",
            residual
        );
        assert!(
            setpoints["battery"] < -0.5,
            "battery setpoint should be negative (discharge), got {}",
            setpoints["battery"]
        );
        assert!(state.correction_is_active);
    }

    #[test]
    fn absorber_battery_absorbs_negative_deviation_within_capacity() {
        let mut state = make_fresh_state();
        let profile = make_test_profile();
        let sim = make_test_snap();
        let event_tx = make_test_event_tx();
        let mut setpoints = HashMap::from([("battery".to_string(), 0.0), ("ev".to_string(), 0.0)]);
        let now = Utc::now();

        let residual = apply_deviation_absorption(
            &mut state,
            -2.0,
            &mut setpoints,
            &sim,
            None,
            &profile,
            now,
            &event_tx,
            None,
        );

        assert!(
            residual.abs() < 0.01,
            "residual should be ~0, got {}",
            residual
        );
        assert!(
            setpoints["battery"] > 0.5,
            "battery setpoint should be positive (charge), got {}",
            setpoints["battery"]
        );
    }

    // T021
    #[test]
    fn absorber_ev_absorbs_residual_when_battery_exhausted() {
        let mut state = make_fresh_state();
        let profile = make_test_profile();
        let (sim, mut setpoints) = make_test_sim_battery_floor_ev_charging();
        let event_tx = make_test_event_tx();
        let now = Utc::now();

        // Battery at min_soc (0 discharge headroom); EV charging at 7.4 kW
        let residual = apply_deviation_absorption(
            &mut state,
            4.0,
            &mut setpoints,
            &sim,
            None,
            &profile,
            now,
            &event_tx,
            None,
        );

        assert!(
            residual.abs() < 0.1,
            "EV should absorb the 4 kW deviation, got residual={}",
            residual
        );
        assert!(
            setpoints["ev"] < 7.4 - 0.5,
            "EV setpoint should be reduced, got {}",
            setpoints["ev"]
        );
    }

    #[test]
    fn absorber_dead_band_prevents_chatter() {
        let mut state = make_fresh_state();
        let profile = make_test_profile();
        let sim = make_test_snap();
        let event_tx = make_test_event_tx();
        let mut setpoints = HashMap::from([("battery".to_string(), 0.0), ("ev".to_string(), 0.0)]);
        let now = Utc::now();

        // 0.05 kW is within the 0.1 kW dead-band
        let residual = apply_deviation_absorption(
            &mut state,
            0.05,
            &mut setpoints,
            &sim,
            None,
            &profile,
            now,
            &event_tx,
            None,
        );

        // Within dead-band: no correction applied, full deviation returned
        assert!(
            residual.abs() > 0.04,
            "residual should include full deviation"
        );
        assert_eq!(setpoints["battery"], 0.0, "battery setpoint unchanged");
        assert!(!state.correction_is_active);
    }

    // T023
    #[test]
    fn absorber_settling_ramps_to_zero() {
        let mut state = make_fresh_state();
        let profile = make_test_profile();
        let sim = make_test_snap();
        let event_tx = make_test_event_tx();
        let mut setpoints = HashMap::from([("battery".to_string(), 0.0), ("ev".to_string(), 0.0)]);
        let now = Utc::now();

        // First tick: deviation → absorber applies correction
        apply_deviation_absorption(
            &mut state,
            2.0,
            &mut setpoints,
            &sim,
            None,
            &profile,
            now,
            &event_tx,
            None,
        );
        assert!(
            state.correction_is_active,
            "correction should be active after absorbing"
        );
        assert!(
            state.active_overlay_kw.values().any(|&v| v.abs() > 0.01),
            "overlay should be non-zero after correction"
        );

        // Second tick: deviation clears (within dead-band) → overlays settle to zero
        apply_deviation_absorption(
            &mut state,
            0.05,
            &mut setpoints,
            &sim,
            None,
            &profile,
            now,
            &event_tx,
            None,
        );
        assert!(
            !state.correction_is_active,
            "correction should clear after deviation settles"
        );
        assert!(
            state.active_overlay_kw.values().all(|&v| v.abs() < 0.01),
            "all overlays should be zeroed after settling"
        );
    }

    // T024
    #[test]
    fn absorber_residual_returned_when_all_exhausted() {
        let mut state = make_fresh_state();
        let profile = make_test_profile();
        let (sim, mut setpoints) = make_test_sim_all_exhausted_positive();
        let event_tx = make_test_event_tx();
        let now = Utc::now();

        // Battery at floor (0 discharge), EV idle (0 curtailment) → no headroom
        let residual = apply_deviation_absorption(
            &mut state,
            6.0,
            &mut setpoints,
            &sim,
            None,
            &profile,
            now,
            &event_tx,
            None,
        );

        assert!(
            residual.abs() > 5.9,
            "full deviation should be returned as residual, got {}",
            residual
        );
    }

    // ── User Story 2: Relay Wear Protection ──────────────────────────────────

    #[test]
    fn linger_ok_returns_false_before_min_time() {
        let now = Utc::now();
        let past = now - chrono::Duration::seconds(20);
        let mut state = make_fresh_state();
        state
            .last_state_change_ts
            .insert("asset1".to_string(), past);

        // 20 s elapsed, min_linger = 30 s → NOT allowed
        assert!(!linger_ok(&state, "asset1", 30, now));
    }

    #[test]
    fn linger_ok_returns_true_after_min_time() {
        let now = Utc::now();
        let past = now - chrono::Duration::seconds(40);
        let mut state = make_fresh_state();
        state
            .last_state_change_ts
            .insert("asset1".to_string(), past);

        // 40 s elapsed, min_linger = 30 s → allowed
        assert!(linger_ok(&state, "asset1", 30, now));
    }

    // T041
    #[test]
    fn linger_ok_returns_true_on_first_change() {
        let state = make_fresh_state(); // no prior timestamps
        let now = Utc::now();

        // No prior change → always allowed
        assert!(linger_ok(&state, "asset1", 30, now));
    }

    // T042
    #[test]
    fn absorber_asset_skipped_when_linger_active() {
        let now = Utc::now();
        let recent_change = now - chrono::Duration::seconds(10); // changed 10 s ago
        let mut state = make_fresh_state();
        state
            .last_state_change_ts
            .insert("battery".to_string(), recent_change);

        let params = make_test_profile_battery_linger(); // battery has 30 s linger
        let sim = make_test_snap(); // battery at SoC 0.50 — has headroom
        let event_tx = make_test_event_tx();
        let mut setpoints = HashMap::from([("battery".to_string(), 0.0)]);

        // Battery linger blocks (10 s < 30 s), profile has no EV asset → full residual
        let residual = apply_deviation_absorption(
            &mut state,
            2.0,
            &mut setpoints,
            &sim,
            None,
            &params,
            now,
            &event_tx,
            None,
        );

        assert!(
            residual.abs() > 1.9,
            "battery linger should block → full residual, got {}",
            residual
        );
        assert_eq!(
            setpoints["battery"], 0.0,
            "battery setpoint unchanged (linger blocked)"
        );
    }

    // ── User Story 3: EV Departure Guard ─────────────────────────────────────

    // T049
    #[test]
    fn absorber_ev_skipped_when_departure_guard_active() {
        let mut state = make_fresh_state();
        let profile = make_test_profile(); // EV has guard_s = 1800
        let (sim, mut setpoints) = make_test_sim_battery_floor_ev_charging();
        let event_tx = make_test_event_tx();
        // Departure in 20 min = 1200 s < 1800 s guard → guard is active
        let session = make_ev_session(1200);
        let now = Utc::now();

        let residual = apply_deviation_absorption(
            &mut state,
            4.0,
            &mut setpoints,
            &sim,
            None,
            &profile,
            now,
            &event_tx,
            Some(&session),
        );

        // Battery 0 headroom, EV guard active → full residual
        assert!(
            residual.abs() > 3.9,
            "EV guard should block → full residual, got {}",
            residual
        );
        assert!(
            (setpoints["ev"] - 7.4).abs() < 0.01,
            "EV setpoint unchanged (guarded), got {}",
            setpoints["ev"]
        );
    }

    // T050
    #[test]
    fn absorber_ev_allowed_when_departure_far_away() {
        let mut state = make_fresh_state();
        let profile = make_test_profile(); // EV has guard_s = 1800
        let (sim, mut setpoints) = make_test_sim_battery_floor_ev_charging();
        let event_tx = make_test_event_tx();
        // Departure in 40 min = 2400 s > 1800 s guard → guard is NOT active
        let session = make_ev_session(2400);
        let now = Utc::now();

        let residual = apply_deviation_absorption(
            &mut state,
            4.0,
            &mut setpoints,
            &sim,
            None,
            &profile,
            now,
            &event_tx,
            Some(&session),
        );

        // EV should absorb (guard not active)
        assert!(
            residual.abs() < 0.1,
            "EV should absorb when departure is far away, got residual={}",
            residual
        );
        assert!(
            setpoints["ev"] < 7.4 - 0.5,
            "EV setpoint should be reduced, got {}",
            setpoints["ev"]
        );
    }

    // T051
    #[test]
    fn absorber_ev_allowed_to_absorb_surplus_near_departure() {
        let mut state = make_fresh_state();
        let profile = make_test_profile(); // EV has guard_s = 1800
                                           // Battery full (no charge headroom), EV idle
        let (sim, mut setpoints) = make_test_sim_battery_full_ev_idle();
        let event_tx = make_test_event_tx();
        // Departure in 20 min < 30 min guard — but NEGATIVE deviation should still be allowed
        let session = make_ev_session(1200);
        let now = Utc::now();

        // Negative deviation (surplus): absorber should increase EV charge regardless of guard
        // Guard only blocks REDUCING charge (positive deviation), not INCREASING it
        let residual = apply_deviation_absorption(
            &mut state,
            -2.0,
            &mut setpoints,
            &sim,
            None,
            &profile,
            now,
            &event_tx,
            Some(&session),
        );

        assert!(
            residual.abs() < 0.1,
            "EV should absorb surplus even near departure, got residual={}",
            residual
        );
        assert!(
            setpoints["ev"] > 0.5,
            "EV setpoint should increase to absorb surplus, got {}",
            setpoints["ev"]
        );
    }

    // T052
    #[test]
    fn absorber_ev_guard_ignored_on_unknown_departure() {
        let mut state = make_fresh_state();
        let profile = make_test_profile(); // EV has guard_s = 1800
        let (sim, mut setpoints) = make_test_sim_battery_floor_ev_charging();
        let event_tx = make_test_event_tx();
        let now = Utc::now();

        // No ev_session → unknown departure → no guard (T048)
        let residual = apply_deviation_absorption(
            &mut state,
            4.0,
            &mut setpoints,
            &sim,
            None,
            &profile,
            now,
            &event_tx,
            None, // no session
        );

        // EV should absorb freely (no session = no guard)
        assert!(
            residual.abs() < 0.1,
            "EV should absorb when departure is unknown, got residual={}",
            residual
        );
        assert!(
            setpoints["ev"] < 7.4 - 0.5,
            "EV setpoint should be reduced, got {}",
            setpoints["ev"]
        );
    }

    // ── Absorber disabled ─────────────────────────────────────────────────────

    #[test]
    fn absorber_disabled_returns_full_deviation_as_residual() {
        let mut state = make_fresh_state();
        let mut params = make_test_profile();
        params.enabled = false;
        let sim = make_test_snap();
        let event_tx = make_test_event_tx();
        let mut setpoints = HashMap::from([("battery".to_string(), 0.0), ("ev".to_string(), 0.0)]);
        let now = Utc::now();

        let residual = apply_deviation_absorption(
            &mut state,
            2.0,
            &mut setpoints,
            &sim,
            None,
            &params,
            now,
            &event_tx,
            None,
        );

        // When disabled: full deviation is returned (passthrough to Tier 2), no setpoint changes
        assert!(
            residual.abs() > 1.9,
            "residual should be full deviation, got {}",
            residual
        );
        assert_eq!(
            setpoints["battery"], 0.0,
            "setpoints unchanged when disabled"
        );
    }

    // ── dead_band_clearing_ticks wait gate ────────────────────────────────────

    fn make_test_profile_clearing_ticks(n: usize) -> AbsorberParams {
        let mut p = make_test_profile();
        p.dead_band_clearing_ticks = n;
        p
    }

    #[test]
    fn absorber_wait_gate_holds_for_multiple_ticks() {
        let profile = make_test_profile_clearing_ticks(3);
        let sim = make_test_snap();
        let event_tx = make_test_event_tx();
        let mut state = make_fresh_state();
        let mut setpoints = HashMap::from([("battery".to_string(), 0.0), ("ev".to_string(), 0.0)]);
        let now = Utc::now();

        // Tick 0: apply correction (above dead-band)
        apply_deviation_absorption(
            &mut state,
            2.0,
            &mut setpoints,
            &sim,
            None,
            &profile,
            now,
            &event_tx,
            None,
        );
        assert!(
            state.correction_is_active,
            "should be active after correction"
        );

        // Ticks 1 and 2: in-band but gate not yet reached (threshold=3)
        for expected_ticks in 1u32..=2 {
            apply_deviation_absorption(
                &mut state,
                0.05,
                &mut setpoints,
                &sim,
                None,
                &profile,
                now,
                &event_tx,
                None,
            );
            assert!(
                state.correction_is_active,
                "tick {}: correction should still be active (gate not fired)",
                expected_ticks
            );
            let ticks = *state.settling_ticks.get("battery").unwrap_or(&0);
            assert_eq!(
                ticks, expected_ticks,
                "tick {}: settling counter mismatch",
                expected_ticks
            );
        }

        // Tick 3: gate fires (counter reaches threshold), overlay cleared
        apply_deviation_absorption(
            &mut state,
            0.05,
            &mut setpoints,
            &sim,
            None,
            &profile,
            now,
            &event_tx,
            None,
        );
        assert!(
            !state.correction_is_active,
            "tick 3: correction should clear after gate fires"
        );
        assert!(
            state.active_overlay_kw.values().all(|&v| v.abs() < 0.01),
            "all overlays should be zero after gate"
        );
    }

    #[test]
    fn absorber_wait_gate_resets_on_chatter() {
        let profile = make_test_profile_clearing_ticks(3);
        let sim = make_test_snap();
        let event_tx = make_test_event_tx();
        let mut state = make_fresh_state();
        let mut setpoints = HashMap::from([("battery".to_string(), 0.0), ("ev".to_string(), 0.0)]);
        let now = Utc::now();

        // Tick 0: apply correction
        apply_deviation_absorption(
            &mut state,
            2.0,
            &mut setpoints,
            &sim,
            None,
            &profile,
            now,
            &event_tx,
            None,
        );

        // Tick 1: in-band, counter = 1
        apply_deviation_absorption(
            &mut state,
            0.05,
            &mut setpoints,
            &sim,
            None,
            &profile,
            now,
            &event_tx,
            None,
        );
        assert_eq!(
            *state.settling_ticks.get("battery").unwrap_or(&0),
            1,
            "counter should be 1 after first in-band tick"
        );

        // Tick 2: deviation returns above dead-band → counter resets to 0
        apply_deviation_absorption(
            &mut state,
            2.0,
            &mut setpoints,
            &sim,
            None,
            &profile,
            now,
            &event_tx,
            None,
        );
        assert_eq!(
            *state.settling_ticks.get("battery").unwrap_or(&0),
            0,
            "counter should reset when deviation returns above dead-band"
        );

        // Tick 3: back in-band → counter = 1 (not 2, because it reset from chatter)
        apply_deviation_absorption(
            &mut state,
            0.05,
            &mut setpoints,
            &sim,
            None,
            &profile,
            now,
            &event_tx,
            None,
        );
        assert!(
            state.correction_is_active,
            "gate not yet reached after reset (need 3 ticks, only have 1)"
        );
        assert_eq!(
            *state.settling_ticks.get("battery").unwrap_or(&0),
            1,
            "counter should restart from 1 after chatter reset"
        );
    }
}
