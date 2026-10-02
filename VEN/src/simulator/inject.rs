//! Behaviour-A one-shot state injections: forcing an asset's stored state to
//! a value, rather than nudging its environment (that is `TickInputs`).
//!
//! Lived in `tasks/sim_tick/helpers.rs`, which is the adapter ring — a module
//! whose job is "run this on a timer". Writing an asset's state into a
//! `SimState` is a simulator operation, so it belongs here, and moving it is
//! also what gives that file room under the `tasks/` size cap instead of yet
//! another split.

use std::collections::HashMap;

use crate::entities::sim_inject::SimInjectState;
use crate::simulator::SimState;

/// One injectable state field: its wire name, the asset it belongs to, what
/// that asset calls it in `Asset::reset`, and how to read it.
///
/// Declared as data, including the accessor, so a new injectable state field
/// is one row rather than another near-identical `if let` block
/// (`declare-dont-branch`). The three that existed differed only in these
/// four things.
struct StateInjection {
    field: &'static str,
    asset_id: &'static str,
    state_key: &'static str,
    read: fn(&SimInjectState) -> Option<f64>,
}

const STATE_INJECTIONS: [StateInjection; 3] = [
    StateInjection {
        field: "battery_soc",
        asset_id: crate::ids::ASSET_BATTERY,
        state_key: "soc",
        read: |i| i.battery_soc,
    },
    StateInjection {
        field: "ev_soc",
        asset_id: crate::ids::ASSET_EV,
        state_key: "soc",
        read: |i| i.ev_soc,
    },
    StateInjection {
        field: "heater_temp_c",
        asset_id: crate::ids::ASSET_HEATER,
        state_key: "temp_c",
        read: |i| i.heater_temp_c,
    },
];

/// Apply every pending one-shot state injection, returning the inject-field
/// names that were applied and so must be cleared after the lock is released.
pub fn apply_state_injections(inject: &SimInjectState, sim: &mut SimState) -> Vec<&'static str> {
    let mut cleared = Vec::new();
    for StateInjection {
        field,
        asset_id,
        state_key,
        read,
    } in STATE_INJECTIONS
    {
        let Some(value) = read(inject) else {
            continue;
        };
        if let Some((entry, cfg)) = sim.find_asset_mut(asset_id) {
            cfg.reset(
                &mut entry.state,
                HashMap::from([(state_key.to_string(), value)]),
            );
        }
        // Pushed even when the asset is absent, matching the previous
        // behaviour: a one-shot inject is consumed by the tick that saw it,
        // whether or not there was an asset to apply it to. Leaving it set
        // would re-apply it on every later tick.
        cleared.push(field);
    }
    cleared
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assets::AssetState;
    use crate::entities::asset_params::{AssetParams, BatteryParams};

    fn battery_sim() -> SimState {
        SimState::from_params(
            &[AssetParams::Battery(BatteryParams {
                id: crate::ids::ASSET_BATTERY.to_string(),
                initial_soc: 0.5,
                ..BatteryParams::default()
            })],
            chrono::Utc::now(),
        )
    }

    #[test]
    fn applies_a_battery_soc_injection_and_reports_the_field() {
        let mut sim = battery_sim();
        let inject = SimInjectState {
            battery_soc: Some(0.8),
            ..SimInjectState::default()
        };

        let cleared = apply_state_injections(&inject, &mut sim);

        assert_eq!(cleared, vec!["battery_soc"]);
        let (entry, _) = sim.find_asset(crate::ids::ASSET_BATTERY).unwrap();
        match &entry.state {
            AssetState::Battery(s) => assert!((s.soc - 0.8).abs() < 1e-9),
            other => panic!("expected Battery state, got {other:?}"),
        }
    }

    #[test]
    fn reports_a_field_whose_asset_is_absent_so_it_is_still_consumed() {
        let mut sim = battery_sim();
        let inject = SimInjectState {
            heater_temp_c: Some(60.0),
            ..SimInjectState::default()
        };

        // No heater in this roster; the inject must still be cleared, or it
        // would re-apply on every subsequent tick.
        assert_eq!(
            apply_state_injections(&inject, &mut sim),
            vec!["heater_temp_c"]
        );
    }

    #[test]
    fn applies_nothing_and_reports_nothing_when_no_injection_is_pending() {
        let mut sim = battery_sim();
        assert!(apply_state_injections(&SimInjectState::default(), &mut sim).is_empty());
    }

    #[test]
    fn applies_every_pending_injection_in_one_pass() {
        let mut sim = battery_sim();
        let inject = SimInjectState {
            battery_soc: Some(0.3),
            ev_soc: Some(0.4),
            heater_temp_c: Some(55.0),
            ..SimInjectState::default()
        };
        assert_eq!(
            apply_state_injections(&inject, &mut sim),
            vec!["battery_soc", "ev_soc", "heater_temp_c"]
        );
    }
}
