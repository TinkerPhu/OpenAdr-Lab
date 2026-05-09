use chrono::{DateTime, Utc};

use crate::entities::plan::SiteFlexibilityEnvelope;
use crate::controller::SimSnapshot;

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
pub fn compute_envelope(sim: &SimSnapshot, now: DateTime<Utc>) -> SiteFlexibilityEnvelope {
    let mut up_kw = 0.0_f64;
    let mut down_kw = 0.0_f64;
    let mut available_discharge_kwh = 0.0_f64;
    let mut available_charge_kwh = 0.0_f64;

    for (_id, snap) in &sim.assets {
        up_kw += (snap.power_kw - snap.cap_max_export_kw).max(0.0);
        down_kw += (snap.cap_max_import_kw - snap.power_kw).max(0.0);

        if let (Some(dis), Some(ch)) = (snap.available_discharge_kwh, snap.available_charge_kwh) {
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

    use crate::controller::{AssetSnapshot, GridSnapshot, SimSnapshot};
    use std::collections::HashMap as HM;

    fn make_battery_entry(
        id: &str,
        soc: f64,
        last_power_kw: f64,
        capacity_kwh: f64,
        max_kw: f64,
        min_soc: f64,
    ) -> (String, AssetSnapshot) {
        let cap_max_export_kw = if soc <= min_soc { 0.0 } else { -max_kw };
        let cap_max_import_kw = if soc >= 1.0 { 0.0 } else { max_kw };
        let available_discharge_kwh = Some((soc - min_soc).max(0.0) * capacity_kwh);
        let available_charge_kwh = Some((1.0 - soc).max(0.0) * capacity_kwh);
        let mut values = HM::new();
        values.insert("soc".into(), soc);
        values.insert("capacity_kwh".into(), capacity_kwh);
        values.insert("max_charge_kw".into(), max_kw);
        values.insert("max_discharge_kw".into(), max_kw);
        values.insert("min_soc".into(), min_soc);
        (
            id.to_string(),
            AssetSnapshot {
                power_kw: last_power_kw,
                asset_type: "battery".to_string(),
                cap_max_import_kw,
                cap_max_export_kw,
                available_discharge_kwh,
                available_charge_kwh,
                default_setpoint_kw: 0.0,
                setpoint_kw: last_power_kw,
                values,
            },
        )
    }

    fn make_ev_entry(
        id: &str,
        soc: f64,
        plugged: bool,
        last_power_kw: f64,
        max_charge_kw: f64,
        battery_kwh: f64,
    ) -> (String, AssetSnapshot) {
        let soc_target = 0.8;
        let (cap_max_import_kw, cap_max_export_kw, avail_dis, avail_ch) = if plugged {
            let import = if soc >= soc_target { 0.0 } else { max_charge_kw };
            (import, 0.0_f64, Some(soc * battery_kwh), Some((1.0 - soc) * battery_kwh))
        } else {
            (0.0, 0.0, None, None)
        };
        let mut values = HM::new();
        values.insert("soc".into(), soc);
        values.insert("plugged".into(), if plugged { 1.0 } else { 0.0 });
        values.insert("max_charge_kw".into(), max_charge_kw);
        values.insert("soc_target".into(), soc_target);
        values.insert("battery_kwh".into(), battery_kwh);
        (
            id.to_string(),
            AssetSnapshot {
                power_kw: last_power_kw,
                asset_type: "ev".to_string(),
                cap_max_import_kw,
                cap_max_export_kw,
                available_discharge_kwh: avail_dis,
                available_charge_kwh: avail_ch,
                default_setpoint_kw: max_charge_kw,
                setpoint_kw: last_power_kw,
                values,
            },
        )
    }

    fn make_pv_entry(id: &str, actual_power_kw: f64, rated_kw: f64) -> (String, AssetSnapshot) {
        let mut values = HM::new();
        values.insert("irradiance".into(), 0.5);
        values.insert("rated_kw".into(), rated_kw);
        values.insert("irradiance_offset".into(), 0.0);
        values.insert("pv_alpha".into(), 0.1);
        (
            id.to_string(),
            AssetSnapshot {
                power_kw: actual_power_kw,
                asset_type: "pv".to_string(),
                cap_max_import_kw: actual_power_kw,
                cap_max_export_kw: actual_power_kw,
                available_discharge_kwh: None,
                available_charge_kwh: None,
                default_setpoint_kw: 0.0,
                setpoint_kw: 0.0,
                values,
            },
        )
    }

    fn make_sim(assets: Vec<(String, AssetSnapshot)>) -> SimSnapshot {
        SimSnapshot {
            ts: Utc::now(),
            grid: GridSnapshot {
                net_power_w: 0.0,
                voltage_v: 230.0,
                import_kwh: 0.0,
                export_kwh: 0.0,
            },
            assets: assets.into_iter().collect(),
        }
    }

    #[test]
    fn test_compute_envelope_no_assets_returns_zero() {
        let sim = make_sim(vec![]);
        let env = compute_envelope(&sim, Utc::now());
        assert_eq!(env.up_kw, 0.0);
        assert_eq!(env.down_kw, 0.0);
        assert!(env.up_duration_s.is_none());
        assert!(env.down_duration_s.is_none());
    }

    #[test]
    fn test_compute_envelope_ev_charging_contributes_up() {
        let sim = make_sim(vec![make_ev_entry("ev", 0.5, true, 7.0, 7.0, 40.0)]);
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
        let sim = make_sim(vec![make_battery_entry("bat", 0.5, 0.0, 10.0, 5.0, 0.1)]);
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
        let sim = make_sim(vec![make_pv_entry("pv", -2.0, 5.0)]);
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
        let sim = make_sim(vec![make_battery_entry("bat", 0.5, 0.0, 10.0, 5.0, 0.1)]);
        let env = compute_envelope(&sim, Utc::now());
        assert_eq!(env.up_duration_s, Some(2880));
        assert_eq!(env.down_duration_s, Some(3600));
    }

    #[test]
    fn duration_suppressed_when_max_kw_below_near_zero_kw() {
        let sub_threshold = NEAR_ZERO_KW * 0.5;
        let sim = make_sim(vec![make_battery_entry("bat", 0.5, 0.0, 10.0, sub_threshold, 0.1)]);
        let env = compute_envelope(&sim, Utc::now());
        assert!(env.up_duration_s.is_none());
        assert!(env.down_duration_s.is_none());
    }

    #[test]
    fn duration_present_when_max_kw_above_near_zero_kw() {
        let above_threshold = NEAR_ZERO_KW * 2.0;
        let sim = make_sim(vec![make_battery_entry("bat", 0.5, 0.0, 10.0, above_threshold, 0.1)]);
        let env = compute_envelope(&sim, Utc::now());
        assert!(env.up_duration_s.is_some());
        assert!(env.down_duration_s.is_some());
    }

    // ── T015: hand-built SimSnapshot test (no SimState) ──────────────────────

    /// Build SimSnapshot directly (not via SimState) to demonstrate trait decoupling.
    /// Battery at 0 kW with 5 kW import/export caps → up_kw=5.0, down_kw=5.0.
    #[test]
    fn compute_envelope_hand_built_snapshot_battery_idle() {
        // Battery idle at 0 kW; cap_max_export_kw is negative (export = negative convention)
        let mut assets = HM::new();
        assets.insert(
            "battery".to_string(),
            AssetSnapshot {
                power_kw: 0.0,
                asset_type: "battery".to_string(),
                cap_max_import_kw: 5.0,   // max charging power
                cap_max_export_kw: -5.0,  // max discharging power (negative = export)
                available_discharge_kwh: Some(4.0),
                available_charge_kwh: Some(6.0),
                default_setpoint_kw: 0.0,
                setpoint_kw: 0.0,
                values: HM::new(),
            },
        );
        let sim = SimSnapshot {
            ts: Utc::now(),
            grid: GridSnapshot {
                net_power_w: 0.0,
                voltage_v: 230.0,
                import_kwh: 0.0,
                export_kwh: 0.0,
            },
            assets,
        };

        let env = compute_envelope(&sim, Utc::now());

        // up_kw = (0.0 − (−5.0)).max(0) = 5.0
        assert!(
            (env.up_kw - 5.0).abs() < 1e-6,
            "up_kw: expected 5.0, got {}",
            env.up_kw
        );
        // down_kw = (5.0 − 0.0).max(0) = 5.0
        assert!(
            (env.down_kw - 5.0).abs() < 1e-6,
            "down_kw: expected 5.0, got {}",
            env.down_kw
        );
        // up_duration = 4.0 / 5.0 * 3600 = 2880 s
        assert_eq!(env.up_duration_s, Some(2880));
        // down_duration = 6.0 / 5.0 * 3600 = 4320 s
        assert_eq!(env.down_duration_s, Some(4320));
    }
}
