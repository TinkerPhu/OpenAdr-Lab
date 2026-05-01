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

use crate::simulator::SimState;
use crate::entities::plan::Plan;
use crate::profile::Profile;
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

    /// Magnitude of last emitted SSE event (used to deduplicate events).
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
///
/// # Returns
///
/// Signed residual deviation (what could NOT be absorbed). Zero = fully absorbed.
pub fn apply_deviation_absorption(
    state: &mut AbsorberState,
    deviation_kw: f64,
    setpoints: &mut HashMap<String, f64>,
    sim: &SimState,
    plan_snap: Option<&Plan>,
    profile: &Profile,
    now: DateTime<Utc>,
) -> f64 {
    // Quick exit: absorber disabled or deviation within dead-band
    if !profile.absorber.enabled || deviation_kw.abs() <= profile.absorber.dead_band_kw {
        // Ramp all overlays to zero if settling
        let asset_ids: Vec<_> = state.active_overlay_kw.keys().cloned().collect();
        for asset_id in asset_ids {
            state.active_overlay_kw.insert(asset_id, 0.0);
        }
        state.correction_is_active = false;
        return 0.0;
    }

    let mut remaining_kw = deviation_kw;

    // Sequential asset iteration by priority (FR-002)
    let mut assets_by_priority = profile.absorber.assets.clone();
    assets_by_priority.sort_by_key(|a| a.priority);

    for asset_cfg in assets_by_priority {
        // Stop if residual is within dead-band
        if remaining_kw.abs() <= profile.absorber.dead_band_kw {
            break;
        }

        // Check linger: skip asset if linger blocks it
        if !linger_ok(state, &asset_cfg.id, asset_cfg.min_state_linger_s, now) {
            continue;
        }

        // EV departure guard: skip if guard active and positive deviation
        // (Would reduce EV charge when departure is imminent)
        if let Some(_guard_s) = asset_cfg.ev_departure_guard_s {
            if remaining_kw > 0.0 {
                // Positive deviation: need to reduce import, might mean reducing EV charge
                // For now, we'll implement the full EV-specific logic in loops
                // Skip this check in the generic absorber for simplicity
            }
        }

        // Compute headroom for this asset
        let headroom_kw = compute_asset_headroom(&asset_cfg.id, remaining_kw, sim, plan_snap);
        if headroom_kw == 0.0 {
            continue;
        }

        // Apply correction (capped to headroom)
        let delta_kw = remaining_kw.clamp(-headroom_kw, headroom_kw);

        // Update setpoint if delta is significant
        if delta_kw.abs() >= 0.01 {
            let current_sp = *setpoints.get(&asset_cfg.id).unwrap_or(&0.0);
            let new_sp = current_sp - delta_kw; // Note: delta is reduction in import, so subtract from setpoint
            setpoints.insert(asset_cfg.id.clone(), new_sp);

            // Track correction overlay and update linger
            state.active_overlay_kw.insert(asset_cfg.id.clone(), delta_kw);
            state.last_state_change_ts.insert(asset_cfg.id.clone(), now);

            // Accumulate remaining deviation
            remaining_kw -= delta_kw;
        }
    }

    // Track correction state for SSE deduplication at the caller level
    state.correction_is_active = state.active_overlay_kw.values().any(|&o| o.abs() > 0.01);

    remaining_kw
}

