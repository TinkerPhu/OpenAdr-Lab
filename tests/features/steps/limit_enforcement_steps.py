"""Step definitions for arbiter limit enforcement (GB-47)."""

import time

from behave import given, then
from features.helpers.api_client import ven_get, ven_post, ven_put
from features.helpers.wait import poll_until


@given("limit enforcement is {state}")
def step_limit_enforcement(context, state):
    assert state in ("enabled", "disabled"), state
    r = ven_put("/arbiter-settings", json={"limit_enforcement_enabled": state == "enabled"})
    r.raise_for_status()
    assert r.json()["limit_enforcement_enabled"] == (state == "enabled")


@given(
    "the site has no PV, no EV charging, the heater in its band "
    "and the battery at {soc:f} SoC"
)
def step_quiet_site(context, soc):
    # Isolates the base-load step: no PV to offset it (time-of-day
    # independent), no EV charge to shed, no thermostat-forced heat, and
    # enough battery energy to cover the step.
    r = ven_post(
        "/sim/inject",
        json={
            "pv_irradiance": 0.0,
            "ev_plugged": False,
            "heater_temp_c": 20.0,
            "battery_soc": soc,
        },
    )
    r.raise_for_status()


def _net_kw():
    r = ven_get("/sim")
    if not r.ok:
        return None
    return r.json().get("grid", {}).get("net_power_w", 0.0) / 1000.0


@then("the VEN net site power stays at or below {limit_kw:f} kW for {hold_s:d} seconds within {seconds:d} seconds")
def step_net_power_held_below(context, limit_kw, hold_s, seconds):
    # The limit pass acts on the next tick; allow it to settle, then require
    # every sample over `hold_s` to be within the limit.
    deadline = time.time() + seconds
    held_since = None
    last = None
    while time.time() < deadline:
        last = _net_kw()
        if last is not None and last <= limit_kw:
            held_since = held_since or time.time()
            if time.time() - held_since >= hold_s:
                return
        else:
            held_since = None
        time.sleep(1)
    raise AssertionError(
        f"net site power not held ≤ {limit_kw} kW for {hold_s}s within {seconds}s (last {last} kW)"
    )


@then("the VEN net site power exceeds {limit_kw:f} kW within {seconds:d} seconds")
def step_net_power_exceeds(context, limit_kw, seconds):
    poll_until(
        _net_kw,
        lambda net_kw: net_kw is not None and net_kw > limit_kw,
        timeout=seconds,
        interval=2,
        description=f"net site power > {limit_kw} kW",
    )


@then("the arbiter diagnostics show the limit pass holding import under {limit_kw:f} kW with nothing unresolved")
def step_limit_diagnostics(context, limit_kw):
    def fetch():
        r = ven_get("/arbiter-diagnostics")
        return r.json() if r.ok else None

    def holding(diag):
        limit = (diag or {}).get("limit")
        return (
            limit is not None
            and limit["target_kw"] < limit_kw
            and limit["active_lever"] is not None
            and limit["unresolved_kw"] < 0.1  # the arbiter's dead band
        )

    poll_until(fetch, holding, timeout=15, interval=1, description="limit pass diagnostics")


@then('the controller event log has a "{pass_name}" ArbiterDecision')
def step_arbiter_decision_logged(context, pass_name):
    def fetch():
        r = ven_get("/trace/events?limit=500")
        return r.json() if r.ok else []

    def logged(events):
        return any(
            e.get("type") == "ArbiterDecision" and e.get("pass") == pass_name
            for e in events
        )

    poll_until(fetch, logged, timeout=15, interval=1, description=f"{pass_name} ArbiterDecision in /trace/events")
