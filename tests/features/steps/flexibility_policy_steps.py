"""
Step definitions for phase_c_flexibility_policy.feature.

The policy VEN runs with policy_test.yaml (default_reserve_up_kw=3.0).
Phase D: reservations are recorded in PlanStep.reserved_up_kw (summed over all
active reservations at each timestep) rather than reducing slot.import_cap_kw.

Layer 1 (CP3): reserved_up_kw=3.0 on all steps (policy default reserve).
Layer 3 (CP4): future SIMPLE event with value 5.0 kW →
  reserved_up_kw=8.0 (3.0+5.0) for steps inside the event window.
"""
import os
import time
import requests
from behave import given, when, then
from features.helpers.api_client import vtn_post, get_token_value

POLICY_VEN_URL = os.environ.get("VEN_POLICY_BASE_URL", "http://localhost:8215")
_PLAN_TIMEOUT_S = 60


def _policy_get(path, **kwargs):
    return requests.get(f"{POLICY_VEN_URL}{path}", timeout=5, **kwargs)


@given("a VTN IMPORT_CAPACITY_LIMIT event with value {kw:f} kW is active")
def step_create_capacity_limit_event(context, kw):
    """Create an open program + IMPORT_CAPACITY_LIMIT event via VTN.

    Stores the event in context.uc_events so environment.py after_scenario
    cleanup deletes it automatically. Uses a unique program name per scenario
    to avoid 409 conflicts from programs not cleaned between scenarios.
    """
    import uuid
    token = getattr(context, "vtn_token", None) or get_token_value("any-business", "any-business")

    # Unique name avoids 409 when program from a previous scenario wasn't deleted.
    prog_name = f"policy-cap-{uuid.uuid4().hex[:8]}"
    r = vtn_post("/programs", token, json={"programName": prog_name, "targets": None})
    assert r.status_code == 201, f"program creation failed: {r.status_code} {r.text}"
    program_id = r.json()["id"]
    context._policy_program_id = program_id  # reused by Layer 3 And steps

    # Create IMPORT_CAPACITY_LIMIT event (no intervalPeriod → active immediately)
    r2 = vtn_post("/events", token, json={
        "programID": program_id,
        "eventName": "policy-cap",
        "priority": 1,
        "intervals": [{"id": 0, "payloads": [{"type": "IMPORT_CAPACITY_LIMIT", "values": [kw]}]}],
    })
    assert r2.status_code == 201, f"event creation failed: {r2.status_code} {r2.text}"
    event = r2.json()

    # Register for auto-cleanup by environment.py after_scenario
    if not hasattr(context, "uc_events"):
        context.uc_events = {}
    context.uc_events["policy-cap"] = event


@when("I wait for the policy VEN plan steps to have reserved_up_kw at least {min_kw:f} kW")
def step_wait_policy_plan_min_reserved(context, min_kw):
    """Poll policy VEN /plan until all steps show reserved_up_kw >= min_kw."""
    deadline = time.time() + _PLAN_TIMEOUT_S
    while time.time() < deadline:
        r = _policy_get("/plan")
        if r.status_code == 200:
            plan = r.json()
            steps = plan.get("steps", [])
            if steps and all(s.get("reserved_up_kw", 0.0) >= min_kw - 0.1 for s in steps):
                context.policy_plan_steps = steps
                return
        time.sleep(1)
    last_val = None
    r = _policy_get("/plan")
    if r.status_code == 200:
        steps = r.json().get("steps", [])
        if steps:
            last_val = steps[0].get("reserved_up_kw")
    raise AssertionError(
        f"policy VEN steps never showed reserved_up_kw >= {min_kw} kW "
        f"after {_PLAN_TIMEOUT_S}s (last seen: {last_val})"
    )


@then("every policy VEN plan step has reserved_up_kw at least {min_kw:f} kW")
def step_every_policy_step_reserved(context, min_kw):
    steps = getattr(context, "policy_plan_steps", [])
    assert steps, "no plan steps found — When step did not complete"
    violations = [
        (s.get("asset_id"), s.get("ts"), s.get("reserved_up_kw"))
        for s in steps
        if s.get("reserved_up_kw", 0.0) < min_kw - 0.1
    ]
    assert not violations, (
        f"plan steps with reserved_up_kw < {min_kw} kW: {violations[:5]}"
    )


# ---------------------------------------------------------------------------
# Layer 3 — pre-announced VTN events (CP4)
# ---------------------------------------------------------------------------

