"""Step definitions for VEN simulator and reactor tests."""

import time
from behave import when, then
from features.helpers.api_client import ven_get, VEN_BASE_URL, HTTP_TIMEOUT
from features.helpers.wait import poll_until
import requests


@when("I query VEN-1 simulator state")
def step_query_sim(context):
    r = ven_get("/sim")
    r.raise_for_status()
    context.sim_response = r.json()


@when("I query the VEN-1 sim schema")
def step_query_sim_schema(context):
    r = ven_get("/sim/schema")
    r.raise_for_status()
    context.sim_schema = r.json()


@then('the schema for "{asset}" has control key "{key}"')
def step_schema_has_control_key(context, asset, key):
    schema = context.sim_schema
    assert asset in schema, (
        f"Asset '{asset}' not found in sim schema. Available: {list(schema.keys())}"
    )
    controls = schema[asset]
    keys = [c.get("key") for c in controls]
    assert key in keys, (
        f"Control key '{key}' not found for asset '{asset}'. Found: {keys}"
    )


@then('the sim response top-level keys are "{keys}"')
def step_sim_top_level_keys(context, keys):
    required = {k.strip() for k in keys.split(",")}
    actual = set(context.sim_response.keys())
    assert required.issubset(actual), (
        f"Expected top-level keys {required} to be present, got: {actual}"
    )
    assert actual == required, (
        f"Expected exactly top-level keys {required}, got extra: {actual - required}"
    )


@then('the sim response does not have field "{field}"')
def step_sim_does_not_have_field(context, field):
    assert field not in context.sim_response, (
        f"Expected field '{field}' to be absent from sim response top level, "
        f"but it was present. Keys: {list(context.sim_response.keys())}"
    )


@then('the sim grid has field "{field}"')
def step_sim_grid_has_field(context, field):
    grid = context.sim_response.get("grid", {})
    assert field in grid, (
        f"Expected field '{field}' in sim.grid, got keys: {list(grid.keys())}"
    )


@then('the sim grid does not have field "{field}"')
def step_sim_grid_does_not_have_field(context, field):
    grid = context.sim_response.get("grid", {})
    assert field not in grid, (
        f"Expected field '{field}' to be absent from sim.grid, "
        f"but it was present. Keys: {list(grid.keys())}"
    )


@then('the sim device "{device}" has field "{field}"')
def step_sim_device_has_field(context, device, field):
    assets = context.sim_response.get("assets", {})
    asset = assets.get(device)
    assert asset is not None, (
        f"Device '{device}' not found in sim assets. Available: {list(assets.keys())}"
    )
    assert field in asset, (
        f"Expected field '{field}' in sim.assets['{device}'], "
        f"got keys: {list(asset.keys())}"
    )


@then('the sim response has device "{device}"')
def step_sim_has_device(context, device):
    assets = context.sim_response.get("assets", {})
    val = assets.get(device)
    assert val is not None, (
        f"Expected device '{device}' in sim response assets, got None. "
        f"Available: {list(assets.keys())}"
    )


@then('the sensor raw source is "{source}"')
def step_sensor_raw_source(context, source):
    raw = context.ven_sensor.get("raw", {})
    actual = raw.get("source") if isinstance(raw, dict) else None
    assert actual == source, (
        f"Expected sensor raw source '{source}', got '{actual}'"
    )


@then('an auto-report for event "{event_name}" exists on VEN-1')
def step_auto_report_exists(context, event_name):
    # Find the event ID from VEN-1's event list
    events = requests.get(f"{VEN_BASE_URL}/events", timeout=HTTP_TIMEOUT).json()
    event = next((e for e in events if e.get("eventName") == event_name), None)
    assert event is not None, f"Event '{event_name}' not found on VEN-1"
    event_id = event["id"]

    # Poll VEN-1 reports until an auto-report for this event appears
    def fetch_matching():
        reports = requests.get(f"{VEN_BASE_URL}/reports", timeout=HTTP_TIMEOUT).json()
        return [
            r for r in reports
            if r.get("reportName", "").startswith("auto-ven-1-")
            and r.get("eventID") == event_id
        ]

    matching = poll_until(
        fetch_matching,
        lambda m: len(m) > 0,
        timeout=60,
        description=f"auto-report for event '{event_name}'",
    )

    # Verify report has USAGE payload with numeric value
    report = matching[0]
    resources = report.get("resources", [])
    assert len(resources) > 0, "Auto-report has no resources"
    payloads = resources[0].get("intervals", [{}])[0].get("payloads", [])
    types = [p.get("type") for p in payloads]
    assert "USAGE" in types, f"Expected USAGE payload, got types: {types}"
    assert "OPERATING_STATE" in types, f"Expected OPERATING_STATE payload, got types: {types}"
