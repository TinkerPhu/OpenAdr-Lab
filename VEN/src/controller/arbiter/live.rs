//! What the arbiter counts for the physics-driven assets (PV arrays, base loads): this tick's
//! previewed power of each, by asset id, instead of its necessarily-stale snapshot (W5b). The
//! one projection of the site's net power every pass uses.

use std::collections::HashMap;

use super::arbiter_levers::current_setpoint_kw;
use crate::controller::SimSnapshot;

/// This tick's previewed power of every physics-driven asset, by asset id (kW, signed:
/// generation negative).
pub type LiveKwByAsset = HashMap<String, f64>;

/// This tick's projected net site power: each previewed asset at its `live` value, and every
/// controlled asset at what `setpoints` makes it draw.
///
/// For battery/EV, `setpoints` must hold the command the adjustments start
/// from: `reconcile` seeds them with `AssetSnapshot.setpoint_kw` (the
/// arbiter's own last-tick command, the dead-beat integrator state), never the
/// plan's static per-slot allocation. Reading the plan allocation instead would
/// make the deviation signal blind to a correction already applied — the next
/// tick "rediscovers" the same deviation and re-applies a fresh correction on
/// top of it, an unbounded per-tick runaway rather than settling once
/// corrected (see
/// `reconcile_battery_converges_under_stationary_disturbance_not_runaway_to_clamp`).
pub fn projected_net_kw(
    sim: &SimSnapshot,
    setpoints: &HashMap<String, f64>,
    live: &LiveKwByAsset,
) -> f64 {
    projected_net_kw_except(sim, setpoints, live, &[])
}

/// `projected_net_kw` without the assets in `except_asset_ids` — for callers
/// that solve for those assets' own setpoints (dispatch override, the no-plan
/// EV fallback).
pub fn projected_net_kw_except(
    sim: &SimSnapshot,
    setpoints: &HashMap<String, f64>,
    live: &LiveKwByAsset,
    except_asset_ids: &[&str],
) -> f64 {
    sim.assets
        .iter()
        .filter(|(id, _)| !except_asset_ids.contains(&id.as_str()))
        .map(|(id, snap)| {
            // A live preview beats the model for the assets that have one; everything else
            // is the asset's own answer to what this tick's command makes it draw — stage
            // quantization, an all-or-nothing start, a thermostat override and a charger's
            // response lag all included, none of them re-derived here (R-81/R-82).
            match live.get(id) {
                Some(&live_kw) => live_kw,
                None => snap.power_drawn_for_setpoint_kw(current_setpoint_kw(setpoints, sim, id)),
            }
        })
        .sum()
}

/// Site net power once every command in `setpoints` has landed (kW, positive = import): as
/// `projected_net_kw`, but each asset at what its command draws after any response lag
/// (`AssetSnapshot::power_when_command_lands_kw`) instead of what it draws this tick. The two
/// differ only for a lagging asset, whose command issued now is what it draws next tick.
pub fn projected_landed_net_kw(
    sim: &SimSnapshot,
    setpoints: &HashMap<String, f64>,
    live: &LiveKwByAsset,
) -> f64 {
    sim.assets
        .iter()
        .map(|(id, snap)| match live.get(id) {
            Some(&live_kw) => live_kw,
            None => snap.power_when_command_lands_kw(current_setpoint_kw(setpoints, sim, id)),
        })
        .sum()
}

/// `live` with the arrays' generation reduced by `tighten_kw` (kW, magnitude): a tightened
/// site generation limit, shared over the arrays in proportion to what each generates now.
/// The arrays are the assets with an inverter (`AssetSnapshot::ac_ceiling_kw`); a lone array
/// loses the whole `tighten_kw`, never past zero.
pub fn tighten_generation(
    live: &LiveKwByAsset,
    sim: &SimSnapshot,
    tighten_kw: f64,
) -> LiveKwByAsset {
    let is_array = |id: &str| {
        sim.assets
            .get(id)
            .is_some_and(|a| a.ac_ceiling_kw.is_some())
    };
    let generating_kw: f64 = live
        .iter()
        .filter(|(id, _)| is_array(id))
        .map(|(_, kw)| (-kw).max(0.0))
        .sum();
    live.iter()
        .map(|(id, &kw)| {
            let tightened_kw = if is_array(id) && generating_kw > 0.0 {
                let share = (-kw).max(0.0) / generating_kw;
                (kw + tighten_kw * share).min(0.0)
            } else {
                kw
            };
            (id.clone(), tightened_kw)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::controller::sim_snapshot::{AssetSnapshot, GridSnapshot};

    fn snap(ac_ceiling_kw: Option<f64>) -> AssetSnapshot {
        use crate::assets::pv::PvInverter;
        let params = crate::entities::asset_params::PvParams {
            id: "pv".to_string(),
            rated_kw: 10.0,
            inverter_max_kw: 10.0,
            co2_g_kwh: 0.0,
        };
        let pv = PvInverter::from_params(&params);
        let state = crate::assets::AssetState::Pv(PvInverter::initial_state(&params));
        let mut s = crate::services::test_support::asset_snapshots::snapshot_from_asset(
            &pv, state, 0.0, 0.0,
        );
        s.ac_ceiling_kw = ac_ceiling_kw;
        s
    }

    /// W5b: a tightened site limit is shared over the arrays by what each generates; a base
    /// load is untouched.
    #[test]
    fn tighten_generation_shares_the_cut_over_the_arrays() {
        let sim = SimSnapshot {
            ts: chrono::Utc::now(),
            grid: GridSnapshot {
                net_power_kw: 0.0,
                voltage_v: 230.0,
                import_kwh: 0.0,
                export_kwh: 0.0,
            },
            assets: HashMap::from([
                ("pv-a".to_string(), snap(Some(6.0))),
                ("pv-b".to_string(), snap(Some(4.0))),
                ("house".to_string(), snap(None)),
            ]),
        };
        let live = LiveKwByAsset::from([
            ("pv-a".to_string(), -3.0),
            ("pv-b".to_string(), -1.0),
            ("house".to_string(), 0.5),
        ]);
        let tightened = tighten_generation(&live, &sim, 2.0);
        assert!(
            (tightened["pv-a"] - (-1.5)).abs() < 1e-9,
            "3/4 of the 2 kW cut"
        );
        assert!((tightened["pv-b"] - (-0.5)).abs() < 1e-9, "1/4 of it");
        assert_eq!(tightened["house"], 0.5);
    }
}
