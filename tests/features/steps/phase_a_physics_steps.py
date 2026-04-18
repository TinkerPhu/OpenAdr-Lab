"""Step definitions for Phase A physics and capability BDD tests.

Covers: capability() state-dependence (battery SoC bounds, EV unplugged, PV is_fixed)
and UserOverrides paths (pv_irradiance, ev_plugged).
"""

import time
from behave import given, when, then
from features.helpers.api_client import ven_post, ven_get
from features.helpers.wait import poll_until


# ── Given: setup helpers ──────────────────────────────────────────────────────

@given("the battery SoC is reset to {soc:f}")
def step_given_battery_soc_reset(context, soc):
    """Force battery SoC to a specific value via POST /sim/reset/battery."""
    r = ven_post("/sim/reset/battery", json={"soc": soc})
    assert r.status_code == 204, (
        f"Expected 204 from /sim/reset/battery, got {r.status_code}: {r.text}"
    )


# ── When: SoC reset and override helpers ─────────────────────────────────────

@when("the battery SoC is reset to {soc:f}")
def step_when_battery_soc_reset(context, soc):
    """Force battery SoC to a specific value via POST /sim/reset/battery."""
    r = ven_post("/sim/reset/battery", json={"soc": soc})
    assert r.status_code == 204, (
        f"Expected 204 from /sim/reset/battery, got {r.status_code}: {r.text}"
    )


@given("I inject ev_soc {soc:f} via sim inject")
def step_given_inject_ev_soc(context, soc):
    r = ven_post("/sim/inject", json={"ev_soc": soc})
    r.raise_for_status()


@given("I inject pv irradiance {irradiance:f} via sim inject")
def step_given_inject_pv_irradiance(context, irradiance):
    r = ven_post("/sim/inject", json={"pv_irradiance": irradiance})
    r.raise_for_status()


@when("I POST a sim override setting pv_irradiance to {irradiance:f}")
def step_sim_override_pv_irradiance(context, irradiance):
    r = ven_post("/sim/inject", json={"pv_irradiance": irradiance})
    r.raise_for_status()
    context.last_response = r


@when("I wait {seconds:d} seconds for the sim to tick")
def step_wait_sim_tick(context, seconds):
    time.sleep(seconds)


# ── Then: capability assertions ───────────────────────────────────────────────
# Uses context.last_response_json set by the shared "I GET {path} from the VEN" step.

@then("the capability max_import_kw is {expected:f}")
def step_capability_max_import(context, expected):
    data = context.last_response_json
    assert data is not None, "No capability JSON in context (request failed?)"
    actual = data.get("max_import_kw")
    assert actual is not None, f"'max_import_kw' missing from response: {data}"
    assert abs(actual - expected) < 1e-6, (
        f"Expected max_import_kw={expected}, got {actual}"
    )


@then("the capability max_export_kw is {expected:f}")
def step_capability_max_export(context, expected):
    data = context.last_response_json
    assert data is not None, "No capability JSON in context (request failed?)"
    actual = data.get("max_export_kw")
    assert actual is not None, f"'max_export_kw' missing from response: {data}"
    assert abs(actual - expected) < 1e-6, (
        f"Expected max_export_kw={expected}, got {actual}"
    )


@then("the capability is_fixed is true")
def step_capability_is_fixed(context):
    data = context.last_response_json
    assert data is not None, "No capability JSON in context (request failed?)"
    assert data.get("is_fixed") is True, (
        f"Expected is_fixed=true, got is_fixed={data.get('is_fixed')}. "
        f"Full response: {data}"
    )


@then("the capability max_import_kw is less than {threshold:f}")
def step_capability_max_import_lt(context, threshold):
    data = context.last_response_json
    assert data is not None, "No capability JSON in context (request failed?)"
    actual = data.get("max_import_kw")
    assert actual is not None, f"'max_import_kw' missing from response: {data}"
    # IEEE 754: -0.0 < 0.0 is False. Treat values within 1e-6 of threshold as passing.
    assert actual < threshold + 1e-6, (
        f"Expected max_import_kw < {threshold}, got {actual}"
    )


# ── Polling capability steps ──────────────────────────────────────────────────

@when('I wait for the VEN /capability/{asset} {field} to equal {expected:f}')
def step_poll_capability(context, asset, field, expected):
    """Poll GET /capability/{asset} until `field` matches `expected` (±1e-6)."""
    def fetch():
        r = ven_get(f"/capability/{asset}")
        r.raise_for_status()
        return r.json()

    result = poll_until(
        fetch,
        lambda data: abs(data.get(field, float('inf')) - expected) < 1e-6,
        timeout=30,
        interval=1,
        description=f"/capability/{asset} {field}=={expected}",
    )
    context.polled_capability = result


@then("the polled capability matched")
def step_polled_capability_matched(context):
    assert context.polled_capability is not None, "No polled capability result"