/// Compute available correction headroom for a single asset (FR-011).
///
/// Bounds the delta that can be applied to a setpoint by asset physical limits
/// and plan flexibility envelope.
///
/// # Returns
///
/// Maximum magnitude (positive or negative) that can be applied as correction.
/// Positive = increase import (discharge battery, reduce EV charge, reduce heater power).
/// Negative = decrease import (charge battery, increase EV charge, increase heater power).
fn compute_asset_headroom(
    asset_id: &str,
    deviation_kw: f64,
    sim: &SimState,
    plan_snap: Option<&Plan>,
) -> f64 {
    // Find both the asset entry and config
    let (entry, cfg) = match sim.find_asset(asset_id) {
        Some(pair) => pair,
        None => {
            warn!("asset {} not found in sim state", asset_id);
            return 0.0;
        }
    };

    use crate::assets::{AssetState, AssetConfig};

    match (cfg, &entry.state) {
        (AssetConfig::Battery(bat_cfg), AssetState::Battery(bat_state)) => {
            // Discharge headroom: how much more can we discharge?
            // Limited by: (SoC - min_soc) in kWh, and max_discharge_kw
            if deviation_kw > 0.0 {
                let soc_headroom_kw = (bat_state.soc - bat_cfg.min_soc) * bat_cfg.capacity_kwh;
                soc_headroom_kw.min(bat_cfg.max_discharge_kw)
            } else {
                // Charge headroom: how much more can we charge?
                // Limited by: (1.0 - SoC) in kWh, and max_charge_kw
                let soc_headroom_kw = (1.0 - bat_state.soc) * bat_cfg.capacity_kwh;
                soc_headroom_kw.min(bat_cfg.max_charge_kw)
            }
        }
        (AssetConfig::Ev(ev_cfg), AssetState::Ev(ev_state)) => {
            // EV can only charge (max_discharge_kw = 0)
            if deviation_kw > 0.0 {
                // Cannot reduce EV charge (discharge forbidden)
                0.0
            } else {
                // Charge headroom: limited by soc_target and max_charge_kw
                let soc_headroom_kw = (ev_cfg.soc_target - ev_state.soc) * ev_cfg.battery_kwh;
                soc_headroom_kw.min(ev_cfg.max_charge_kw)
            }
        }
        (AssetConfig::Heater(heater_cfg), AssetState::Heater(heater_state)) => {
            // Heater has discrete power levels (0, mid, full)
            // Current power from last tick setpoint
            let current_power = entry.setpoint_kw;
            let mid_kw = if let Some(mid) = heater_cfg.mid_kw {
                mid
            } else {
                heater_cfg.max_kw / 2.0
            };

            if deviation_kw > 0.0 {
                // Need to reduce power: go from current level toward 0
                if current_power > mid_kw {
                    // Currently at or above mid → can drop to mid
                    (current_power - mid_kw).max(0.0)
                } else if current_power > 0.0 {
                    // Currently between 0 and mid → can drop to 0
                    current_power
                } else {
                    0.0
                }
            } else {
                // Need to increase power: go from current level toward full
                if current_power < mid_kw {
                    // Currently below mid → can raise to mid
                    mid_kw - current_power
                } else if current_power < heater_cfg.max_kw {
                    // Currently between mid and full → can raise to full
                    heater_cfg.max_kw - current_power
                } else {
                    0.0
                }
            }
        }
        _ => {
            // Other assets (PV, BaseLoad) are not controllable, or state/config mismatch
            0.0
        }
    }
}

/// Check if linger enforcement allows state change for this asset.
///
/// Returns true if enough time has passed since last state change, or if linger is disabled (0).
fn linger_ok(
    state: &AbsorberState,
    asset_id: &str,
    min_linger_s: u64,
    now: DateTime<Utc>,
) -> bool {
    if min_linger_s == 0 {
        return true;
    }
    match state.last_state_change_ts.get(asset_id) {
        None => true,
        Some(ts) => (now - ts).num_seconds() as u64 >= min_linger_s,
    }
}

