"""GB-35: the per-scenario cleanup (`helpers/scenario_state.py`) puts back what a scenario changed.

The pair in `scenario_cleanup.feature` spans two scenarios, so what the first one found is kept here
at module level: behave builds a fresh context for each scenario.
"""

from behave import given, then
from features.helpers.api_client import ven_get

_remembered = {}


def _ven_state():
    sim = ven_get("/sim").json()
    return {
        "battery_soc": sim["assets"]["battery"]["soc"],
        "ev_comfort_source": ven_get("/assets/ev/comfort_curve").json()["source"],
        "ev_plugged_inject": ven_get("/sim/inject").json().get("ev_plugged"),
    }


@given("I remember the VEN state a scenario can change")
def step_remember_ven_state(context):
    _remembered.clear()
    _remembered.update(_ven_state())


@then("the VEN state a scenario can change is back to what was remembered")
def step_ven_state_restored(context):
    assert _remembered, "the previous scenario remembered nothing; run the pair together"
    now = _ven_state()
    # SoC moves with the battery's own charging between the two reads: 0.02 is ~1.5 min at 5 kW.
    assert abs(now["battery_soc"] - _remembered["battery_soc"]) < 0.02, (now, _remembered)
    assert now["ev_comfort_source"] == _remembered["ev_comfort_source"], (now, _remembered)
    assert now["ev_plugged_inject"] == _remembered["ev_plugged_inject"], (now, _remembered)
