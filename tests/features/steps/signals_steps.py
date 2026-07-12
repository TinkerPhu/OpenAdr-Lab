"""Step definitions for the WP4.6 GET /signals aggregate."""

from behave import when, then
from features.helpers.api_client import ven_get
from features.helpers.wait import poll_until


def _fetch_signals():
    r = ven_get("/signals")
    if not r.ok:
        return None
    return r.json()


@when("I wait for the VEN /signals to report an active alert")
def step_wait_signals_alert(context):
    context.signals = poll_until(
        _fetch_signals,
        lambda s: s is not None and len(s.get("alerts", [])) > 0,
        timeout=90,
        interval=5,
        description="/signals reports an active alert window",
    )


@when("I wait for the VEN /signals to report no active alert")
def step_wait_signals_no_alert(context):
    context.signals = poll_until(
        _fetch_signals,
        lambda s: s is not None and len(s.get("alerts", [])) == 0,
        timeout=90,
        interval=5,
        description="/signals reports no active alert window",
    )


@then('the VEN /signals response has the keys "{keys}"')
def step_signals_keys(context, keys):
    expected = {k.strip() for k in keys.split(",")}
    actual = set(context.signals.keys())
    assert expected <= actual, f"missing keys: {expected - actual} in {actual}"