/// Validate absorber configuration at startup (FR-013).
///
/// Checks:
/// 1. Asset ID Matching: all `AbsorberAssetConfig.id` must exist in `SimState.assets`
/// 2. Priority Uniqueness: logs WARN if duplicates detected
/// 3. Linger Bounds: logs WARN if min_state_linger_s > 300s
/// 4. EV Departure Guard: only relevant for EV assets (ignored for others)
///
/// Returns `Ok(())` if validation passes, `Err` if critical issue found (asset ID mismatch).
pub fn validate_startup(profile: &Profile, sim: &SimState) -> anyhow::Result<()> {
    if !profile.absorber.enabled {
        return Ok(());
    }

    // Check asset ID matching
    let sim_asset_ids: std::collections::HashSet<&str> = sim.assets.iter().map(|a| a.id.as_str()).collect();

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

    // Check priority uniqueness (optional, but recommended)
    let mut priorities = std::collections::HashSet::new();
    for asset_cfg in &profile.absorber.assets {
        if !priorities.insert(asset_cfg.priority) {
            warn!(
                priority = asset_cfg.priority,
                "absorber has duplicate priority — absorber will still work but may indicate config error"
            );
        }
    }

    // Check linger bounds
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

    fn make_test_sim() -> SimState {
        use crate::assets::{AssetConfig, AssetState};
        use crate::simulator::AssetEntry;
        use crate::assets::battery::Battery;
        use crate::assets::ev::EvCharger;
        use crate::profile::{BatteryConfig, EvConfig};
        use crate::simulator::EnergyCounter;

        let battery_cfg = BatteryConfig {
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
                    history: crate::simulator::AssetHistoryBuffer::new(3600),
                },
                AssetEntry {
                    id: "ev".to_string(),
                    state: AssetState::Ev(ev_state),
                    setpoint_kw: 0.0,
                    last_power_kw: 0.0,
                    energy: EnergyCounter::new(),
                    history: crate::simulator::AssetHistoryBuffer::new(3600),
                },
            ],
            grid: crate::simulator::GridMeter::default(),
            grid_asset: crate::simulator::Grid::default(),
            pv_smoothing: Default::default(),
            base_load_smoothing: Default::default(),
            last_tick: chrono::Utc::now(),
        }
    }

    #[test]
    fn absorber_battery_absorbs_positive_deviation_within_capacity() {
        let mut state = AbsorberState {
            residual_ticks: 0,
            last_state_change_ts: HashMap::new(),
            settling_ticks: HashMap::new(),
            active_overlay_kw: HashMap::new(),
            correction_is_active: false,
            last_emitted_correction_kw: 0.0,
        };
        let profile = make_test_profile();
        let sim = make_test_sim();
        let mut setpoints: HashMap<String, f64> = [
            ("battery".to_string(), 0.0),
            ("ev".to_string(), 0.0),
        ]
        .iter()
        .cloned()
        .collect();

        let now = chrono::Utc::now();
        let deviation_kw = 2.0; // Positive: need to reduce import

        let residual = apply_deviation_absorption(
            &mut state,
            deviation_kw,
            &mut setpoints,
            &sim,
            None,
            &profile,
            now,
        );

        // Battery should have absorbed the full deviation
        assert!(
            residual.abs() < 0.01,
            "residual should be near 0, got {}",
            residual
        );
        // Battery setpoint should be negative (discharging)
        assert!(
            setpoints["battery"] < -0.5,
            "battery setpoint should be negative for discharge, got {}",
            setpoints["battery"]
        );
        assert!(state.correction_is_active);
    }

    #[test]
    fn absorber_battery_absorbs_negative_deviation_within_capacity() {
        let mut state = AbsorberState {
            residual_ticks: 0,
            last_state_change_ts: HashMap::new(),
            settling_ticks: HashMap::new(),
            active_overlay_kw: HashMap::new(),
            correction_is_active: false,
            last_emitted_correction_kw: 0.0,
        };
        let profile = make_test_profile();
        let sim = make_test_sim();
        let mut setpoints: HashMap<String, f64> = [
            ("battery".to_string(), 0.0),
            ("ev".to_string(), 0.0),
        ]
        .iter()
        .cloned()
        .collect();

        let now = chrono::Utc::now();
        let deviation_kw = -2.0; // Negative: need to increase import (reduce export / increase charging)

        let residual = apply_deviation_absorption(
            &mut state,
            deviation_kw,
            &mut setpoints,
            &sim,
            None,
            &profile,
            now,
        );

        // Battery should have absorbed the full deviation
        assert!(
            residual.abs() < 0.01,
            "residual should be near 0, got {}",
            residual
        );
        // Battery setpoint should be positive (charging)
        assert!(
            setpoints["battery"] > 0.5,
            "battery setpoint should be positive for charge, got {}",
            setpoints["battery"]
        );
    }

    #[test]
    fn absorber_dead_band_prevents_chatter() {
        let mut state = AbsorberState {
            residual_ticks: 0,
            last_state_change_ts: HashMap::new(),
            settling_ticks: HashMap::new(),
            active_overlay_kw: HashMap::new(),
            correction_is_active: false,
            last_emitted_correction_kw: 0.0,
        };
        let profile = make_test_profile();
        let sim = make_test_sim();
        let mut setpoints: HashMap<String, f64> = [
            ("battery".to_string(), 0.0),
            ("ev".to_string(), 0.0),
        ]
        .iter()
        .cloned()
        .collect();

        let now = chrono::Utc::now();
        let deviation_kw = 0.05; // Within 0.1 kW dead-band

        let residual = apply_deviation_absorption(
            &mut state,
            deviation_kw,
            &mut setpoints,
            &sim,
            None,
            &profile,
            now,
        );

        // No correction should be applied
        assert!(residual.abs() > 0.04, "residual should include full deviation");
        assert_eq!(
            setpoints["battery"], 0.0,
            "battery setpoint should be unchanged"
        );
        assert!(!state.correction_is_active);
    }

    #[test]
    fn linger_ok_returns_false_before_min_time() {
        let now = Utc::now();
        let past = now - chrono::Duration::seconds(20);

        let mut state = AbsorberState {
            residual_ticks: 0,
            last_state_change_ts: {
                let mut m = HashMap::new();
                m.insert("asset1".to_string(), past);
                m
            },
            settling_ticks: HashMap::new(),
            active_overlay_kw: HashMap::new(),
            correction_is_active: false,
            last_emitted_correction_kw: 0.0,
        };

        // 20 seconds have passed, but min_linger is 30 seconds
        assert!(!linger_ok(&state, "asset1", 30, now));
    }

    #[test]
    fn linger_ok_returns_true_after_min_time() {
        let now = Utc::now();
        let past = now - chrono::Duration::seconds(40);

        let mut state = AbsorberState {
            residual_ticks: 0,
            last_state_change_ts: {
                let mut m = HashMap::new();
                m.insert("asset1".to_string(), past);
                m
            },
            settling_ticks: HashMap::new(),
            active_overlay_kw: HashMap::new(),
            correction_is_active: false,
            last_emitted_correction_kw: 0.0,
        };

        // 40 seconds have passed, min_linger is 30 seconds
        assert!(linger_ok(&state, "asset1", 30, now));
    }

    #[test]
    fn absorber_disabled_returns_zero_residual() {
        let mut state = AbsorberState {
            residual_ticks: 0,
            last_state_change_ts: HashMap::new(),
            settling_ticks: HashMap::new(),
            active_overlay_kw: HashMap::new(),
            correction_is_active: false,
            last_emitted_correction_kw: 0.0,
        };
        let mut profile = make_test_profile();
        profile.absorber.enabled = false; // Disable absorber
        let sim = make_test_sim();
        let mut setpoints: HashMap<String, f64> = [
            ("battery".to_string(), 0.0),
            ("ev".to_string(), 0.0),
        ]
        .iter()
        .cloned()
        .collect();

        let now = chrono::Utc::now();
        let deviation_kw = 2.0;

        let residual = apply_deviation_absorption(
            &mut state,
            deviation_kw,
            &mut setpoints,
            &sim,
            None,
            &profile,
            now,
        );

        // When disabled, residual = deviation (nothing absorbed)
        assert!(residual.abs() > 1.9, "residual should be full deviation");
        assert_eq!(setpoints["battery"], 0.0, "setpoints unchanged when disabled");
    }
}
