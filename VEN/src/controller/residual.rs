/// SITE_RESIDUAL (BL-08, Phase 5 WP5.1) — unmodelled site consumption.
///
/// `residual_kw = grid meter reading (kW) − Σ modelled asset power (kW)`.
/// Exposed as a read-only virtual asset so the planner can budget for
/// background load it cannot otherwise see, and so its history accumulates
/// for BL-14's learned heuristics (Phase 5 WP5.2).
use std::collections::HashMap;

use crate::controller::simulator_port::{AssetSnapshot, SimSnapshot};

/// Asset ID for the site-residual virtual asset.
pub const SITE_RESIDUAL_ASSET_ID: &str = "site-residual";

/// Asset type discriminant for the site-residual virtual asset.
pub const SITE_RESIDUAL_ASSET_TYPE: &str = "site_residual";

/// Compute `residual_kw = grid_kw − Σ modelled_asset_kw` from a raw
/// simulator snapshot. Positive = unmodelled import (extra background
/// load); not clamped, since a negative value (modelled assets exceeding
/// the meter reading) is itself a signal worth surfacing rather than hiding.
///
/// Must be called against the snapshot *before* any synthetic assets
/// (e.g. shiftable-load runtimes) are inserted into `sim.assets`, so a
/// currently-running shiftable load is not folded into "unexplained" load.
pub fn compute_site_residual_kw(sim: &SimSnapshot) -> f64 {
    let grid_kw = sim.grid.net_power_w / 1000.0;
    let modelled_kw: f64 = sim.assets.values().map(|a| a.power_kw).sum();
    grid_kw - modelled_kw
}

/// Build the read-only virtual `AssetSnapshot` for site-residual, ready to
/// insert into `SimSnapshot.assets`. Never controllable: zero import/export
/// capability marks it as a point-reading asset, not a dispatchable one.
pub fn site_residual_snapshot(residual_kw: f64) -> AssetSnapshot {
    AssetSnapshot {
        power_kw: residual_kw,
        asset_type: SITE_RESIDUAL_ASSET_TYPE.into(),
        cap_max_import_kw: 0.0,
        cap_max_export_kw: 0.0,
        available_discharge_kwh: None,
        available_charge_kwh: None,
        default_setpoint_kw: 0.0,
        setpoint_kw: 0.0,
        values: HashMap::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::controller::simulator_port::GridSnapshot;
    use chrono::Utc;

    fn asset(power_kw: f64) -> AssetSnapshot {
        AssetSnapshot {
            power_kw,
            asset_type: "base_load".into(),
            cap_max_import_kw: 0.0,
            cap_max_export_kw: 0.0,
            available_discharge_kwh: None,
            available_charge_kwh: None,
            default_setpoint_kw: 0.0,
            setpoint_kw: 0.0,
            values: HashMap::new(),
        }
    }

    fn sim_snapshot(net_power_w: f64, assets: HashMap<String, AssetSnapshot>) -> SimSnapshot {
        SimSnapshot {
            ts: Utc::now(),
            grid: GridSnapshot {
                net_power_w,
                voltage_v: 230.0,
                import_kwh: 0.0,
                export_kwh: 0.0,
            },
            assets,
        }
    }

    #[test]
    fn compute_site_residual_kw_finds_unmodelled_extra_load() {
        // base_load 1kW + PV -0.5kW modelled => 0.5kW modelled net import.
        // Meter shows an extra 500W unmodelled => 1.0kW net import at the meter.
        let mut assets = HashMap::new();
        assets.insert("base_load".to_string(), asset(1.0));
        assets.insert("pv".to_string(), asset(-0.5));
        let sim = sim_snapshot(1000.0, assets);

        let residual_kw = compute_site_residual_kw(&sim);

        assert!(
            (residual_kw - 0.5).abs() < 1e-9,
            "expected 0.5kW residual, got {residual_kw}"
        );
    }

    #[test]
    fn compute_site_residual_kw_zero_when_fully_modelled() {
        let mut assets = HashMap::new();
        assets.insert("base_load".to_string(), asset(0.8));
        let sim = sim_snapshot(800.0, assets);

        let residual_kw = compute_site_residual_kw(&sim);

        assert!(
            residual_kw.abs() < 1e-9,
            "expected 0kW residual, got {residual_kw}"
        );
    }

    #[test]
    fn compute_site_residual_kw_excludes_running_shiftable_load_when_already_modelled() {
        // A shiftable-load runtime already present in sim.assets (as publish.rs
        // inserts it) must not be double-counted as "unexplained" residual: the
        // meter reading already reflects it, and it's already summed into
        // modelled_kw because it's a normal entry in `sim.assets`.
        let mut assets = HashMap::new();
        assets.insert("base_load".to_string(), asset(0.5));
        assets.insert("washing_machine".to_string(), asset(2.0)); // running shiftable load
        let sim = sim_snapshot(2500.0, assets); // meter matches modelled exactly

        let residual_kw = compute_site_residual_kw(&sim);

        assert!(
            residual_kw.abs() < 1e-9,
            "shiftable load should not appear as residual, got {residual_kw}"
        );
    }

    #[test]
    fn site_residual_snapshot_is_read_only_point_asset() {
        let snap = site_residual_snapshot(0.5);

        assert_eq!(snap.power_kw, 0.5);
        assert_eq!(snap.asset_type, SITE_RESIDUAL_ASSET_TYPE);
        assert_eq!(snap.cap_max_import_kw, 0.0);
        assert_eq!(snap.cap_max_export_kw, 0.0);
        assert_eq!(snap.setpoint_kw, 0.0);
    }
}
