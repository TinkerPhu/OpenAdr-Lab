"""Step definitions for VEN Entity Model — Stage 1."""

import time
import json
import requests
from behave import given, when, then
from features.helpers.api_client import ven_get, ven_post, VEN_BASE_URL


def _resolve_nested(data, path):
    """Resolve a dotted path like 'battery.soc' into nested dict value."""
    parts = path.split(".")
    val = data
    for part in parts:
        if not isinstance(val, dict):
            return None
        val = val.get(part)
    return val


@given('the VEN is running with profile "{profile}"')
def step_ven_running_with_profile(context, profile):
    """Verify the VEN is reachable (profile is set at container startup)."""
    r = ven_get("/health")
    assert r.status_code == 200, f"VEN health check failed: {r.status_code}"
    context.ven_profile = profile


@given("the VEN battery has initial SoC {soc:f}")
def step_ven_battery_initial_soc(context, soc):
    """Record expected initial SoC for reference in following assertions."""
    context.expected_battery_soc = soc


@given("the VEN battery has initial SoC equal to min_soc")
def step_ven_battery_at_min_soc(context):
    """Use the sim response to confirm battery is at or near min_soc."""
    r = ven_get("/sim")
    r.raise_for_status()
    battery = r.json().get("battery", {})
    context.battery_min_soc = battery.get("min_soc", 0.10)
    context.expected_battery_soc = context.battery_min_soc


@given("the VEN battery has initial SoC 1.0")
def step_ven_battery_at_full(context):
    context.expected_battery_soc = 1.0


@when("I GET {path} from the VEN")
def step_ven_get(context, path):
    r = ven_get(path)
    context.last_response = r
    context.last_response_json = r.json() if r.headers.get("content-type", "").startswith("application/json") else None


@when("I POST {path} with body:")
def step_ven_post_with_body(context, path):
    body = json.loads(context.text)
    r = ven_post(path, json=body)
    context.last_response = r
    context.last_response_json = r.json() if r.headers.get("content-type", "").startswith("application/json") else None


@when("I wait {seconds:d} seconds")
def step_wait_seconds(context, seconds):
    time.sleep(seconds)


@when("the battery is commanded to discharge")
def step_battery_commanded_discharge(context):
    """Stage 1: battery holds at 0 — nothing to command, just record intent."""
    context.battery_command = -5.0


@when("the battery is commanded to charge")
def step_battery_commanded_charge(context):
    """Stage 1: battery holds at 0 — nothing to command, just record intent."""
    context.battery_command = 5.0


@then("the response status is {code:d}")
def step_response_status(context, code):
    assert context.last_response.status_code == code, (
        f"Expected HTTP {code}, got {context.last_response.status_code}. "
        f"Body: {context.last_response.text[:200]}"
    )


@then('the response JSON has field "{field_path}"')
def step_response_json_has_field(context, field_path):
    data = context.last_response_json
    assert data is not None, "Response was not JSON"
    val = _resolve_nested(data, field_path)
    assert val is not None, (
        f"Expected field '{field_path}' in response JSON, got None. "
        f"Available keys: {list(data.keys()) if isinstance(data, dict) else type(data)}"
    )


@then('the response JSON field "{field_path}" is a number between {lo:f} and {hi:f}')
def step_response_json_field_number_range(context, field_path, lo, hi):
    data = context.last_response_json
    val = _resolve_nested(data, field_path)
    assert isinstance(val, (int, float)), (
        f"Field '{field_path}' is not a number: {val!r}"
    )
    assert lo <= val <= hi, (
        f"Field '{field_path}' = {val} is not in [{lo}, {hi}]"
    )


@then('the response JSON field "{field_path}" is greater than {threshold:f}')
def step_response_json_field_greater_than(context, field_path, threshold):
    data = context.last_response_json
    val = _resolve_nested(data, field_path)
    assert isinstance(val, (int, float)), (
        f"Field '{field_path}' is not a number: {val!r}"
    )
    assert val > threshold, (
        f"Field '{field_path}' = {val} is not > {threshold}"
    )


@then('the response JSON field "{field_path}" is less than {threshold:f}')
def step_response_json_field_less_than(context, field_path, threshold):
    data = context.last_response_json
    val = _resolve_nested(data, field_path)
    assert isinstance(val, (int, float)), (
        f"Field '{field_path}' is not a number: {val!r}"
    )
    assert val < threshold, (
        f"Field '{field_path}' = {val} is not < {threshold}"
    )


@then('the response JSON field "{field_path}" equals {expected:f}')
def step_response_json_field_equals(context, field_path, expected):
    data = context.last_response_json
    val = _resolve_nested(data, field_path)
    assert isinstance(val, (int, float)), (
        f"Field '{field_path}' is not a number: {val!r}"
    )
    assert abs(val - expected) < 1e-6, (
        f"Field '{field_path}' = {val} != {expected}"
    )


@then("the response JSON is an empty array")
def step_response_json_empty_array(context):
    data = context.last_response_json
    assert isinstance(data, list), (
        f"Expected a JSON array, got {type(data).__name__}: {data!r}"
    )
    assert len(data) == 0, (
        f"Expected empty array, got {len(data)} items: {data[:3]}"
    )


@then("the response JSON is an array")
def step_response_json_array(context):
    data = context.last_response_json
    assert isinstance(data, list), (
        f"Expected a JSON array, got {type(data).__name__}: {data!r}"
    )


@then("the response JSON is null")
def step_response_json_null(context):
    # axum returns JSON null as the literal "null"
    text = context.last_response.text.strip()
    data = context.last_response_json
    assert data is None or text == "null", (
        f"Expected JSON null, got: {text[:100]}"
    )


@then("the battery current_kw is 0.0")
def step_battery_current_kw_is_zero(context):
    """Stage 1: battery holds at 0 (no dispatcher yet)."""
    r = ven_get("/sim")
    r.raise_for_status()
    battery = r.json().get("battery")
    assert battery is not None, "No battery in /sim response"
    assert abs(battery.get("current_kw", 999)) < 1e-6, (
        f"Expected battery.current_kw = 0.0, got {battery.get('current_kw')}"
    )
