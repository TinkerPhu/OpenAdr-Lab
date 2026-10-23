"""Step definitions for the opportunistic objective (openspec `opportunistic-objective`, UC-19).

The objective is changed the way the VEN UI changes it (`PUT /plan/objective`, restored to the
profile's after each scenario by `helpers/scenario_state.py`) and read back from
`GET /plan/objective`; what the VEN then does is read from `/sim` (asset and site power)
and `/plan` (the adopted plan's objective and status).
"""

import time

from behave import given, then, when
from features.helpers.api_client import ven_get, ven_post, ven_put
from features.helpers.wait import poll_until


def _set_objective(objective):
    r = ven_put("/plan/objective", json={"objective": objective})
    assert r.status_code == 204, f"PUT /plan/objective {objective}: {r.status_code} {r.text}"


@given('I set the VEN\'s objective to "{objective}"')
@when('I set the VEN\'s objective to "{objective}"')
def step_set_objective(context, objective):
    _set_objective(objective)


@given('the VEN runs under the "{objective}" objective')
def step_runs_under_objective(context, objective):
    # A tick dispatches by the adopted plan's objective, not by the one just set: the switch
    # takes effect once a replan under it is adopted, which on a loaded host took over 30 s.
    _set_objective(objective)
    poll_until(
        _plan,
        lambda p: p and p.get("objective") == objective,
        timeout=120,
        interval=2,
        description=f"a plan under {objective} adopted",
    )


@then('GET /plan/objective reports "{objective}" set by "{source}"')
def step_objective_reported(context, objective, source):
    body = ven_get("/plan/objective").json()
    assert (body["objective"], body["source"]) == (objective, source), body
    assert body["set_at"], "a change says when it was made"
    assert objective in body["available_objectives"], body


def _plan():
    r = ven_get("/plan")
    return r.json() if r.ok else None


@then('within {seconds:d} seconds the VEN adopts a "{status}" plan under "{objective}" with slots')
@given('within {seconds:d} seconds the VEN adopts a "{status}" plan under "{objective}" with slots')
def step_adopts_plan_with_status(context, seconds, status, objective):
    poll_until(
        _plan,
        lambda p: p
        and p.get("objective") == objective
        and p.get("solve_status") == status
        and len(p.get("slots", [])) > 0,
        timeout=seconds,
        interval=2,
        description=f"a {status} plan under {objective} with slots",
    )


@then('within {seconds:d} seconds the VEN adopts a solved plan under "{objective}"')
def step_adopts_solved_plan(context, seconds, objective):
    poll_until(
        _plan,
        lambda p: p
        and p.get("objective") == objective
        and p.get("solve_status") in ("OPTIMAL", "TIME_LIMIT", "GAP_LIMIT"),
        timeout=seconds,
        interval=2,
        description=f"a solved plan under {objective}",
    )


@given(
    "the site has no PV, the EV plugged at {soc:g} SoC, the heater in its band "
    "and the battery at {battery_soc:g} SoC"
)
def step_site_with_plugged_ev(context, soc, battery_soc):
    # As the limit feature's quiet site, but with the EV plugged in and the battery near its
    # floor, so an EV draw shows at the meter instead of being covered by storage.
    r = ven_post(
        "/sim/inject",
        json={
            "pv_irradiance": 0.0,
            "ev_plugged": True,
            "ev_soc": soc,
            "heater_temp_c": 20.0,
            "battery_soc": battery_soc,
        },
    )
    r.raise_for_status()


@given("the site has no EV charging, the heater in its band and the battery at {battery_soc:g} SoC")
def step_site_pv_from_measurement(context, battery_soc):
    # As the limit feature's quiet site, without its PV inject: the scenario's PV is the
    # measurement it publishes, and an irradiance inject's decaying offset would add to that
    # (an offset an earlier scenario left is flushed by the step before this one).
    r = ven_post(
        "/sim/inject",
        json={"ev_plugged": False, "heater_temp_c": 20.0, "battery_soc": battery_soc},
    )
    r.raise_for_status()


def _asset_kw(asset_id):
    r = ven_get("/sim")
    if not r.ok:
        return None
    return r.json()["assets"][asset_id]["power_kw"]


