from behave import given, when, then
from features.helpers.api_client import bff_get, bff_post, bff_put, bff_delete


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
