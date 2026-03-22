"""
Step definitions for phase_c_flexibility_policy.feature.

The policy VEN runs with policy_test.yaml (default_reserve up_kw=3.0).
EV max_charge_kw=7.0 → available_cap.max_import_kw = 7.0 - 3.0 = 4.0 kW.
"""
import os
import time
import requests
from behave import given, when, then

POLICY_VEN_URL = os.environ.get("VEN_POLICY_BASE_URL", "http://localhost:8215")
_PLAN_TIMEOUT_S = 60


def _policy_get(path, **kwargs):
    return requests.get(f"{POLICY_VEN_URL}{path}", timeout=5, **kwargs)


@given("the policy VEN is healthy")
def step_policy_ven_healthy(context):
    r = _policy_get("/health")
    assert r.status_code == 200, f"policy VEN health check failed: {r.status_code}"


@when("I wait for the policy VEN plan to have EV firm allocations within headroom")
def step_wait_policy_ev_allocs(context):
    """Poll /plan until firm_slots with EV allocations appear, then store them."""
    deadline = time.time() + _PLAN_TIMEOUT_S
    while time.time() < deadline:
        r = _policy_get("/plan")
        if r.status_code == 200:
            plan = r.json()
            if plan and plan.get("firm_slots"):
                ev_allocs = [
                    alloc
                    for slot in plan["firm_slots"]
                    for alloc in slot.get("allocations", [])
                    if alloc.get("asset_id") == "ev"
                ]
                if ev_allocs:
                    context.policy_ev_allocs = ev_allocs
                    return
        time.sleep(1)
    raise AssertionError(
        f"policy VEN plan had no EV firm allocations after {_PLAN_TIMEOUT_S}s"
    )


@then("every EV firm allocation grid_power_kw is at most {limit:f} kW")
def step_ev_allocs_within_limit(context, limit):
    allocs = getattr(context, "policy_ev_allocs", [])
    assert allocs, "no EV firm allocations found — When step did not run"
    violations = [
        a for a in allocs if a.get("grid_power_kw", 0.0) > limit
    ]
    assert not violations, (
        f"EV firm allocations exceeded {limit} kW grid_power_kw: "
        + ", ".join(f"{v['grid_power_kw']:.3f}" for v in violations)
    )
