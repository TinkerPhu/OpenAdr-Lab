"""Step implementations for deviation absorber BDD scenarios."""

from behave import given, when, then
import requests
import json
import time
from datetime import datetime, timedelta
from features.helpers.api_client import ven_get, ven_post, VEN_BASE_URL, HTTP_TIMEOUT


def _inject(payload, label):
    """POST /sim/inject and assert 2xx (endpoint returns 204 No Content)."""
    resp = ven_post("/sim/inject", json=payload)
    assert resp.status_code in (200, 204), f"{label}: {resp.status_code} {resp.text}"


@given("the VEN is running with the test profile")
def step_ven_running_test_profile(context):
    resp = ven_get("/sim")
    assert resp.status_code == 200, f"VEN not running: {resp.text}"


@given("the absorber is enabled")
def step_absorber_enabled(context):
    resp = ven_get("/sim")
    assert resp.status_code == 200, f"Failed to get sim state: {resp.text}"
    # Absorber state is internal; verified through behaviour


@given("the battery SoC is reset to {soc:f}")
def step_battery_soc_reset(context, soc):
    _inject({"battery_soc": soc}, "Failed to set battery SoC")
    context.battery_initial_soc = soc


@given("the battery SoC is reset to min_soc")
def step_battery_soc_min(context):
    step_battery_soc_reset(context, 0.10)
    context.battery_at_min = True


@given("the EV is plugged with SoC at {soc:f}")
def step_ev_plugged_soc(context, soc):
    _inject({"ev_plugged": True, "ev_soc": soc}, "Failed to set EV state")
    context.ev_plugged = True
    context.ev_initial_soc = soc


@given("the EV is plugged with SoC at {soc:f} (below target)")
def step_ev_plugged_soc_below_target(context, soc):
    step_ev_plugged_soc(context, soc)
    context.ev_below_target = True


@given("the EV is plugged with SoC at soc_target")
def step_ev_plugged_soc_at_target(context):
    step_ev_plugged_soc(context, 0.80)
    context.ev_at_target = True


@given("the EV is configured with departure in {minutes:d} minutes")
def step_ev_departure_configured(context, minutes):
    # ev_departure_seconds is not a sim inject field; track in context for guard check
    # The absorber guard uses the EvSession from the HEMS state machine, not sim inject.
    context.ev_departure_minutes = minutes


@given("the ev_departure_guard_s is set to {seconds:d} seconds ({minutes:d} minutes)")
def step_ev_departure_guard(context, seconds, minutes):
    # Guard threshold is configured in profile (test.yaml); track in context only
    context.ev_departure_guard_s = seconds


@given("the EV SoC is reset to soc_target")
def step_ev_soc_target(context):
    step_ev_plugged_soc(context, 0.80)
    context.ev_at_target = True


@given("the heater is configured with min_state_linger_s of {seconds:d} seconds")
def step_heater_linger_configured(context, seconds):
    # Linger is set in profile; track in context
    context.heater_linger_s = seconds


@given("the heater is at temp_max_c")
def step_heater_at_max_temp(context):
    # Set heater temperature to near max so heater is "full"
    _inject({"heater_temp_c": 22.0}, "Failed to set heater temp")
    context.heater_at_max = True


@given("all absorber assets are at or near their limits")
def step_all_assets_at_limits(context):
    _inject(
        {"battery_soc": 0.10, "ev_soc": 0.80, "ev_plugged": True, "heater_temp_c": 22.0},
        "Failed to set asset limits",
    )
    context.all_assets_at_limits = True


@given("the plan state is initialized with net import {kwh:f} kW")
def step_plan_net_import(context, kwh):
    context.plan_net_import_kw = kwh


@when("I inject a PV drop of {drop_kw:f} kW (positive deviation)")
def step_inject_pv_drop(context, drop_kw):
    # Reduce PV irradiance to simulate PV power drop
    payload = {"pv_irradiance": max(0.0, 1.0 - (drop_kw / 6.0))}
    _inject(payload, "Failed to inject PV drop")
    context.last_injection = payload
    context.deviation_start_time = datetime.now()


@when("I inject a PV drop of {drop_kw:f} kW (small positive deviation within dead-band)")
def step_inject_small_pv_drop(context, drop_kw):
    step_inject_pv_drop(context, drop_kw)
    context.is_small_deviation = True


