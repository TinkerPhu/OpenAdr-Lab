"""Step definitions for arbiter limit enforcement (GB-47)."""

import time

from behave import given, then, when
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


@when("I release the sustained base-load step")
def step_release_base_load_step(context):
    # With alpha 1.0 the offset decays to zero on the next tick — a clean
    # release, unlike forcing 0 kW (which leaves a slowly decaying negative offset).
    r = ven_post("/sim/inject", json={"base_load_kw": 0.5, "base_load_alpha": 1.0})
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


def _wait_for_limit_diagnostics(limit_kw, lever_required):
    def fetch():
        r = ven_get("/arbiter-diagnostics")
        return r.json() if r.ok else None

    def steering(diag):
        limit = (diag or {}).get("limit")
        return (
            limit is not None
            and limit["target_kw"] < limit_kw
            and (limit["active_lever"] is not None or not lever_required)
            and limit["unresolved_kw"] < 0.1  # the arbiter's dead band
        )

    poll_until(fetch, steering, timeout=15, interval=1, description="limit pass diagnostics")


@then("the arbiter diagnostics show the limit pass steering under {limit_kw:f} kW with nothing unresolved")
def step_limit_diagnostics(context, limit_kw):
    # Whether a lever is needed depends on the plan: a plan that already
    # accounts for the limit leaves the pass nothing to shed.
    _wait_for_limit_diagnostics(limit_kw, lever_required=False)


@then("the arbiter diagnostics show the limit pass steering under {limit_kw:f} kW with a lever and nothing unresolved")
def step_limit_diagnostics_with_lever(context, limit_kw):
    _wait_for_limit_diagnostics(limit_kw, lever_required=True)


@given("the simulator imposes an import limit of {limit_kw:f} kW")
def step_sim_import_limit(context, limit_kw):
    # `grid_import_limit_kw` stands in for a capacity limit at execution only
    # (the planner never reads it), so the plan keeps violating it.
    r = ven_post("/sim/inject", json={"grid_import_limit_kw": limit_kw})
    r.raise_for_status()


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
