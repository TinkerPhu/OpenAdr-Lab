"""Step definitions for VEN Dispatcher (Stage 4) BDD tests."""

from behave import given, when, then
from features.helpers.api_client import ven_get, ven_post, ven_put
from features.helpers.wait import poll_until


# ---------------------------------------------------------------------------
# When: poll for ledger state
# ---------------------------------------------------------------------------

@when('I poll VEN /ledger until field "{field}" is greater than {threshold:f}')
def step_poll_ledger_field(context, field, threshold):
    """Poll GET /ledger until a dotted field exceeds a threshold."""
    def fetch():
        r = ven_get("/ledger")
        r.raise_for_status()
        return r.json()

    def resolve(data, path):
        for part in path.split("."):
            if not isinstance(data, dict):
                return None
            data = data.get(part)
        return data

    def check(data):
        val = resolve(data, field)
        return isinstance(val, (int, float)) and val > threshold

    context.last_response_json = poll_until(
        fetch, check, timeout=15,
        description=f"VEN /ledger field '{field}' > {threshold}",
    )


# ---------------------------------------------------------------------------
# Then: packet status assertions
# ---------------------------------------------------------------------------

@then('the response JSON field "{field_path}" is the string "{expected}"')
def step_response_json_field_is_string(context, field_path, expected):
    """Assert a nested JSON field equals a specific string value."""
    data = context.last_response_json
    assert data is not None, "Response was not JSON"

    def resolve(d, path):
        parts = path.split(".")
        for part in parts:
            if not isinstance(d, dict):
                return None
            d = d.get(part)
        return d

    val = resolve(data, field_path)
    assert val == expected, (
        f"Field '{field_path}' = {val!r}, expected string '{expected}'"
    )


# ---------------------------------------------------------------------------
# Layer 1 — reactive battery correction
# ---------------------------------------------------------------------------

@when("I inject base_load_kw {kw:f} with alpha {alpha:f} via sim inject")
def step_inject_base_load(context, kw, alpha):
    """Inject a persistent base-load offset into the VEN sim."""
    r = ven_post("/sim/inject", json={"base_load_kw": kw, "base_load_alpha": alpha})
    r.raise_for_status()


@when("I clear the base_load_kw inject")
def step_clear_base_load_inject(context):
    """Reset the base-load override so subsequent ticks decay back to baseline."""
    r = ven_post("/sim/inject", json={"base_load_kw": 0.0, "base_load_alpha": None})
    r.raise_for_status()


@given("the deviation arbiter is enabled")
def step_deviation_arbiter_enabled(context):
    """BL-37: flip the arbiter rollout gate on (PUT /arbiter-settings)."""
    r = ven_put("/arbiter-settings", json={"deviation_arbiter_enabled": True})
    r.raise_for_status()


@when("the deviation arbiter is disabled")
def step_deviation_arbiter_disabled(context):
    """BL-37: reset the arbiter rollout gate to its default so later
    scenarios start clean."""
    r = ven_put("/arbiter-settings", json={"deviation_arbiter_enabled": False})
    r.raise_for_status()