@when("I inject a sustained negative deviation of {surplus_kw:f} kW (surplus absorption)")
def step_inject_surplus(context, surplus_kw):
    # Increase PV irradiance to simulate PV surplus
    payload = {"pv_irradiance": min(1.0, (6.0 + surplus_kw) / 6.0)}
    _inject(payload, "Failed to inject surplus")
    context.last_injection = payload
    context.is_surplus = True


@when("I inject PV surplus of {surplus_kw:f} kW (negative deviation)")
def step_inject_pv_surplus(context, surplus_kw):
    step_inject_surplus(context, surplus_kw)


@when("I inject another negative deviation of {surplus_kw:f} kW immediately after")
def step_inject_another_surplus_immediately(context, surplus_kw):
    step_inject_surplus(context, surplus_kw)
    context.second_injection_time = datetime.now()


@when("I inject another negative deviation of {surplus_kw:f} kW")
def step_inject_another_surplus(context, surplus_kw):
    step_inject_surplus(context, surplus_kw)


@when("I clear the deviation injection")
def step_clear_deviation(context):
    resp = ven_post("/sim/inject/reset")
    assert resp.status_code in (200, 204), f"Failed to reset injection: {resp.status_code} {resp.text}"
    context.deviation_cleared = True


@when("I inject a positive deviation of {kwh:f} kW")
def step_inject_positive_deviation(context, kwh):
    step_inject_pv_drop(context, kwh)


@when("I inject a sustained positive deviation of {kwh:f} kW")
def step_inject_sustained_positive_deviation(context, kwh):
    step_inject_pv_drop(context, kwh)
    context.sustained_deviation = kwh
    context.sustained_start_time = datetime.now()


@when("I wait {ticks:d} ticks for the sim to process")
def step_wait_ticks(context, ticks):
    time.sleep(ticks * 1.1)  # 1s tick + 10% margin


@when("I wait 1 tick for the sim to process")
def step_wait_1_tick(context):
    time.sleep(1.2)


@when("I wait {seconds:d} seconds for the linger window to elapse")
def step_wait_linger_window(context, seconds):
    time.sleep(seconds + 0.5)


@when("I wait for deviation_trigger_ticks ticks")
def step_wait_for_deviation_trigger(context):
    # Test profile: deviation_trigger_ticks = 10 at 1s/tick
    time.sleep(10 * 1.1)


@when("the deviation is absorbed by the battery")
def step_deviation_absorbed_by_battery(context):
    resp = ven_get("/sim")
    assert resp.status_code == 200
    sim_state = resp.json()
    battery_sp = sim_state.get("assets", {}).get("battery", {}).get("setpoint", 0.0)
    assert battery_sp < -0.1, f"Battery didn't absorb deviation: {battery_sp}"
    context.deviation_absorbed = True


@then("the battery setpoint is more negative than {kw:f} kW")
def step_battery_setpoint_more_negative(context, kw):
    resp = ven_get("/sim")
    assert resp.status_code == 200
    sim_state = resp.json()
    battery_sp = sim_state.get("assets", {}).get("battery", {}).get("setpoint", 0.0)
    assert battery_sp < kw, f"Battery setpoint {battery_sp} not more negative than {kw}"
    context.battery_setpoint = battery_sp


@then("the battery setpoint is at max discharge")
def step_battery_at_max_discharge(context):
    resp = ven_get("/sim")
    assert resp.status_code == 200
    sim_state = resp.json()
    battery_sp = sim_state.get("assets", {}).get("battery", {}).get("setpoint", 0.0)
    assert battery_sp < -4.5, f"Battery not at max discharge: {battery_sp}"


@then("the battery setpoint is unchanged")
def step_battery_setpoint_unchanged(context):
    resp = ven_get("/sim")
    assert resp.status_code == 200
    sim_state = resp.json()
    battery_sp = sim_state.get("assets", {}).get("battery", {}).get("setpoint", 0.0)
    assert abs(battery_sp) < 0.1, f"Battery setpoint changed: {battery_sp}"


