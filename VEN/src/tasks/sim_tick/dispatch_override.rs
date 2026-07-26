// WP3.4 (BL-06) DISPATCH_SETPOINT override, split out of `helpers.rs` to keep
// the `tasks/` file-size cap.

use chrono::{DateTime, Utc};
use std::collections::HashMap;

use crate::controller::SimSnapshot;

/// While a DISPATCH_SETPOINT window is active, steer the battery so net site
/// power hits the commanded setpoint, overriding the plan's battery allocation
/// (other assets keep their planned setpoints; the plan keeps running
/// underneath and resumes when the window ends).
/// Precedence (recorded decision): an active alert window wins - safety over
/// instruction - so the override is skipped entirely while one is active.
pub(crate) fn apply_dispatch_override(
    sp: &mut HashMap<String, f64>,
    sim_snap: &SimSnapshot,
    now: DateTime<Utc>,
    dispatch_windows: &[crate::entities::capacity::DispatchWindow],
    alert_windows: &[crate::entities::capacity::AlertWindow],
    live_pv_kw: Option<f64>,
) {
    let alert_active = alert_windows.iter().any(|a| a.start <= now && now < a.end);
    if alert_active {
        return;
    }
    let Some(win) = dispatch_windows
        .iter()
        .find(|w| w.start <= now && now < w.end)
    else {
        return;
    };
    let Some(bat) = sim_snap.assets.get(crate::ids::ASSET_BATTERY) else {
        return; // no dispatchable actuator - nothing to steer with
    };

    // Net site power without the battery: commanded setpoints for controlled
    // assets, live power for uncontrolled ones. PV prefers `live_pv_kw` (this
    // tick's value from `SimState::peek_pv_kw`) over the snapshot, which holds
    // last tick's output. Uncontrollable assets carry an f64::MAX sentinel
    // default_setpoint_kw that lands in `sp` — any non-finite or absurd
    // magnitude falls back to live power.
    let net_without_battery: f64 = sim_snap
        .assets
        .iter()
        .filter(|(id, _)| id.as_str() != crate::ids::ASSET_BATTERY)
        .map(|(id, snap)| {
            if id.as_str() == crate::ids::ASSET_PV {
                if let Some(pv_kw) = live_pv_kw {
                    return pv_kw;
                }
            }
            sp.get(id)
                .copied()
                .filter(|v| v.is_finite() && v.abs() < 1.0e6)
                .unwrap_or(snap.power_kw)
        })
        .sum();

    // battery > 0 = charging (adds import). Clamp to live capability.
    let wanted_bat_kw = win.setpoint_kw - net_without_battery;
    let clamped = wanted_bat_kw.clamp(bat.cap_max_export_kw, bat.cap_max_import_kw);
    sp.insert(crate::ids::ASSET_BATTERY.to_string(), clamped);
}

#[cfg(test)]
mod dispatch_override_tests {
    use super::*;
    use crate::controller::simulator_port::{AssetSnapshot, GridSnapshot};
    use crate::entities::capacity::{AlertWindow, DispatchWindow};
    use chrono::TimeZone;

    fn ts(secs: i64) -> DateTime<Utc> {
        Utc.timestamp_opt(1_700_000_000 + secs, 0).unwrap()
    }

    fn snap_asset(power_kw: f64, imp: f64, exp: f64) -> AssetSnapshot {
        AssetSnapshot {
            power_kw,
            asset_type: "x".into(),
            cap_max_import_kw: imp,
            cap_max_export_kw: exp,
            available_discharge_kwh: None,
            available_charge_kwh: None,
            default_setpoint_kw: power_kw,
            setpoint_kw: power_kw,
            values: std::collections::HashMap::new(),
        }
    }

    fn make_sim() -> SimSnapshot {
        let mut assets = std::collections::HashMap::new();
        assets.insert("base_load".to_string(), snap_asset(0.5, 0.5, 0.5));
        assets.insert("battery".to_string(), snap_asset(0.0, 5.0, -5.0));
        SimSnapshot {
            ts: ts(0),
            grid: GridSnapshot {
                net_power_w: 500.0,
                voltage_v: 230.0,
                import_kwh: 0.0,
                export_kwh: 0.0,
            },
            assets,
        }
    }

    fn win(setpoint_kw: f64) -> DispatchWindow {
        DispatchWindow {
            setpoint_kw,
            start: ts(0),
            end: ts(600),
            event_id: "disp-1".into(),
        }
    }

