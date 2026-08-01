import time

from behave import given, when, then
from features.helpers.api_client import (
    bff_get, bff_post, bff_put, bff_delete,
    get_token_value, vtn_get, vtn_post, vtn_put,
)


# ── Programs ─────────────────────────────────────────────────────────────────

@when('I create a program via BFF named "{name}"')
@given('I create a program via BFF named "{name}"')
def step_create_program(context, name):
    r = bff_post("/api/programs", json={"programName": name})
    context.response = r
    context.created_id = r.json().get("id") if r.ok else None


@given('I create a program via BFF named "{name}" and save its ID')
def step_create_program_save_id(context, name):
    r = bff_post("/api/programs", json={"programName": name})
    r.raise_for_status()
    context.saved_program_id = r.json()["id"]
    context.response = r


@when('I update the program name to "{name}"')
def step_update_program(context, name):
    r = bff_put(f"/api/programs/{context.created_id}", json={"programName": name})
    context.response = r


@when("I delete the program via BFF")
def step_delete_program(context):
    r = bff_delete(f"/api/programs/{context.created_id}")
    context.response = r


@then("the program no longer appears in the BFF program list")
def step_program_not_in_list(context):
    r = bff_get("/api/programs")
    r.raise_for_status()
    ids = [p["id"] for p in r.json()]
    assert context.created_id not in ids, f"Program {context.created_id} still in list"


# ── Events ───────────────────────────────────────────────────────────────────

@when('I create an event via BFF for the saved program named "{name}"')
@given('I create an event via BFF for the saved program named "{name}"')
def step_create_event(context, name):
    r = bff_post(
        "/api/events",
        json={
            "programID": context.saved_program_id,
            "eventName": name,
            "intervals": [{"id": 0, "payloads": [{"type": "SIMPLE", "values": [1.0]}]}],
        },
    )
    context.response = r
    context.created_event_id = r.json().get("id") if r.ok else None


@when("I delete the event via BFF")
def step_delete_event(context):
    r = bff_delete(f"/api/events/{context.created_event_id}")
    context.response = r


@then("the event no longer appears in the BFF event list")
def step_event_not_in_list(context):
    r = bff_get("/api/events")
    r.raise_for_status()
    ids = [e["id"] for e in r.json()]
    assert context.created_event_id not in ids, f"Event {context.created_event_id} still in list"


# ── VENs ─────────────────────────────────────────────────────────────────────

@when("I list VENs via BFF")
def step_list_vens(context):
    context.response = bff_get("/api/vens")


def _register_ven(ven_name, attributes):
    """Register a VEN directly against the VTN (ven-manager scope), idempotent:
    if it already exists, PUT the desired attributes onto it instead."""
    token = get_token_value("ven-manager", "ven-manager")
    body = {"venName": ven_name}
    if attributes:
        body["attributes"] = attributes
    r = vtn_post("/vens", token, json=body)
    if r.status_code == 409:
        existing = vtn_get("/vens", token, params={"venName": ven_name}).json()
        match = next(v for v in existing if v["venName"] == ven_name)
        put_body = {k: v for k, v in match.items()
                    if k not in ("id", "createdDateTime", "modificationDateTime")}
        put_body["attributes"] = attributes or None
        vtn_put(f"/vens/{match['id']}", token, json=put_body).raise_for_status()
    else:
        r.raise_for_status()


@given('I register a VEN named "{ven_name}" with a DASHBOARD_URL attribute of "{url}"')
def step_register_ven_with_dashboard_url(context, ven_name, url):
    _register_ven(ven_name, [{"type": "DASHBOARD_URL", "values": [url]}])


@given('I register a VEN named "{ven_name}" with no DASHBOARD_URL attribute')
def step_register_ven_without_dashboard_url(context, ven_name):
    _register_ven(ven_name, [])


def _wait_for_ven_in_list(ven_name, timeout=12, interval=1):
    """Poll BFF's /api/vens until ven_name shows up or timeout elapses.

    The BFF caches GET /vens for CACHE_TTL_VENS (default 10s). A prior
    scenario's list call can leave a stale cached response that predates a
    VEN registered by this scenario, so a single fetch is not reliable —
    retry until the cache naturally rolls over.
    """
    deadline = time.monotonic() + timeout
    vens = []
    while True:
        r = bff_get("/api/vens")
        r.raise_for_status()
        vens = r.json()
        match = next((v for v in vens if v["venName"] == ven_name), None)
        if match is not None or time.monotonic() >= deadline:
            return match, vens
        time.sleep(interval)


@then('the listed VEN "{ven_name}" has a DASHBOARD_URL attribute of "{url}"')
def step_listed_ven_has_dashboard_url(context, ven_name, url):
    match, vens = _wait_for_ven_in_list(ven_name)
    assert match is not None, f"VEN '{ven_name}' not in BFF VEN list"
    attrs = match.get("attributes") or []
    values = next((a["values"] for a in attrs if a["type"] == "DASHBOARD_URL"), None)
    assert values == [url], f"Expected DASHBOARD_URL=[{url}], got {values}"


@then('the listed VEN "{ven_name}" has no DASHBOARD_URL attribute')
def step_listed_ven_has_no_dashboard_url(context, ven_name):
    match, vens = _wait_for_ven_in_list(ven_name)
    assert match is not None, f"VEN '{ven_name}' not in BFF VEN list"
    attrs = match.get("attributes") or []
    assert all(a["type"] != "DASHBOARD_URL" for a in attrs), f"Unexpected DASHBOARD_URL in {attrs}"


# ── Health ───────────────────────────────────────────────────────────────────

@when("I GET BFF health")
def step_get_bff_health(context):
    context.response = bff_get("/api/health")


@then("the BFF health shows VTN reachable")
def step_bff_health_vtn(context):
    data = context.response.json()
    assert data["vtn"]["reachable"] is True, f"VTN not reachable: {data}"


# ── Shared assertions ────────────────────────────────────────────────────────
# "the response status is {status:d}" is defined in vtn_auth_steps.py (shared)
# 'the response contains "{field}" equal to "{value}"' is defined in vtn_programs_steps.py (shared)
