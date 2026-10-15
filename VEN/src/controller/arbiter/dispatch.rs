//! A VTN dispatch window (WP3.4, BL-06): the site's net power steered to the window's
//! setpoint by the storage levers, ranked like every other pass (W5a), so two batteries
//! share a window the way they share a deviation.

use std::collections::HashMap;

use super::{
    apply_ranked_levers, projected_net_kw, ArbiterTick, LeverInputs, SetpointBoundsKw,
    DISPATCH_POLICY,
};

/// Steer the storage levers so the projected net site power meets `setpoint_kw`. Other
/// assets keep their setpoints; the planner objective does not apply (the VTN's instruction
/// outranks it).
pub fn meet_dispatch_setpoint(
    tick: &ArbiterTick,
    setpoints: &mut HashMap<String, f64>,
    setpoint_kw: f64,
) {
    let net_kw = projected_net_kw(tick.sim, setpoints, tick.live_pv_kw, tick.live_base_load_kw);
    let inputs = LeverInputs {
        tick,
        incumbent_lever: None,
        bounds_kw: &SetpointBoundsKw::new(),
    };
    apply_ranked_levers(setpoints, &DISPATCH_POLICY, &inputs, net_kw - setpoint_kw);
}
