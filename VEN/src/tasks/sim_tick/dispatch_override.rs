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
    // magnitude falls back to live power. The heater prefers
    // `predict_heater_forced_kw` over its commanded setpoint whenever its own
    // thermostat hysteresis/safety cutoff will override that setpoint this tick
    // (same "commanded ≠ actual" gap PV's live_pv_kw closes — see that function's
    // doc comment for the E2E failure this fixes).
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
            if id.as_str() == crate::ids::ASSET_HEATER {
                if let Some(forced_kw) =
                    crate::controller::dispatcher::predict_heater_forced_kw(snap)
                {
                    return forced_kw;
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

/// R-59 fail-safe: once VTN comms-loss has been confirmed for the profile's
/// configured debounce window, cap every controllable asset's setpoint to
/// `max_power_pct` of its own ceiling — symmetric for the battery (both
/// charge and discharge capped, no special-casing), one generic knob for all
/// assets rather than one fail-safe per asset type. Runs last in the tick
/// pipeline (after `apply_dispatch_override`) because a comms-loss fail-safe
/// outranks a VTN-instructed dispatch window: if the VTN can't be reached,
/// any window it set is stale/unverifiable. No-op when `comms_loss` is `None`
/// (profile opt-out) or not yet `active` (debounce not elapsed).
pub(crate) fn apply_comms_loss_clamp(
    sp: &mut HashMap<String, f64>,
    sim_snap: &SimSnapshot,
    comms_loss: Option<super::context::CommsLossState>,
) {
    let Some(cl) = comms_loss.filter(|c| c.active) else {
        return;
    };
    let pct = cl.max_power_pct;

    for (asset_id, import_key, export_key) in [
        (crate::ids::ASSET_EV, "max_charge_kw", None),
        (crate::ids::ASSET_HEATER, "max_kw", None),
        (
            crate::ids::ASSET_BATTERY,
            "max_charge_kw",
            Some("max_discharge_kw"),
        ),
    ] {
        let Some(snap) = sim_snap.assets.get(asset_id) else {
            continue;
        };
        let Some(&sp_val) = sp.get(asset_id) else {
            continue;
        };
        let max_charge = snap.val(import_key).unwrap_or(f64::MAX);
        let max_discharge = export_key.and_then(|k| snap.val(k)).unwrap_or(0.0);
        // Sign convention matches this file's existing battery clamp above:
        // positive = charge/import, negative = discharge/export.
        let clamped = sp_val.clamp(-(pct * max_discharge), pct * max_charge);
        sp.insert(asset_id.to_string(), clamped);
    }
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
                import_limit_kw: f64::MAX,
                export_limit_kw: -f64::MAX,
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
    fn test_apply_dispatch_override_accounts_for_heater_forced_on() {
        // Regression: WP3.4 DISPATCH_SETPOINT E2E scenario failed because the
        // heater was mid-emergency-hysteresis (drawing max_kw physically) while
        // its commanded setpoint read 0 — the override's net_without_battery
        // calc trusted the commanded 0 and undershot the battery correction by
        // exactly the heater's forced power (see
        // `dispatcher::predict_heater_forced_kw`'s doc comment for the full story).
        let mut sim = make_sim();
        let mut heater_values = std::collections::HashMap::new();
        heater_values.insert("temp_c".to_string(), 20.0);
        heater_values.insert("max_kw".to_string(), 3.0);
        heater_values.insert("temp_min_c".to_string(), 18.0);
        heater_values.insert("temp_max_c".to_string(), 23.0);
        heater_values.insert("temp_safety_max_c".to_string(), 23.0);
        sim.assets.insert(
            "heater".to_string(),
            AssetSnapshot {
                power_kw: 3.0, // forced on last tick, still within hysteresis window
                asset_type: "heater".into(),
                cap_max_import_kw: 3.0,
                cap_max_export_kw: 0.0,
                available_discharge_kwh: None,
                available_charge_kwh: None,
                default_setpoint_kw: 0.0,
                setpoint_kw: 0.0,
                values: heater_values,
            },
        );
        let mut sp = HashMap::from([
            ("base_load".to_string(), 0.5),
            ("heater".to_string(), 0.0), // dispatcher committed 0; hysteresis overrides it
        ]);
        apply_dispatch_override(&mut sp, &sim, ts(60), &[win(2.0)], &[], None);
        // net w/o battery = base 0.5 + forced heater 3.0 = 3.5 -> battery must
        // discharge 1.5 kW (charge = -1.5) to still hit the 2.0 kW site target.
        assert!(
            (sp["battery"] - (-1.5)).abs() < 1e-9,
            "expected battery -1.5 kW accounting for the heater's forced 3.0 kW, got {}",
            sp["battery"]
        );
    }

    #[test]
    fn test_apply_dispatch_override_inactive_outside_window() {
        let sim = make_sim();
        let mut sp = HashMap::from([("base_load".to_string(), 0.5)]);
        apply_dispatch_override(&mut sp, &sim, ts(700), &[win(2.0)], &[], None);
        assert!(!sp.contains_key("battery"), "window ended - no override");
    }

    // ── apply_comms_loss_clamp (R-59) ────────────────────────────────────────

    fn snap_asset_with_values(power_kw: f64, values: &[(&str, f64)]) -> AssetSnapshot {
        let mut snap = snap_asset(power_kw, f64::MAX, -f64::MAX);
        snap.values = values.iter().map(|(k, v)| (k.to_string(), *v)).collect();
        snap
    }

    fn comms_loss(
        active: bool,
        max_power_pct: f64,
    ) -> Option<super::super::context::CommsLossState> {
        Some(super::super::context::CommsLossState {
            active,
            max_power_pct,
        })
    }

    #[test]
    fn apply_comms_loss_clamp_noop_when_comms_loss_is_none() {
        let mut sim = make_sim();
        sim.assets.insert(
            "ev".to_string(),
            snap_asset_with_values(10.0, &[("max_charge_kw", 7.4)]),
        );
        let mut sp = HashMap::from([("ev".to_string(), 7.4)]);
        apply_comms_loss_clamp(&mut sp, &sim, None);
        assert_eq!(sp["ev"], 7.4, "no comms_loss config -> untouched");
    }

    #[test]
    fn apply_comms_loss_clamp_noop_when_not_yet_active() {
        let mut sim = make_sim();
        sim.assets.insert(
            "ev".to_string(),
            snap_asset_with_values(10.0, &[("max_charge_kw", 7.4)]),
        );
        let mut sp = HashMap::from([("ev".to_string(), 7.4)]);
        apply_comms_loss_clamp(&mut sp, &sim, comms_loss(false, 0.7));
        assert_eq!(sp["ev"], 7.4, "debounce not elapsed -> untouched");
    }

    #[test]
    fn apply_comms_loss_clamp_caps_ev_charge_to_pct_of_max_charge_kw() {
        let mut sim = make_sim();
        sim.assets.insert(
            "ev".to_string(),
            snap_asset_with_values(10.0, &[("max_charge_kw", 7.4)]),
        );
        let mut sp = HashMap::from([("ev".to_string(), 7.4)]);
        apply_comms_loss_clamp(&mut sp, &sim, comms_loss(true, 0.5));
        assert!((sp["ev"] - 3.7).abs() < 1e-9, "got {}", sp["ev"]);
    }

    #[test]
    fn apply_comms_loss_clamp_caps_heater_to_pct_of_max_kw() {
        let mut sim = make_sim();
        sim.assets.insert(
            "heater".to_string(),
            snap_asset_with_values(6.0, &[("max_kw", 6.0)]),
        );
        let mut sp = HashMap::from([("heater".to_string(), 6.0)]);
        apply_comms_loss_clamp(&mut sp, &sim, comms_loss(true, 0.7));
        assert!((sp["heater"] - 4.2).abs() < 1e-9, "got {}", sp["heater"]);
    }

    #[test]
    fn apply_comms_loss_clamp_caps_battery_charge_and_discharge_symmetrically() {
        let mut sim = make_sim(); // already has "battery" with cap_max_import/export, override values
        sim.assets.insert(
            "battery".to_string(),
            snap_asset_with_values(0.0, &[("max_charge_kw", 5.0), ("max_discharge_kw", 5.0)]),
        );
        let mut sp = HashMap::from([("battery".to_string(), 5.0)]);
        apply_comms_loss_clamp(&mut sp, &sim, comms_loss(true, 0.6));
        assert!(
            (sp["battery"] - 3.0).abs() < 1e-9,
            "charge got {}",
            sp["battery"]
        );

        sp.insert("battery".to_string(), -5.0);
        apply_comms_loss_clamp(&mut sp, &sim, comms_loss(true, 0.6));
        assert!(
            (sp["battery"] - (-3.0)).abs() < 1e-9,
            "discharge got {}",
            sp["battery"]
        );
    }

    #[test]
    fn apply_comms_loss_clamp_leaves_setpoint_untouched_when_already_under_cap() {
        let mut sim = make_sim();
        sim.assets.insert(
            "ev".to_string(),
            snap_asset_with_values(10.0, &[("max_charge_kw", 7.4)]),
        );
        let mut sp = HashMap::from([("ev".to_string(), 1.0)]);
        apply_comms_loss_clamp(&mut sp, &sim, comms_loss(true, 0.7));
        assert!((sp["ev"] - 1.0).abs() < 1e-9, "got {}", sp["ev"]);
    }

    #[test]
    fn apply_comms_loss_clamp_ignores_assets_missing_from_setpoints_map() {
        let mut sim = make_sim();
        sim.assets.insert(
            "ev".to_string(),
            snap_asset_with_values(10.0, &[("max_charge_kw", 7.4)]),
        );
        let mut sp = HashMap::new(); // ev has no setpoint entry this tick
        apply_comms_loss_clamp(&mut sp, &sim, comms_loss(true, 0.5));
        assert!(!sp.contains_key("ev"));
    }
}
