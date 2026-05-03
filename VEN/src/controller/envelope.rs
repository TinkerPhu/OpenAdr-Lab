use chrono::{DateTime, Utc};

use crate::entities::plan::SiteFlexibilityEnvelope;
use crate::simulator::SimState;

const NEAR_ZERO_KW: f64 = 1e-3;
const NEAR_ZERO_KWH: f64 = 1e-6;

/// Compute the site-level flexibility envelope from current asset states.
///
/// up_kw:   how much the VEN can reduce grid consumption right now (kW, ≥ 0).
/// down_kw: how much the VEN can increase grid consumption right now (kW, ≥ 0).
///
/// For each asset:
///   phys_cap = config.capability(&entry.state)
///   up_kw   += (entry.last_power_kw − phys_cap.max_export_kw).max(0.0)
///   down_kw += (phys_cap.max_import_kw − entry.last_power_kw).max(0.0)
///
/// Uncontrollable assets (PV, BaseLoad) have a point-range capability so they
/// contribute 0 to both directions.
///
/// Duration is estimated from available storage energy:
///   up_duration_s   = available_discharge_kwh / up_kw × 3600
///   down_duration_s = available_charge_kwh    / down_kw × 3600
pub fn compute_envelope(sim: &SimState, now: DateTime<Utc>) -> SiteFlexibilityEnvelope {
    let mut up_kw = 0.0_f64;
    let mut down_kw = 0.0_f64;
    let mut available_discharge_kwh = 0.0_f64;
    let mut available_charge_kwh = 0.0_f64;

    for (entry, config) in sim.iter_assets() {
        let phys_cap = config.capability(&entry.state);

        up_kw += (entry.last_power_kw - phys_cap.max_export_kw).max(0.0);
        down_kw += (phys_cap.max_import_kw - entry.last_power_kw).max(0.0);

        if let Some((dis, ch)) = config.available_storage_kwh(&entry.state) {
            available_discharge_kwh += dis;
            available_charge_kwh += ch;
        }
    }

    let up_duration_s = if up_kw > NEAR_ZERO_KW && available_discharge_kwh > NEAR_ZERO_KWH {
        Some((available_discharge_kwh / up_kw * 3600.0) as u64)
    } else {
        None
    };
    let down_duration_s = if down_kw > NEAR_ZERO_KW && available_charge_kwh > NEAR_ZERO_KWH {
        Some((available_charge_kwh / down_kw * 3600.0) as u64)
    } else {
        None
    };

    SiteFlexibilityEnvelope {
        ts: now,
        up_kw,
        down_kw,
        up_duration_s,
        down_duration_s,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    use crate::assets::{
        AssetConfig, AssetHistoryBuffer, AssetState, Battery, BatteryState, EvCharger, EvState,
        Grid, PvInverter, PvState,
    };
    use crate::simulator::{energy::EnergyCounter, AssetEntry, GridMeter, SimState};

    fn make_battery_entry(id: &str, soc: f64, last_power_kw: f64) -> AssetEntry {
        AssetEntry {
            id: id.to_string(),
            state: AssetState::Battery(BatteryState {
                soc,
                actual_power_kw: last_power_kw,
            }),
            setpoint_kw: last_power_kw,
            last_power_kw,
            energy: EnergyCounter::new(),
            history: AssetHistoryBuffer::new(3600),
        }
    }

    fn make_battery_config(capacity_kwh: f64, max_kw: f64, min_soc: f64) -> AssetConfig {
        AssetConfig::Battery(Battery {
            capacity_kwh,
            max_charge_kw: max_kw,
            max_discharge_kw: max_kw,
            round_trip_efficiency: 1.0,
            min_soc,
        })
    }

    fn make_ev_entry(id: &str, soc: f64, plugged: bool, last_power_kw: f64) -> AssetEntry {
        AssetEntry {
            id: id.to_string(),
            state: AssetState::Ev(EvState {
                soc,
                plugged,
                actual_power_kw: last_power_kw,
            }),
            setpoint_kw: last_power_kw,
            last_power_kw,
            energy: EnergyCounter::new(),
            history: AssetHistoryBuffer::new(3600),
        }
    }

    fn make_ev_config(max_charge_kw: f64, battery_kwh: f64) -> AssetConfig {
        AssetConfig::Ev(EvCharger {
            max_charge_kw,
            max_discharge_kw: 0.0,
            battery_kwh,
            soc_target: 0.8,
            soc_target_profile: 0.8,
            default_charge_kw: max_charge_kw,
            min_soc: 0.0,
        })
    }

    fn make_pv_entry(id: &str, actual_power_kw: f64) -> AssetEntry {
        AssetEntry {
            id: id.to_string(),
            state: AssetState::Pv(PvState { actual_power_kw }),
            setpoint_kw: 0.0,
            last_power_kw: actual_power_kw,
            energy: EnergyCounter::new(),
            history: AssetHistoryBuffer::new(3600),
        }
    }

    fn make_pv_config(rated_kw: f64) -> AssetConfig {
        AssetConfig::Pv(PvInverter {
            rated_kw,
            export_limit_kw: None,
            irradiance: 0.5,
            irradiance_offset: 0.0,
            pv_alpha: 0.1,
        })
    }

    fn make_sim(asset_configs: Vec<AssetConfig>, assets: Vec<AssetEntry>) -> SimState {
        SimState {
            asset_configs,
            assets,
            grid: GridMeter::default(),
            grid_asset: Grid::new(),
            pv_smoothing: crate::simulator::PvSmoothingState::default(),
            base_load_smoothing: Default::default(),
            last_tick: Utc::now(),
        }
    }

    #[test]
    fn test_compute_envelope_no_assets_returns_zero() {
        let sim = make_sim(vec![], vec![]);
        let env = compute_envelope(&sim, Utc::now());
        assert_eq!(env.up_kw, 0.0);
        assert_eq!(env.down_kw, 0.0);
        assert!(env.up_duration_s.is_none());
        assert!(env.down_duration_s.is_none());
    }

    #[test]
    fn test_compute_envelope_ev_charging_contributes_up() {
        let sim = make_sim(
            vec![make_ev_config(7.0, 40.0)],
            vec![make_ev_entry("ev", 0.5, true, 7.0)],
        );
        let env = compute_envelope(&sim, Utc::now());
        assert!(
            (env.up_kw - 7.0).abs() < 1e-6,
            "up_kw should be 7.0, got {}",
            env.up_kw
        );
        assert!(
            (env.down_kw).abs() < 1e-6,
            "down_kw should be 0.0, got {}",
            env.down_kw
        );
    }

    #[test]
    fn test_compute_envelope_battery_idle_contributes_both() {
        let sim = make_sim(
            vec![make_battery_config(10.0, 5.0, 0.1)],
            vec![make_battery_entry("bat", 0.5, 0.0)],
        );
        let env = compute_envelope(&sim, Utc::now());
        assert!(
            (env.up_kw - 5.0).abs() < 1e-6,
            "up_kw should be 5.0, got {}",
            env.up_kw
        );
        assert!(
            (env.down_kw - 5.0).abs() < 1e-6,
            "down_kw should be 5.0, got {}",
            env.down_kw
        );
    }

    #[test]
    fn test_compute_envelope_pv_contributes_nothing() {
        let sim = make_sim(vec![make_pv_config(5.0)], vec![make_pv_entry("pv", -2.0)]);
        let env = compute_envelope(&sim, Utc::now());
        assert!(
            (env.up_kw).abs() < 1e-6,
            "PV up_kw should be 0, got {}",
            env.up_kw
        );
        assert!(
            (env.down_kw).abs() < 1e-6,
            "PV down_kw should be 0, got {}",
            env.down_kw
        );
    }

    #[test]
    fn test_compute_envelope_duration_from_battery_soc() {
        let sim = make_sim(
            vec![make_battery_config(10.0, 5.0, 0.1)],
            vec![make_battery_entry("bat", 0.5, 0.0)],
        );
        let env = compute_envelope(&sim, Utc::now());
        assert_eq!(env.up_duration_s, Some(2880));
        assert_eq!(env.down_duration_s, Some(3600));
    }

    #[test]
    fn duration_suppressed_when_max_kw_below_near_zero_kw() {
        let sub_threshold = NEAR_ZERO_KW * 0.5;
        let sim = make_sim(
            vec![make_battery_config(10.0, sub_threshold, 0.1)],
            vec![make_battery_entry("bat", 0.5, 0.0)],
        );
        let env = compute_envelope(&sim, Utc::now());
        assert!(env.up_duration_s.is_none());
        assert!(env.down_duration_s.is_none());
    }

    #[test]
    fn duration_present_when_max_kw_above_near_zero_kw() {
        let above_threshold = NEAR_ZERO_KW * 2.0;
        let sim = make_sim(
            vec![make_battery_config(10.0, above_threshold, 0.1)],
            vec![make_battery_entry("bat", 0.5, 0.0)],
        );
        let env = compute_envelope(&sim, Utc::now());
        assert!(env.up_duration_s.is_some());
        assert!(env.down_duration_s.is_some());
    }
}
