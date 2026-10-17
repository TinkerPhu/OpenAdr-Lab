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


@when("PV generates {kw:g} kW against the {base_kw:g} kW base load")
def step_pv_generates(context, kw, base_kw):
    # The test profile's array is rated 5 kW. A forced irradiance decays over about 47
    # minutes, so it holds for the seconds this scenario watches.
    rated_kw = ven_get("/sim").json()["assets"]["pv"].get("rated_kw", 5.0)
    r = ven_post("/sim/inject", json={"pv_irradiance": kw / rated_kw, "base_load_kw": base_kw})
    r.raise_for_status()


def _asset_kw(asset_id):
    r = ven_get("/sim")
    if not r.ok:
        return None
    return r.json()["assets"][asset_id]["power_kw"]


@then('within {seconds:d} seconds the "{asset_id}" draws at least {kw:g} kW')
def step_asset_draws_at_least(context, seconds, asset_id, kw):
    poll_until(
        lambda: _asset_kw(asset_id),
        lambda p: p is not None and p >= kw,
        timeout=seconds,
        interval=1,
        description=f"{asset_id} drawing at least {kw} kW",
    )


@then('the "{asset_id}" draws at most {kw:g} kW for {seconds:d} seconds')
def step_asset_draws_at_most(context, asset_id, kw, seconds):
    deadline = time.time() + seconds
    while time.time() < deadline:
        p = _asset_kw(asset_id)
        assert p is None or p <= kw, f"{asset_id} drew {p} kW, more than {kw} kW"
        time.sleep(1)


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
