"""
Step definitions for phase_c_flexibility_policy.feature.

The policy VEN runs with policy_test.yaml (default_reserve up_kw=3.0).
A VTN IMPORT_CAPACITY_LIMIT event sets a finite grid cap (e.g. 10.0 kW).
The planner reduces it by site_import_reduction_kw = 3.0 kW.

Layer 1 (CP3): import_cap_kw = 10.0 - 3.0 = 7.0 kW on all firm slots.
Layer 3 (CP4): future SIMPLE event with value 5.0 kW →
  import_cap_kw = 10.0 - 3.0 - 5.0 = 2.0 kW for slots inside the event window.
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


@when("I wait for the policy VEN plan to have firm slots with import_cap_kw at most {cap:f} kW")
def step_wait_policy_plan_cap(context, cap):
    """Poll policy VEN /plan until all firm slots reflect the reduced cap."""
    deadline = time.time() + _PLAN_TIMEOUT_S
    while time.time() < deadline:
        r = _policy_get("/plan")
        if r.status_code == 200:
            plan = r.json()
            if plan and plan.get("firm_slots"):
                all_ok = all(
                    slot.get("import_cap_kw", float("inf")) <= cap + 0.1
                    for slot in plan["firm_slots"]
                )
                if all_ok:
                    context.policy_firm_slots = plan["firm_slots"]
                    return
        time.sleep(1)
    # Capture last seen cap for diagnostic
    last_cap = None
    r = _policy_get("/plan")
    if r.status_code == 200:
        plan = r.json()
        slots = plan.get("firm_slots", [])
        if slots:
            last_cap = slots[0].get("import_cap_kw")
    raise AssertionError(
        f"policy VEN firm slots never showed import_cap_kw ≤ {cap} kW "
        f"after {_PLAN_TIMEOUT_S}s (last seen: {last_cap})"
    )


@then("every policy VEN firm slot has import_cap_kw at most {cap:f} kW")
def step_every_policy_slot_cap(context, cap):
    slots = getattr(context, "policy_firm_slots", [])
    assert slots, "no firm slots found — When step did not complete"
    violations = [
        (s.get("slot_index"), s.get("import_cap_kw"))
        for s in slots
        if s.get("import_cap_kw", float("inf")) > cap + 0.1
    ]
    assert not violations, (
        f"firm slots exceeded {cap} kW import_cap_kw: {violations}"
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


@when("I wait for the policy VEN plan to have at least one firm slot with import_cap_kw at most {cap:f} kW")
def step_wait_at_least_one_slot_capped(context, cap):
    """Poll GET /plan until at least one firm slot has import_cap_kw <= cap."""
    deadline = time.time() + _PLAN_TIMEOUT_S
    while time.time() < deadline:
        r = _policy_get("/plan")
        if r.status_code == 200:
            plan = r.json()
            slots = plan.get("firm_slots", [])
            matching = [s for s in slots if s.get("import_cap_kw", float("inf")) <= cap + 0.1]
            if matching:
                context.policy_matching_slots = matching
                return
        time.sleep(1)
    last_caps = []
    r = _policy_get("/plan")
    if r.status_code == 200:
        last_caps = [s.get("import_cap_kw") for s in r.json().get("firm_slots", [])]
    raise AssertionError(
        f"Timeout: no firm slot with import_cap_kw <= {cap} kW after {_PLAN_TIMEOUT_S}s "
        f"(sample caps: {last_caps[:5]})"
    )


@then("at least one policy VEN firm slot has import_cap_kw at most {cap:f} kW")
def step_at_least_one_slot_capped(context, cap):
    slots = getattr(context, "policy_matching_slots", [])
    assert slots, f"Expected at least one firm slot with import_cap_kw <= {cap} kW, found none"
    for s in slots:
        actual = s.get("import_cap_kw", float("inf"))
        assert actual <= cap + 0.1, f"Slot import_cap_kw={actual} exceeds {cap + 0.1} kW"
