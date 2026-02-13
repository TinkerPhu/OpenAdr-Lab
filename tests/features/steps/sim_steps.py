"""Step definitions for VEN simulator and reactor tests."""

import time
from behave import when, then
from features.helpers.api_client import ven_get


@when("I query VEN-1 simulator state")
def step_query_sim(context):
    r = ven_get("/sim")
    r.raise_for_status()
    context.sim_response = r.json()


@when("I query VEN-1 decision trace")
def step_query_trace(context):
    r = ven_get("/trace")
    r.raise_for_status()
    context.trace_response = r.json()


@when("I wait {seconds:d} seconds for the reactor")
def step_wait_seconds(context, seconds):
    time.sleep(seconds)


@then('the sim response has field "{field}"')
def step_sim_has_field(context, field):
    assert field in context.sim_response, (
        f"Expected field '{field}' in sim response, "
        f"got keys: {list(context.sim_response.keys())}"
    )


@then('the sim response has device "{device}"')
def step_sim_has_device(context, device):
    val = context.sim_response.get(device)
    assert val is not None, (
        f"Expected device '{device}' in sim response, got None. "
        f"Available: {[k for k in context.sim_response if context.sim_response[k] is not None]}"
    )


@then("the trace response is a list")
def step_trace_is_list(context):
    assert isinstance(context.trace_response, list), (
        f"Expected trace to be a list, got {type(context.trace_response)}"
    )


@then('each trace entry has fields "{fields}"')
def step_trace_entry_fields(context, fields):
    required = [f.strip() for f in fields.split(",")]
    for i, entry in enumerate(context.trace_response):
        for field in required:
            assert field in entry, (
                f"Trace entry {i} missing field '{field}', "
                f"got keys: {list(entry.keys())}"
            )


@then('the sensor raw source is "{source}"')
def step_sensor_raw_source(context, source):
    raw = context.ven_sensor.get("raw", {})
    actual = raw.get("source") if isinstance(raw, dict) else None
    assert actual == source, (
        f"Expected sensor raw source '{source}', got '{actual}'"
    )


@then('the trace contains an entry with mode "{mode}"')
def step_trace_contains_mode(context, mode):
    entries = context.trace_response
    assert any(e.get("mode") == mode for e in entries), (
        f"No trace entry with mode '{mode}'. "
        f"Modes found: {[e.get('mode') for e in entries[:10]]}"
    )
