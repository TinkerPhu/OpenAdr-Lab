"""Step definitions for Phase A physics and capability BDD tests.

Covers: capability() state-dependence (battery SoC bounds, EV unplugged, PV is_fixed)
and UserOverrides paths (pv_irradiance, ev_plugged).
"""

import time
from behave import given, when, then
from features.helpers.api_client import ven_post


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


@when("I POST a sim override setting pv_irradiance to {irradiance:f}")
def step_sim_override_pv_irradiance(context, irradiance):
    r = ven_post("/sim/override", json={"pv_irradiance": irradiance})
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
    assert actual < threshold, (
        f"Expected max_import_kw < {threshold}, got {actual}"
    )