@given("a VTN SIMPLE event with value {kw:f} kW starting in {hours:d} hours for {duration:d} hours")
def step_future_simple_event(context, kw, hours, duration):
    """Create a SIMPLE event with a future intervalPeriod (Layer 3 pre-announced)."""
    import datetime
    token = getattr(context, "vtn_token", None) or get_token_value("any-business", "any-business")

    # Reuse the program created by the IMPORT_CAPACITY_LIMIT Given step, if present.
    program_id = getattr(context, "_policy_program_id", None)
    if program_id is None:
        r = vtn_post("/programs", token, json={"programName": "policy-simple-test", "targets": None})
        assert r.status_code == 201, f"program creation failed: {r.status_code} {r.text}"
        program_id = r.json()["id"]
        context._policy_program_id = program_id

    now = datetime.datetime.now(datetime.timezone.utc)
    start = now + datetime.timedelta(hours=hours)

    r = vtn_post("/events", token, json={
        "programID": program_id,
        "eventName": f"future-simple-{hours}h",
        "intervals": [{
            "id": 0,
            "intervalPeriod": {
                "start":    start.isoformat(),
                "duration": f"PT{duration}H",
            },
            "payloads": [{"type": "SIMPLE", "values": [kw]}],
        }],
    })
    assert r.status_code == 201, f"create future SIMPLE event failed: {r.status_code} {r.text}"
    if not hasattr(context, "uc_events"):
        context.uc_events = {}
    context.uc_events["policy-future-simple"] = r.json()


@given("a VTN SIMPLE event with value {kw:f} kW expired {hours:d} hours ago")
def step_expired_simple_event(context, kw, hours):
    """Create a SIMPLE event whose intervalPeriod has already ended (should be excluded)."""
    import datetime
    token = getattr(context, "vtn_token", None) or get_token_value("any-business", "any-business")

    program_id = getattr(context, "_policy_program_id", None)
    if program_id is None:
        r = vtn_post("/programs", token, json={"programName": "policy-expired-test", "targets": None})
        assert r.status_code == 201, f"program creation failed: {r.status_code} {r.text}"
        program_id = r.json()["id"]
        context._policy_program_id = program_id

    now = datetime.datetime.now(datetime.timezone.utc)
    # Window: started (hours+1)h ago, duration 1h → ended hours ago
    start = now - datetime.timedelta(hours=hours + 1)

    r = vtn_post("/events", token, json={
        "programID": program_id,
        "eventName": f"expired-simple-{hours}h-ago",
        "intervals": [{
            "id": 0,
            "intervalPeriod": {
                "start":    start.isoformat(),
                "duration": "PT1H",
            },
            "payloads": [{"type": "SIMPLE", "values": [kw]}],
        }],
    })
    assert r.status_code == 201, f"create expired SIMPLE event failed: {r.status_code} {r.text}"
    if not hasattr(context, "uc_events"):
        context.uc_events = {}
    context.uc_events["policy-expired-simple"] = r.json()


@when("I wait for the policy VEN plan steps to have at least one with reserved_up_kw at least {min_kw:f} kW")
def step_wait_at_least_one_step_reserved(context, min_kw):
    """Poll GET /plan until at least one step has reserved_up_kw >= min_kw."""
    deadline = time.time() + _PLAN_TIMEOUT_S
    while time.time() < deadline:
        r = _policy_get("/plan")
        if r.status_code == 200:
            plan = r.json()
            steps = plan.get("steps", [])
            matching = [s for s in steps if s.get("reserved_up_kw", 0.0) >= min_kw - 0.1]
            if matching:
                context.policy_matching_steps = matching
                return
        time.sleep(1)
    last_vals = []
    r = _policy_get("/plan")
    if r.status_code == 200:
        last_vals = [s.get("reserved_up_kw") for s in r.json().get("steps", [])][:5]
    raise AssertionError(
        f"Timeout: no step with reserved_up_kw >= {min_kw} kW after {_PLAN_TIMEOUT_S}s "
        f"(sample reserved_up_kw: {last_vals})"
    )


@then("at least one policy VEN plan step has reserved_up_kw at least {min_kw:f} kW")
def step_at_least_one_step_reserved(context, min_kw):
    steps = getattr(context, "policy_matching_steps", [])
    assert steps, f"Expected at least one step with reserved_up_kw >= {min_kw} kW, found none"
    for s in steps:
        actual = s.get("reserved_up_kw", 0.0)
        assert actual >= min_kw - 0.1, f"Step reserved_up_kw={actual} < {min_kw - 0.1} kW"
