//! A storage asset's one-tick bridge for a lagging lever (R-82, R-104).
//!
//! The EV charger applies a command a tick late (`response_delay_s`), so the lever loop
//! hands what the charger has not delivered yet to the next lever, usually the battery,
//! for that tick: import never crosses a hard limit while the cut lands. That share is
//! a bridge, not a correction. The battery carries its own last command forward as its
//! baseline, so a bridge carried with it reads, once the charger's command has landed,
//! as a deviation of its own, and the arbiter undid the EV's landed change to remove it:
//! the EV/battery hunt of R-104. So each pass reports its bridge, the tick hands the
//! total back, and `reconcile` takes it out of the carried battery command first.

use std::collections::HashMap;

use super::arbiter_levers::LeverEffect;

/// Accounts one pass of the lever loop: what a lagging lever left undelivered, and each
/// storage asset's share covering it.
#[derive(Debug, Default)]
pub(super) struct LagBridge {
    /// Commanded by a lagging lever but not delivered this tick (kW, magnitude).
    lagging_kw: f64,
    /// Each storage asset's bridging share (kW, setpoint sign).
    bridge_kw_by_asset: HashMap<String, f64>,
}

impl LagBridge {
    /// The EV lever moved: its undelivered part waits for a bridge. Returns what it
    /// achieved this tick.
    pub(super) fn ev_moved(&mut self, effect: LeverEffect) -> f64 {
        self.lagging_kw += (effect.landed_kw - effect.now_kw).max(0.0);
        effect.now_kw
    }

    /// The storage lever of `asset_id` achieved `kw` (magnitude) against `deviation_kw`: up
    /// to what is still undelivered, that is bridge. Returns `kw`.
    pub(super) fn battery_moved(&mut self, asset_id: &str, kw: f64, deviation_kw: f64) -> f64 {
        let bridge_kw = kw.min(self.lagging_kw);
        if bridge_kw > 0.0 {
            self.lagging_kw -= bridge_kw;
            // Shedding import discharges (setpoint down), absorbing a surplus charges.
            *self
                .bridge_kw_by_asset
                .entry(asset_id.to_string())
                .or_insert(0.0) -= deviation_kw.signum() * bridge_kw;
        }
        kw
    }

    pub(super) fn into_bridge_kw_by_asset(self) -> HashMap<String, f64> {
        self.bridge_kw_by_asset
    }
}

/// The carried storage commands without last tick's bridges: the command that bridged
/// has landed since, so what remains is the correction proper.
pub(super) fn drop_last_bridge(
    setpoints: &mut HashMap<String, f64>,
    prev_bridge_kw_by_asset: &HashMap<String, f64>,
) {
    for (asset_id, bridge_kw) in prev_bridge_kw_by_asset {
        if let Some(setpoint_kw) = setpoints.get_mut(asset_id) {
            *setpoint_kw -= bridge_kw;
        }
    }
}
