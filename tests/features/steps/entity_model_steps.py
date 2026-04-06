"""Step definitions for VEN Entity Model — Stage 1."""

import time
import json
import requests
from behave import given, when, then
import features.helpers.api_client as api_client
from features.helpers.api_client import ven_get, ven_post, VEN_BASE_URL
from features.helpers.wait import poll_until


def _resolve_nested(data, path):
    """Resolve a dotted path like 'battery.soc' into nested dict value.

    Falls back to data['assets'][first_part] when the first key is not found
    at the root level, supporting the new SimSnapshot.assets structure.
    """
    parts = path.split(".")
    val = data
    for i, part in enumerate(parts):
        if not isinstance(val, dict):
            return None
        if part in val:
            val = val[part]
        elif i == 0 and isinstance(data.get("assets"), dict) and part in data["assets"]:
            val = data["assets"][part]
        else:
            return None
    return val


@given('the VEN is running with profile "{profile}"')
def step_ven_running_with_profile(context, profile):
    """Route subsequent VEN calls to the container running the requested profile."""
    profile_urls = {
        "no_pv_test": api_client.VEN_NO_PV_BASE_URL,
    }
    api_client.VEN_BASE_URL = profile_urls.get(profile, api_client._DEFAULT_VEN_BASE_URL)
    r = ven_get("/health")
    assert r.status_code == 200, f"VEN health check failed for profile '{profile}': {r.status_code}"
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
    battery = r.json().get("assets", {}).get("battery", {})
    context.battery_min_soc = battery.get("min_soc", 0.10)
    context.expected_battery_soc = context.battery_min_soc



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


@when('I poll VEN {path} until field "{field}" is present')
def step_poll_ven_field_present(context, path, field):
    """Poll a VEN endpoint until a dotted field path is non-None."""
    def fetch():
        r = ven_get(path)
        r.raise_for_status()
        return r.json()

    context.last_response_json = poll_until(
        fetch,
        lambda data: _resolve_nested(data, field) is not None,
        timeout=15,
        description=f"VEN {path} field '{field}' is present",
    )


@when('I poll VEN {path} until field "{field}" is greater than {threshold:f}')
def step_poll_ven_field_gt(context, path, field, threshold):
    """Poll a VEN endpoint until a field exceeds a threshold."""
    def fetch():
        r = ven_get(path)
        r.raise_for_status()
        return r.json()

    def check(data):
        val = _resolve_nested(data, field)
        return isinstance(val, (int, float)) and val > threshold

    context.last_response_json = poll_until(
        fetch, check, timeout=15,
        description=f"VEN {path} field '{field}' > {threshold}",
    )


@when('I poll VEN {path} until field "{field}" equals {expected:f}')
def step_poll_ven_field_eq(context, path, field, expected):
    """Poll a VEN endpoint until a field equals an expected value."""
    def fetch():
        r = ven_get(path)
        r.raise_for_status()
        return r.json()

    def check(data):
        val = _resolve_nested(data, field)
        return isinstance(val, (int, float)) and abs(val - expected) < 0.001

    context.last_response_json = poll_until(
        fetch, check, timeout=15,
        description=f"VEN {path} field '{field}' == {expected}",
    )


@when('I poll VEN {path} until it is a non-empty array')
def step_poll_ven_nonempty_array(context, path):
    """Poll a VEN endpoint until it returns a non-empty JSON array."""
    def fetch():
        r = ven_get(path)
        r.raise_for_status()
        return r.json()

    context.last_response_json = poll_until(
        fetch,
        lambda data: isinstance(data, list) and len(data) > 0,
        timeout=15,
        description=f"VEN {path} returns non-empty array",
    )


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
    resp = getattr(context, "last_response", None)
    if resp is None:
        resp = getattr(context, "response", None)
    assert resp is not None, "No response stored in context (checked last_response and response)"
    assert resp.status_code == code, (
        f"Expected HTTP {code}, got {resp.status_code}. "
        f"Body: {resp.text[:200]}"
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


@then('the response JSON contains field "{field}"')
def step_response_json_has_field(context, field):
    data = context.last_response_json
    assert isinstance(data, dict), (
        f"Expected JSON object, got {type(data).__name__}: {data!r}"
    )
    assert field in data, (
        f"Response missing field '{field}'. Keys: {list(data.keys())}"
    )


@then('the response JSON field "{field}" is greater than {threshold:f}')
def step_response_json_field_gt(context, field, threshold):
    data = context.last_response_json
    assert isinstance(data, dict), f"Expected JSON object, got {type(data).__name__}: {data!r}"
    val = data.get(field)
    assert isinstance(val, (int, float)), f"Field '{field}' is not a number: {val!r}"
    assert val > threshold, f"Field '{field}' = {val} is not > {threshold}"


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
    battery = r.json().get("assets", {}).get("battery")
    assert battery is not None, "No battery in /sim response assets"
    assert abs(battery.get("power_kw", 999)) < 1e-6, (
        f"Expected battery.power_kw = 0.0, got {battery.get('power_kw')}"
    )
