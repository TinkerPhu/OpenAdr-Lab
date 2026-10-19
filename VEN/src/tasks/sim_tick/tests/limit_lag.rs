//! R-151: under a planning objective the import-limit pass holds a lagging lever where its
//! command lands. The EV charger applies a command a tick late, so a pass that judged only this
//! tick's import read a charger it had just cut as "within the limit", let the plan's command
//! through, and the site imported above the limit a tick later.

use super::setpoints_golden::{plan_import_limit_no_bridge, replay};

#[test]
fn build_tick_setpoints_keeps_a_planned_site_within_the_import_limit_once_it_has_acted() {
    let limit_kw = 2.0;
    let mut net_kw_by_tick = Vec::new();
    replay(&plan_import_limit_no_bridge(), |_, _, sim| {
        net_kw_by_tick.push(sim.to_sim_snapshot().grid.net_power_kw);
    });
    // Tick 0 lands what the site drew before the pass had acted on anything.
    let over: Vec<(usize, f64)> = net_kw_by_tick
        .iter()
        .copied()
        .enumerate()
        .skip(1)
        .filter(|(_, kw)| *kw > limit_kw + 1e-6)
        .collect();
    assert!(
        over.is_empty(),
        "site import above the {limit_kw} kW limit at (tick, kW) {over:?}; every tick: {net_kw_by_tick:?}"
    );
}