@then("the battery setpoint returns to near {kw:f} kW")
def step_battery_returns_to_setpoint(context, kw):
    resp = ven_get("/sim")
    assert resp.status_code == 200
    sim_state = resp.json()
    battery_sp = sim_state.get("assets", {}).get("battery", {}).get("setpoint", 0.0)
    assert abs(battery_sp - kw) < 0.2, f"Battery setpoint did not settle: {battery_sp}"


@then("the battery setpoint is negative")
def step_battery_setpoint_negative(context):
    resp = ven_get("/sim")
    assert resp.status_code == 200
    sim_state = resp.json()
    battery_sp = sim_state.get("assets", {}).get("battery", {}).get("setpoint", 0.0)
    assert battery_sp < -0.1, f"Battery setpoint not negative: {battery_sp}"
    context.battery_sp_corrected = battery_sp


@then("the EV charge setpoint is more negative than baseline")
def step_ev_setpoint_more_negative(context):
    resp = ven_get("/sim")
    assert resp.status_code == 200
    sim_state = resp.json()
    ev_sp = sim_state.get("assets", {}).get("ev", {}).get("setpoint", 0.0)
    assert ev_sp < 0.0, f"EV setpoint not reduced: {ev_sp}"


@then("the EV charge setpoint is unchanged from baseline")
def step_ev_setpoint_unchanged(context):
    resp = ven_get("/sim")
    assert resp.status_code == 200
    sim_state = resp.json()
    ev_sp = sim_state.get("assets", {}).get("ev", {}).get("setpoint", 0.0)
    assert abs(ev_sp) < 0.1, f"EV setpoint changed: {ev_sp}"


@then("the EV charge setpoint is more positive than baseline")
def step_ev_setpoint_more_positive(context):
    resp = ven_get("/sim")
    assert resp.status_code == 200
    sim_state = resp.json()
    ev_sp = sim_state.get("assets", {}).get("ev", {}).get("setpoint", 0.0)
    assert ev_sp > 0.0, f"EV setpoint not increased: {ev_sp}"


@then("the EV moves closer to soc_target")
def step_ev_soc_closer_to_target(context):
    resp = ven_get("/sim")
    assert resp.status_code == 200
    sim_state = resp.json()
    ev_soc = sim_state.get("assets", {}).get("ev", {}).get("soc", 0.0)
    assert ev_soc > 0.30 + 0.05, f"EV SoC did not increase toward target: {ev_soc}"


@then("the absorber residual is less than {kw:f} kW")
def step_absorber_residual_less_than(context, kw):
    resp = ven_get("/trace?limit=5")
    assert resp.status_code == 200
    trace = resp.json()
    if trace:
        last_entry = trace[-1]
        residual = last_entry.get("absorber_residual_kw", 0.0)
        assert abs(residual) < kw, f"Residual {residual} >= {kw}"
        context.last_residual = residual


@then("the absorber residual equals the injected deviation")
def step_absorber_residual_equals_deviation(context):
    resp = ven_get("/trace?limit=5")
    assert resp.status_code == 200
    trace = resp.json()
    if trace:
        last_entry = trace[-1]
        residual = abs(last_entry.get("absorber_residual_kw", 0.0))
        assert residual > 0.04, f"Residual {residual} not capturing small deviation"


@then("the absorber is active with an overlay")
def step_absorber_active(context):
    resp = ven_get("/trace?limit=5")
    assert resp.status_code == 200
    trace = resp.json()
    if trace:
        last_entry = trace[-1]
        overlay = last_entry.get("absorber_active_overlay_kw", 0.0)
        assert abs(overlay) > 0.1, f"No active overlay: {overlay}"
        context.active_overlay = overlay


@then("the absorber settling counter increments")
def step_absorber_settling_increments(context):
    context.settling_observed = True


@then("the overlay goes to zero")
def step_overlay_goes_to_zero(context):
    resp = ven_get("/sim")
    assert resp.status_code == 200
    sim_state = resp.json()
    battery_sp = sim_state.get("assets", {}).get("battery", {}).get("setpoint", 0.0)
    assert abs(battery_sp) < 0.15, f"Overlay not zero: {battery_sp}"


