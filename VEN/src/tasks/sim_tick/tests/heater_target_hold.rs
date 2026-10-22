//! R-150: the sim's "Comfort target" inject (`heater_setpoint_c`) holds the target without
//! chattering the relay. A bare on/off at the target switched the relay every 10-15 s; the
//! heater's own ceiling latch holds it open for a full `thermostat_delta_c` below the cut-out.

use chrono::Duration;

use super::setpoints_golden::{context, plan_with, sim, t0, Scenario};
use crate::simulator::TickInputs;

const TICKS: i64 = 600;
const TARGET_C: f64 = 21.5;

/// Relay switches (on to off or off to on) of the heater over `TICKS` one-second ticks, from a
/// tank 0.1 degrees under the target, with a plan that allocates the heater nothing.
fn relay_switches_holding_target() -> usize {
    let mut sim = sim();
    let mut scenario = Scenario::new("heater_target_hold", Some(plan_with(&[])));
    scenario.heater_setpoint_c = Some(TARGET_C);
    let mut ctx = context(&scenario, &sim);
    ctx.inject.heater_temp_c = Some(TARGET_C - 0.1);
    crate::simulator::inject::apply_state_injections(&ctx.inject, &mut sim);
    ctx.inject.heater_temp_c = None;
    let mut switches = 0;
    let mut was_on: Option<bool> = None;
    for tick in 0..TICKS {
        let now = t0() + Duration::seconds(tick);
        let snap = sim.to_sim_snapshot();
        let (live, released) = super::feeds::live_previews(&sim, &ctx, &snap, now, 1.0);
        let thermostat = sim.thermostat_setpoints_kw(ctx.inject.heater_setpoint_c);
        let o = super::setpoints::build_tick_setpoints(
            &ctx,
            &snap,
            &thermostat,
            now,
            (&live, &released),
        );
        sim.tick(TickInputs {
            heater_target_c_override: ctx.inject.heater_setpoint_c,
            ..TickInputs::new(1.0, now, o.setpoints.clone())
        });
        let on = sim.to_sim_snapshot().assets["heater"].power_kw > 0.0;
        if was_on.is_some_and(|w| w != on) {
            switches += 1;
        }
        was_on = Some(on);
    }
    switches
}

#[test]
fn a_comfort_target_inside_the_band_switches_the_relay_once_not_every_few_seconds() {
    let switches = relay_switches_holding_target();
    assert!(
        switches <= 1,
        "the relay switched {switches} times in {TICKS} s"
    );
}