@then('within {seconds:d} seconds the "{first}" and the "{second}" together draw at least {kw:g} kW')
def step_assets_draw_together(context, seconds, first, second, kw):
    poll_until(
        lambda: (_asset_kw(first), _asset_kw(second)),
        lambda p: None not in p and p[0] + p[1] >= kw,
        timeout=seconds,
        interval=1,
        description=f"{first} and {second} drawing at least {kw} kW together",
    )


@then('within {seconds:d} seconds the "{asset_id}" draws at least {kw:g} kW')
def step_asset_draws_at_least(context, seconds, asset_id, kw):
    poll_until(
        lambda: _asset_kw(asset_id),
        lambda p: p is not None and p >= kw,
        timeout=seconds,
        interval=1,
        description=f"{asset_id} drawing at least {kw} kW",
    )


@then('the "{asset_id}" stays at or below {kw:g} kW for {hold_s:d} seconds within {seconds:d} seconds')
def step_asset_held_at_most(context, asset_id, kw, hold_s, seconds):
    # A charger applies a command a tick late, so whatever the previous scenario left pending
    # lands first; the claim is that it settles there and stays.
    deadline = time.time() + seconds
    held_since = None
    last = None
    while time.time() < deadline:
        last = _asset_kw(asset_id)
        if last is not None and last <= kw:
            held_since = held_since or time.time()
            if time.time() - held_since >= hold_s:
                return
        else:
            held_since = None
        time.sleep(1)
    raise AssertionError(
        f"{asset_id} not held at or below {kw} kW for {hold_s}s within {seconds}s (last {last} kW)"
    )


@then(
    "the VEN net site power stays between {low_kw:g} and {high_kw:g} kW "
    "for {hold_s:d} seconds within {seconds:d} seconds"
)
def step_net_power_held_between(context, low_kw, high_kw, hold_s, seconds):
    deadline = time.time() + seconds
    held_since = None
    last = None
    while time.time() < deadline:
        r = ven_get("/sim")
        last = r.json().get("grid", {}).get("net_power_kw") if r.ok else None
        if last is not None and low_kw <= last <= high_kw:
            held_since = held_since or time.time()
            if time.time() - held_since >= hold_s:
                return
        else:
            held_since = None
        time.sleep(1)
    raise AssertionError(
        f"net site power not held in [{low_kw}, {high_kw}] kW for {hold_s}s within {seconds}s "
        f"(last {last} kW)"
    )


def _asset(asset_id):
    r = ven_get("/sim")
    if not r.ok:
        return None
    return r.json()["assets"].get(asset_id)


@given("the EV is plugged in on the snapshot")
def step_ev_plugged_on_snapshot(context):
    # The departure is the plugged-to-away transition, so the tick must have seen it plugged.
    poll_until(
        lambda: _asset("ev") or {},
        lambda a: a.get("plugged") == 1.0,
        timeout=20,
        description="the EV plugged on /sim",
    )


@when("the EV leaves")
def step_ev_leaves(context):
    r = ven_post("/sim/inject", json={"ev_plugged": False})
    r.raise_for_status()


@then('within {seconds:d} seconds the "{asset_id}" reports more than {kwh:g} kWh unserved')
def step_reports_unserved_more_than(context, seconds, asset_id, kwh):
    poll_until(
        lambda: _asset(asset_id) or {},
        lambda a: (a.get("unserved_energy_kwh") or 0.0) > kwh,
        timeout=seconds,
        description=f"{asset_id} unserved energy above {kwh} kWh",
    )


@then('within {seconds:d} seconds the "{asset_id}" reports no unserved energy')
def step_reports_no_unserved(context, seconds, asset_id):
    poll_until(
        lambda: _asset(asset_id) or {},
        lambda a: a.get("unserved_energy_kwh") == 0.0,
        timeout=seconds,
        description=f"{asset_id} declares 0 kWh unserved",
    )


@then('within {seconds:d} seconds the "{asset_id}" reports stored energy')
def step_reports_stored(context, seconds, asset_id):
    poll_until(
        lambda: _asset(asset_id) or {},
        lambda a: (a.get("stored_energy_kwh") or 0.0) > 0.0,
        timeout=seconds,
        description=f"{asset_id} stored energy",
    )


@then('the "{asset_id}" declares no unserved energy')
def step_declares_no_unserved(context, asset_id):
    asset = _asset(asset_id)
    assert asset is not None, f"{asset_id} is not on /sim"
    assert "unserved_energy_kwh" not in asset, f"{asset_id}: {asset['unserved_energy_kwh']}"