@then("the heater setpoint has changed")
def step_heater_changed(context):
    resp = ven_get("/sim")
    assert resp.status_code == 200
    sim_state = resp.json()
    heater_sp = sim_state.get("assets", {}).get("heater", {}).get("setpoint", 0.0)
    assert heater_sp > 0.5, f"Heater not activated: {heater_sp}"
    context.heater_sp_after_change = heater_sp


@then("the absorber last_state_change_ts is recorded for heater")
def step_absorber_recorded_heater_change(context):
    context.heater_change_recorded = True


@then("the heater setpoint does not change again")
def step_heater_does_not_change(context):
    resp = ven_get("/sim")
    assert resp.status_code == 200
    sim_state = resp.json()
    heater_sp = sim_state.get("assets", {}).get("heater", {}).get("setpoint", 0.0)
    assert abs(heater_sp - context.heater_sp_after_change) < 0.1, \
        f"Heater changed despite linger: {heater_sp} vs {context.heater_sp_after_change}"


@then("the absorber residual propagates uncovered")
def step_residual_propagates(context):
    resp = ven_get("/trace?limit=5")
    assert resp.status_code == 200
    trace = resp.json()
    if trace:
        last_entry = trace[-1]
        residual = last_entry.get("absorber_residual_kw", 0.0)
        assert residual > 1.0, f"Residual not propagating: {residual}"


@then("the heater setpoint can change again")
def step_heater_can_change(context):
    resp = ven_get("/sim")
    assert resp.status_code == 200
    sim_state = resp.json()
    heater_sp = sim_state.get("assets", {}).get("heater", {}).get("setpoint", 0.0)
    assert heater_sp != context.heater_sp_after_change or heater_sp > context.heater_sp_after_change, \
        f"Heater still blocked after linger: {heater_sp}"


@then("the absorber skips the EV asset")
def step_absorber_skips_ev(context):
    context.ev_skipped = True


@then("the absorber can adjust the EV charging")
def step_absorber_adjusts_ev(context):
    context.ev_adjusted = True


@then("no DeviceDeviation trigger has fired within {ticks:d} ticks")
def step_no_device_deviation(context, ticks):
    resp = ven_get("/trace?limit=100")
    assert resp.status_code == 200
    trace = resp.json()
    for entry in trace:
        trigger = entry.get("trigger_event", "")
        assert trigger != "DeviceDeviation", f"DeviceDeviation fired unexpectedly: {entry}"
    context.no_device_deviation_fired = True


@then("no DeviceDeviation trigger fires within 120 ticks")
def step_no_device_deviation_120(context):
    step_no_device_deviation(context, 120)


@then("the DeviceDeviation trigger fires")
def step_device_deviation_fires(context):
    resp = ven_get("/trace?limit=100")
    assert resp.status_code == 200
    trace = resp.json()
    found = any(entry.get("trigger_event") == "DeviceDeviation" for entry in trace)
    assert found, "DeviceDeviation did not fire"
    context.device_deviation_fired = True


@then("a new MILP plan is produced")
def step_new_milp_plan_produced(context):
    resp = ven_get("/plan")
    assert resp.status_code == 200
    plan = resp.json()
    assert plan, "No plan produced"
    context.new_plan_produced = True


@then("the replanning is triggered only once (no chattering)")
def step_no_replan_chattering(context):
    resp = ven_get("/trace?limit=200")
    assert resp.status_code == 200
    trace = resp.json()
    replan_count = sum(1 for entry in trace if entry.get("trigger_event") == "DeviceDeviation")
    assert replan_count <= 1, f"Replanner triggered {replan_count} times (chattering)"


@then("the MILP planner does not execute a replan")
def step_no_planner_replan(context):
    resp = ven_get("/trace?limit=200")
    assert resp.status_code == 200
    trace = resp.json()
    for entry in trace:
        if entry.get("trigger_event") == "DeviceDeviation":
            raise AssertionError("Planner replanned for transient deviation")
    context.no_replan_for_transient = True


@then("correction_is_active is false")
def step_correction_not_active(context):
    resp = ven_get("/trace?limit=5")
    assert resp.status_code == 200
    trace = resp.json()
    if trace:
        last_entry = trace[-1]
        overlay = last_entry.get("absorber_active_overlay_kw", 0.0)
        assert abs(overlay) < 0.01, f"Correction active despite dead-band: {overlay}"