    #[test]
    fn test_apply_dispatch_override_steers_battery_to_site_setpoint() {
        let sim = make_sim();
        let mut sp = HashMap::from([("base_load".to_string(), 0.5)]);
        apply_dispatch_override(&mut sp, &sim, ts(60), &[win(2.0)], &[], None);
        // site = base 0.5 + battery -> battery must charge 1.5 kW to hit 2.0.
        assert!((sp["battery"] - 1.5).abs() < 1e-9);
    }

    #[test]
    fn apply_dispatch_override_prefers_live_pv_kw_over_stale_snapshot() {
        let mut sim = make_sim();
        // Stale snapshot: PV read 0.0 last tick; this tick it exports 3.0 kW.
        sim.assets
            .insert("pv".to_string(), snap_asset(0.0, 0.0, 8.0));
        let mut sp = HashMap::from([
            ("base_load".to_string(), 0.5),
            ("pv".to_string(), f64::MAX), // uncontrollable sentinel
        ]);
        apply_dispatch_override(&mut sp, &sim, ts(60), &[win(2.0)], &[], Some(-3.0));
        // net w/o battery = base 0.5 + live PV −3.0 = −2.5 → battery charges 4.5.
        assert!(
            (sp["battery"] - 4.5).abs() < 1e-9,
            "expected battery 4.5 kW using live PV, got {}",
            sp["battery"]
        );
    }

    #[test]
    fn apply_dispatch_override_falls_back_to_snapshot_without_live_pv_kw() {
        let mut sim = make_sim();
        sim.assets
            .insert("pv".to_string(), snap_asset(0.0, 0.0, 8.0));
        let mut sp = HashMap::from([("base_load".to_string(), 0.5), ("pv".to_string(), f64::MAX)]);
        apply_dispatch_override(&mut sp, &sim, ts(60), &[win(2.0)], &[], None);
        // net w/o battery = base 0.5 + stale PV 0.0 → battery charges 1.5.
        assert!((sp["battery"] - 1.5).abs() < 1e-9);
    }

    #[test]
    fn test_apply_dispatch_override_clamps_to_battery_capability() {
        let sim = make_sim();
        let mut sp = HashMap::from([("base_load".to_string(), 0.5)]);
        apply_dispatch_override(&mut sp, &sim, ts(60), &[win(20.0)], &[], None);
        assert!((sp["battery"] - 5.0).abs() < 1e-9, "clamped at max charge");
        apply_dispatch_override(&mut sp, &sim, ts(60), &[win(-20.0)], &[], None);
        assert!(
            (sp["battery"] - (-5.0)).abs() < 1e-9,
            "clamped at max discharge"
        );
    }

    #[test]
    fn test_apply_dispatch_override_alert_wins() {
        let sim = make_sim();
        let mut sp = HashMap::from([("base_load".to_string(), 0.5)]);
        let alert = AlertWindow {
            alert_type: "ALERT_GRID_EMERGENCY".into(),
            start: ts(0),
            end: ts(600),
            event_id: "a1".into(),
            message: String::new(),
        };
        apply_dispatch_override(&mut sp, &sim, ts(60), &[win(2.0)], &[alert], None);
        assert!(
            !sp.contains_key("battery"),
            "override skipped while alert active"
        );
    }

    #[test]
    fn test_apply_dispatch_override_ignores_pv_sentinel_setpoint() {
        // Uncontrollable assets carry an f64::MAX default_setpoint_kw that
        // lands in the setpoint map — the override must fall back to live
        // power for them instead of summing the sentinel (regression: the
        // battery got clamped to full discharge because the wanted power
        // came out -inf).
        let mut sim = make_sim();
        sim.assets
            .insert("pv".to_string(), snap_asset(-2.0, f64::MAX, f64::MAX));
        let mut sp = HashMap::from([("base_load".to_string(), 0.5), ("pv".to_string(), f64::MAX)]);
        apply_dispatch_override(&mut sp, &sim, ts(60), &[win(2.0)], &[], None);
        // net without battery = 0.5 + (-2.0 live PV) = -1.5 -> battery 3.5.
        assert!((sp["battery"] - 3.5).abs() < 1e-9);
    }

    #[test]
    fn test_apply_dispatch_override_inactive_outside_window() {
        let sim = make_sim();
        let mut sp = HashMap::from([("base_load".to_string(), 0.5)]);
        apply_dispatch_override(&mut sp, &sim, ts(700), &[win(2.0)], &[], None);
        assert!(!sp.contains_key("battery"), "window ended - no override");
    }
}
