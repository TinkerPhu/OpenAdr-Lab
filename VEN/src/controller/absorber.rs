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

use crate::assets::{AssetConfig, AssetState};
use crate::entities::device_session::EvSession;
use crate::entities::plan::Plan;
use crate::planner_events::{PlannerEvent, PlannerEventTx};
use crate::profile::{PlannerObjective, Profile};
use crate::simulator::SimState;
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

    /// Per-asset: counter of ticks during settling phase (ramps overlay to zero).
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
    sim: &SimState,
    plan_snap: Option<&Plan>,
    profile: &Profile,
    now: DateTime<Utc>,
    event_tx: &PlannerEventTx,
    ev_session: Option<&EvSession>,
) -> f64 {
    let was_active = state.correction_is_active;

    // Early exit: absorber disabled — passthrough full deviation to Tier 2.
    if !profile.absorber.enabled {
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

    // Within dead-band: settle overlays, don't apply any correction.
    // Return deviation as-is (small enough that Tier 2 dead-band will also absorb it).
    if deviation_kw.abs() <= profile.absorber.dead_band_kw {
        let asset_ids: Vec<_> = state.active_overlay_kw.keys().cloned().collect();
        for id in asset_ids {
            state.active_overlay_kw.insert(id, 0.0);
        }
        if was_active {
            let _ = event_tx.send(PlannerEvent::CorrectionCleared {
                ts: now,
                reason: "deviation_cleared".to_string(),
            });
            state.last_emitted_correction_kw = 0.0;
        }
        state.correction_is_active = false;
        return deviation_kw;
    }

    let mut remaining_kw = deviation_kw;

    // Sequential asset iteration by priority (FR-002)
    let mut assets_by_priority = profile.absorber.assets.clone();
    assets_by_priority.sort_by_key(|a| a.priority);

    for asset_cfg in &assets_by_priority {
        // Stop if residual is within dead-band
        if remaining_kw.abs() <= profile.absorber.dead_band_kw {
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
                if let Some((entry, cfg)) = sim.find_asset(&asset_cfg.id) {
                    if let (AssetConfig::Ev(ev_cfg), AssetState::Ev(ev_state)) = (cfg, &entry.state)
                    {
                        if ev_state.soc < ev_cfg.soc_target {
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
    sim: &SimState,
    _plan_snap: Option<&Plan>,
    current_setpoint_kw: f64,
) -> f64 {
    let (entry, cfg) = match sim.find_asset(asset_id) {
        Some(pair) => pair,
        None => {
            warn!("absorber: asset {} not found in sim state", asset_id);
            return 0.0;
        }
    };

    match (cfg, &entry.state) {
        (AssetConfig::Battery(bat_cfg), AssetState::Battery(bat_state)) => {
            if deviation_kw > 0.0 {
                // Discharge headroom: limited by (SoC - min_soc) and max_discharge_kw
                let soc_headroom_kw = (bat_state.soc - bat_cfg.min_soc) * bat_cfg.capacity_kwh;
                soc_headroom_kw.max(0.0).min(bat_cfg.max_discharge_kw)
            } else {
                // Charge headroom: limited by (1.0 - SoC) and max_charge_kw
                let soc_headroom_kw = (1.0 - bat_state.soc) * bat_cfg.capacity_kwh;
                soc_headroom_kw.max(0.0).min(bat_cfg.max_charge_kw)
            }
        }
        (AssetConfig::Ev(ev_cfg), AssetState::Ev(ev_state)) => {
            if deviation_kw > 0.0 {
                // Curtail EV charging (load reduction, not V2G discharge).
                // Headroom = current charge setpoint (how much rate we can reduce, down to 0).
                current_setpoint_kw.max(0.0)
            } else {
                // Increase EV charging to absorb surplus.
                // Headroom limited by remaining SoC capacity and max charge rate.
                let soc_headroom_kw = (ev_cfg.soc_target - ev_state.soc) * ev_cfg.battery_kwh;
                soc_headroom_kw.max(0.0).min(ev_cfg.max_charge_kw)
            }
        }
        (AssetConfig::Heater(heater_cfg), AssetState::Heater(_heater_state)) => {
            // Heater has discrete power levels (0, mid, full)
            let current_power = entry.setpoint_kw;
            let mid_kw = heater_cfg.mid_kw;

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
                } else if current_power < heater_cfg.max_kw {
                    heater_cfg.max_kw - current_power
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
pub fn validate_startup(profile: &Profile, sim: &SimState) -> anyhow::Result<()> {
    if !profile.absorber.enabled {
        return Ok(());
    }

    let sim_asset_ids: std::collections::HashSet<&str> =
        sim.assets.iter().map(|a| a.id.as_str()).collect();

    for asset_cfg in &profile.absorber.assets {
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
    for asset_cfg in &profile.absorber.assets {
        if !priorities.insert(asset_cfg.priority) {
            warn!(
                priority = asset_cfg.priority,
                "absorber has duplicate priority — may indicate config error"
            );
        }
    }

    for asset_cfg in &profile.absorber.assets {
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

    fn make_test_profile() -> Profile {
        use crate::profile::{AbsorberAssetConfig, AbsorberConfig};

        let mut profile = Profile::default();
        profile.absorber = AbsorberConfig {
            enabled: true,
            dead_band_kw: 0.1,
            dead_band_clearing_ticks: 1,
            assets: vec![
                AbsorberAssetConfig {
                    id: "battery".to_string(),
                    priority: 0,
                    min_state_linger_s: 0,
                    ev_departure_guard_s: None,
                },
                AbsorberAssetConfig {
                    id: "ev".to_string(),
                    priority: 1,
                    min_state_linger_s: 0,
                    ev_departure_guard_s: Some(1800),
                },
            ],
        };
        profile
    }

    fn make_test_profile_battery_linger() -> Profile {
        use crate::profile::{AbsorberAssetConfig, AbsorberConfig};

        let mut profile = Profile::default();
        profile.absorber = AbsorberConfig {
            enabled: true,
            dead_band_kw: 0.1,
            dead_band_clearing_ticks: 1,
            assets: vec![AbsorberAssetConfig {
                id: "battery".to_string(),
                priority: 0,
                min_state_linger_s: 30, // 30 s linger for relay wear test
                ev_departure_guard_s: None,
            }],
        };
        profile
    }

    fn make_test_sim() -> SimState {
        use crate::assets::battery::Battery;
        use crate::assets::ev::EvCharger;
        use crate::profile::{BatteryConfig, EvConfig};
        use crate::simulator::energy::EnergyCounter;
        use crate::simulator::AssetEntry;

        let battery_cfg = BatteryConfig {
            id: "battery".to_string(),
            capacity_kwh: 10.0,
            max_charge_kw: 5.0,
            max_discharge_kw: 5.0,
            initial_soc: 0.50,
            round_trip_efficiency: 0.92,
            min_soc: 0.10,
        };
        let battery = Battery::from_config(&battery_cfg);
        let battery_state = crate::assets::battery::BatteryState {
            soc: 0.50,
            actual_power_kw: 0.0,
        };

        let ev_cfg = EvConfig {
            id: "ev".to_string(),
            max_charge_kw: 7.4,
            max_discharge_kw: 0.0,
            initial_soc: 0.30,
            battery_kwh: 60.0,
            soc_target: 0.80,
            default_charge_kw: 0.0,
            min_charge_kw: 1.4,
        };
        let ev = EvCharger::from_config(&ev_cfg);
        let ev_state = crate::assets::ev::EvState {
            soc: 0.30,
            plugged: true,
            actual_power_kw: 0.0,
        };

        SimState {
            asset_configs: vec![AssetConfig::Battery(battery), AssetConfig::Ev(ev)],
            assets: vec![
                AssetEntry {
                    id: "battery".to_string(),
                    state: AssetState::Battery(battery_state),
                    setpoint_kw: 0.0,
                    last_power_kw: 0.0,
                    energy: EnergyCounter::new(),
                    history: crate::assets::AssetHistoryBuffer::new(3600),
                },
                AssetEntry {
                    id: "ev".to_string(),
                    state: AssetState::Ev(ev_state),
                    setpoint_kw: 0.0,
                    last_power_kw: 0.0,
                    energy: EnergyCounter::new(),
                    history: crate::assets::AssetHistoryBuffer::new(3600),
                },
            ],
            grid: crate::simulator::GridMeter::default(),
            grid_asset: crate::assets::Grid::default(),
            pv_smoothing: Default::default(),
            base_load_smoothing: Default::default(),
            last_tick: chrono::Utc::now(),
        }
    }

    /// Battery at min_soc (no discharge headroom), EV charging at 7.4 kW.
    fn make_test_sim_battery_floor_ev_charging() -> (SimState, HashMap<String, f64>) {
        let mut sim = make_test_sim();
        if let AssetState::Battery(ref mut s) = sim.assets[0].state {
            s.soc = 0.10; // At min_soc → 0 discharge headroom
        }
        sim.assets[1].setpoint_kw = 7.4; // EV charging at max rate
        let setpoints = HashMap::from([("battery".to_string(), 0.0), ("ev".to_string(), 7.4)]);
        (sim, setpoints)
    }

    /// Battery at min_soc, EV idle (setpoint 0) — both have 0 positive-deviation headroom.
    fn make_test_sim_all_exhausted_positive() -> (SimState, HashMap<String, f64>) {
        let mut sim = make_test_sim();
        if let AssetState::Battery(ref mut s) = sim.assets[0].state {
            s.soc = 0.10;
        }
        sim.assets[1].setpoint_kw = 0.0; // EV idle → 0 curtailment headroom
        let setpoints = HashMap::from([("battery".to_string(), 0.0), ("ev".to_string(), 0.0)]);
        (sim, setpoints)
    }

    /// Battery fully charged (no charge headroom), EV idle — for negative deviation tests.
    fn make_test_sim_battery_full_ev_idle() -> (SimState, HashMap<String, f64>) {
        let mut sim = make_test_sim();
        if let AssetState::Battery(ref mut s) = sim.assets[0].state {
            s.soc = 1.0; // Full → 0 charge headroom for negative deviation
        }
        sim.assets[1].setpoint_kw = 0.0;
        let setpoints = HashMap::from([("battery".to_string(), 0.0), ("ev".to_string(), 0.0)]);
        (sim, setpoints)
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

    // ── User Story 1: Transient Deviation Absorption ──────────────────────────

    #[test]
    fn absorber_battery_absorbs_positive_deviation_within_capacity() {
        let mut state = make_fresh_state();
        let profile = make_test_profile();
        let sim = make_test_sim();
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
        let sim = make_test_sim();
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
        let sim = make_test_sim();
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
        let sim = make_test_sim();
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

        let profile = make_test_profile_battery_linger(); // battery has 30 s linger
        let sim = make_test_sim(); // battery at SoC 0.50 — has headroom
        let event_tx = make_test_event_tx();
        let mut setpoints = HashMap::from([("battery".to_string(), 0.0)]);

        // Battery linger blocks (10 s < 30 s), profile has no EV asset → full residual
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
        let mut profile = make_test_profile();
        profile.absorber.enabled = false;
        let sim = make_test_sim();
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
}
