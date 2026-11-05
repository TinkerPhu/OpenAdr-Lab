"""Step definitions for forecast_publication.feature: the lab-forecaster container's retained PV
forecasts on the test Mosquitto broker. Requires the `test-forecaster` and `mosquitto` services in
tests/docker-compose.test.yml; the weather input is published by the steps of weather_forecast_steps.py.
Contract: docs/architecture/forecast_publication.md and docs/reference/WIRE_PROFILE.md#v4.
"""

import json
import threading
import time
from datetime import datetime, timezone

from behave import then

from features.helpers.wait import poll_until

SITE = "ven-1"
ROOT = "openadr-lab"
STATUS_TOPIC = f"{ROOT}/forecast/{SITE}/status"
TOTAL_TOPIC = f"{ROOT}/forecast/{SITE}/pv/total/power_kw"


def _array_topic(array_id: str) -> str:
    return f"{ROOT}/forecast/{SITE}/pv/{array_id}/power_kw"


def _retained(topic: str, wait_s: float = 4.0):
    """The retained message on `topic` as a dict, or None when the broker holds none. A fresh
    subscription is what a late reader does, so this is also the late-subscriber check."""
    import paho.mqtt.client as mqtt

    got = {}
    arrived = threading.Event()
    client = mqtt.Client()

    def on_connect(c, _userdata, _flags, _rc):
        c.subscribe(topic, qos=1)

    def on_message(_c, _userdata, msg):
        if msg.retain:
            got["body"] = json.loads(msg.payload)
            arrived.set()

    client.on_connect = on_connect
    client.on_message = on_message
    client.connect("mosquitto", 1883, keepalive=10)
    client.loop_start()
    try:
        arrived.wait(wait_s)
    finally:
        client.loop_stop()
        client.disconnect()
    return got.get("body")


def _wait_for(fn, description, timeout=60):
    """Poll until `fn()` returns something other than None."""
    return poll_until(fn, lambda result: result is not None, timeout=timeout, interval=1, description=description)


def _status_in(state: str):
    """The retained status when it is in `state`, else None."""
    status = _retained(STATUS_TOPIC)
    return status if status and status["state"] == state else None


def _parse_time(text: str) -> datetime:
    return datetime.fromisoformat(text.replace("Z", "+00:00"))


@then('the lab-forecaster has published a retained PV forecast for the array "{array_id}" of VEN-1\'s site')
def step_pv_forecast_published(context, array_id):
    # `generated_at` must be newer than this scenario's weather message, so a forecast left over from an
    # earlier scenario does not count.
    started = datetime.now(timezone.utc).timestamp() - 60

    def fresh():
        body = _retained(_array_topic(array_id))
        if body and _parse_time(body["generated_at"]).timestamp() >= started:
            return body
        return None

    context.pv_forecast = _wait_for(fresh, f"a retained PV forecast on {_array_topic(array_id)}")


@then("that forecast declares power in kW, positive for generation, natural, as the average over each step")
def step_forecast_declares_contract(context):
    body = context.pv_forecast
    assert body["quantity"] == "power", body
    assert body["unit"] == "kW", body
    assert body["sign"] == "generation_positive", body
    assert body["curtailment"] == "natural", body
    assert body["reading"] == "average_over_step", body
    assert body["schema_version"].split(".")[0] == "1", body
    assert body["producer_id"] == "test-forecaster", body
    assert body["model_version"], body
    assert all(isinstance(v, (int, float)) and v >= 0 for v in body["values"]), body["values"]


@then("that forecast is valid for {minutes:g} minutes and covers {hours:g} hours")
def step_forecast_validity_and_coverage(context, minutes, hours):
    body = context.pv_forecast
    limit_s = (_parse_time(body["valid_until"]) - _parse_time(body["generated_at"])).total_seconds()
    assert limit_s == minutes * 60, f"freshness limit {limit_s}s, expected {minutes} min"
    assert body["coverage_h"] == hours, body["coverage_h"]
    assert len(body["values"]) * body["step_s"] / 3600 == hours, "values do not match coverage_h"


@then('the retained site total equals the PV forecast of the array "{array_id}"')
def step_total_equals_array(context, array_id):
    # One array in the test profile, so the sum of the arrays is that array.
    body = _wait_for(lambda: _retained(TOTAL_TOPIC), f"a retained {TOTAL_TOPIC}", timeout=30)
    array = _retained(_array_topic(array_id))
    assert body["values"] == array["values"], "site total differs from the only array"
    assert body["sign"] == "generation_positive" and body["unit"] == "kW", body


@then('the lab-forecaster\'s status is "{state}" and names its producer and the age of its inputs')
def step_status_names_producer_and_inputs(context, state):
    status = _wait_for(lambda: _status_in(state), f"a retained status in state {state}")
    assert status["producer_id"] == "test-forecaster", status
    assert status["input_age_s"]["weather"] is not None, status
    assert status["snow_state"]["pv"] in ("observed", "unobserved"), status


@then('the lab-forecaster\'s status becomes "{state}"')
def step_status_becomes(context, state):
    _wait_for(lambda: _status_in(state), f"the forecaster status to become {state}")


@then('the retained PV forecast of the array "{array_id}" is not refreshed while the weather stays stale')
def step_forecast_not_refreshed(context, array_id):
    before = _retained(_array_topic(array_id))["generated_at"]
    time.sleep(12)  # more than two recompute cadences (5 s) of the test container
    after = _retained(_array_topic(array_id))["generated_at"]
    assert after == before, f"the forecast was refreshed on stale weather: {before} -> {after}"
